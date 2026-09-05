//! Database module for SQLite storage

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

use crate::models::{
    AgentJob, AgentRun, AgentRunUpdate, List, Priority, Reminder, Tag, Task, TaskDependency,
    TaskEvent, TaskStatus, TaskWorkflow,
};
use crate::sync::{RecordType, SyncRecord};

/// A single incoming sync record that could not be applied locally.
#[derive(Debug)]
pub struct SyncApplyFailure {
    pub record: SyncRecord,
    pub error: String,
}

/// Result of applying the incoming half of a sync response.
#[derive(Debug)]
pub struct SyncApplyReport {
    pub applied: usize,
    pub rejected: Vec<SyncApplyFailure>,
}

/// Database connection wrapper
pub struct Database {
    conn: Connection,
}

fn agent_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRun> {
    Ok(AgentRun {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
        task_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
        agent: row.get(2)?,
        conversation_id: row.get(3)?,
        status: row.get(4)?,
        workspace: row.get(5)?,
        branch: row.get(6)?,
        commit_sha: row.get(7)?,
        pull_request_url: row.get(8)?,
        error: row.get(9)?,
        created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(10)?)
            .unwrap()
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?)
            .unwrap()
            .with_timezone(&chrono::Utc),
    })
}

fn task_workflow_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskWorkflow> {
    let status = TaskStatus::parse(&row.get::<_, String>(1)?).unwrap_or(TaskStatus::Ready);
    Ok(TaskWorkflow {
        task_id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
        status,
        created_by: row.get(2)?,
        owner: row.get(3)?,
        review_required: row.get::<_, i32>(4)? != 0,
        blocked_reason: row.get(5)?,
        updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
            .unwrap()
            .with_timezone(&chrono::Utc),
    })
}

impl Database {
    /// Open or create the database at the default location
    pub fn open() -> Result<Self> {
        let path = Self::default_path()?;
        Self::open_path(&path)
    }

    /// Open or create the database at a specific path
    pub fn open_path(path: &PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        let conn = Connection::open(path).context("Failed to open database")?;

        let db = Self { conn };
        db.init()?;

        Ok(db)
    }

    /// Get the default database path
    pub fn default_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join("tickit");
        Ok(config_dir.join("tickit.sqlite"))
    }

    /// Execute raw SQL (for PRAGMA statements)
    pub fn execute_raw(&self, sql: &str) -> Result<()> {
        self.conn.execute(sql, [])?;
        Ok(())
    }

    /// Return whether SQLite foreign-key enforcement is enabled on this
    /// connection. Sync temporarily changes this connection-local setting.
    fn foreign_keys_enabled(&self) -> Result<bool> {
        self.conn
            .query_row("PRAGMA foreign_keys", [], |row| {
                row.get::<_, i32>(0).map(|value| value != 0)
            })
            .map_err(Into::into)
    }

    /// Apply incoming sync records in dependency order while retaining every
    /// record that fails. The caller decides whether a response with rejected
    /// records is eligible to advance `last_sync`.
    pub fn apply_sync_records(&self, records: &[SyncRecord]) -> Result<SyncApplyReport> {
        let mut lists = Vec::new();
        let mut tags = Vec::new();
        let mut tasks = Vec::new();
        let mut task_tags = Vec::new();
        let mut deletes = Vec::new();

        for record in records {
            match record {
                SyncRecord::List(_) => lists.push(record),
                SyncRecord::Tag(_) => tags.push(record),
                SyncRecord::Task(_) => tasks.push(record),
                SyncRecord::TaskTag(_) => task_tags.push(record),
                SyncRecord::Deleted { .. } => deletes.push(record),
            }
        }

        let foreign_keys_enabled = self.foreign_keys_enabled()?;
        if foreign_keys_enabled {
            self.execute_raw("PRAGMA foreign_keys = OFF")?;
        }

        let mut report = SyncApplyReport {
            applied: 0,
            rejected: Vec::new(),
        };
        for record in lists
            .into_iter()
            .chain(tags)
            .chain(tasks)
            .chain(task_tags)
            .chain(deletes)
        {
            match self.apply_sync_record(record) {
                Ok(()) => report.applied += 1,
                Err(error) => report.rejected.push(SyncApplyFailure {
                    record: record.clone(),
                    error: format!("{error:#}"),
                }),
            }
        }

        // Always restore the connection-local setting, including when one or
        // more records were rejected. Never silently leave FK checks disabled.
        let restore = if foreign_keys_enabled {
            self.execute_raw("PRAGMA foreign_keys = ON")
        } else {
            self.execute_raw("PRAGMA foreign_keys = OFF")
        };
        restore.context("failed to restore SQLite foreign-key enforcement")?;

        Ok(report)
    }

    fn apply_sync_record(&self, record: &SyncRecord) -> Result<()> {
        match record {
            SyncRecord::Task(task) => self.upsert_task(task),
            SyncRecord::List(list) => self.upsert_list(list),
            SyncRecord::Tag(tag) => self.upsert_tag(tag),
            SyncRecord::TaskTag(link) => self.upsert_task_tag(link),
            SyncRecord::Deleted {
                id, record_type, ..
            } => match record_type {
                RecordType::Task => self.delete_task_by_id(*id),
                RecordType::List => self.delete_list_by_id(*id),
                RecordType::Tag => self.delete_tag_by_id(*id),
                RecordType::TaskTag => {
                    anyhow::bail!("task-tag tombstones cannot identify both link endpoints")
                }
            },
        }
    }

    /// Initialize the database schema
    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            -- Lists table
            CREATE TABLE IF NOT EXISTS lists (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                icon TEXT NOT NULL DEFAULT '📋',
                color TEXT,
                is_inbox INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            -- Tags table
            CREATE TABLE IF NOT EXISTS tags (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                color TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            -- Tasks table
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT,
                url TEXT,
                priority TEXT NOT NULL DEFAULT 'medium',
                completed INTEGER NOT NULL DEFAULT 0,
                list_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                completed_at TEXT,
                due_date TEXT,
                FOREIGN KEY (list_id) REFERENCES lists(id) ON DELETE CASCADE
            );

            -- Task-Tag junction table
            CREATE TABLE IF NOT EXISTS task_tags (
                task_id TEXT NOT NULL,
                tag_id TEXT NOT NULL,
                PRIMARY KEY (task_id, tag_id),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
                FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
            );

            -- Indexes for common queries
            CREATE INDEX IF NOT EXISTS idx_tasks_list ON tasks(list_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_completed ON tasks(completed);
            CREATE INDEX IF NOT EXISTS idx_tasks_priority ON tasks(priority);
            CREATE INDEX IF NOT EXISTS idx_task_tags_task ON task_tags(task_id);
            CREATE INDEX IF NOT EXISTS idx_task_tags_tag ON task_tags(tag_id);

            -- Sync tracking table (for deleted records)
            CREATE TABLE IF NOT EXISTS sync_tombstones (
                id TEXT PRIMARY KEY,
                record_type TEXT NOT NULL,
                deleted_at TEXT NOT NULL
            );

            -- Sync state table
            CREATE TABLE IF NOT EXISTS sync_state (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- Local notification delivery ledger. This is intentionally not synced.
            CREATE TABLE IF NOT EXISTS reminder_deliveries (
                task_id TEXT NOT NULL,
                reminder_kind TEXT NOT NULL,
                due_date TEXT NOT NULL,
                delivered_at TEXT NOT NULL,
                PRIMARY KEY (task_id, reminder_kind, due_date)
            );

            -- Explicit reminders are device-local and intentionally not synced.
            CREATE TABLE IF NOT EXISTS reminders (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                scheduled_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                claimed_at TEXT,
                delivered_at TEXT,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_reminders_task ON reminders(task_id, scheduled_at);
            CREATE INDEX IF NOT EXISTS idx_reminders_pending
                ON reminders(delivered_at, scheduled_at, claimed_at);

            -- Project-scoped tag vocabulary. Existing global tags continue to
            -- work; rows here opt a tag into a particular list/project.
            CREATE TABLE IF NOT EXISTS project_tags (
                list_id TEXT NOT NULL,
                tag_id TEXT NOT NULL,
                PRIMARY KEY (list_id, tag_id),
                FOREIGN KEY (list_id) REFERENCES lists(id) ON DELETE CASCADE,
                FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS task_workflow (
                task_id TEXT PRIMARY KEY,
                status TEXT NOT NULL DEFAULT 'ready',
                created_by TEXT,
                owner TEXT,
                review_required INTEGER NOT NULL DEFAULT 0,
                blocked_reason TEXT,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS task_dependencies (
                task_id TEXT NOT NULL,
                depends_on TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (task_id, depends_on),
                CHECK (task_id <> depends_on),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
                FOREIGN KEY (depends_on) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS task_events (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                actor TEXT NOT NULL,
                event_type TEXT NOT NULL,
                message TEXT,
                metadata TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS agent_runs (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                agent TEXT NOT NULL,
                conversation_id TEXT,
                status TEXT NOT NULL,
                workspace TEXT,
                branch TEXT,
                commit_sha TEXT,
                pull_request_url TEXT,
                error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS agent_jobs (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                agent TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'queued',
                instructions TEXT,
                claimed_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_task_workflow_status
                ON task_workflow(status);
            CREATE INDEX IF NOT EXISTS idx_task_dependencies_prerequisite
                ON task_dependencies(depends_on);
            CREATE INDEX IF NOT EXISTS idx_agent_jobs_status
                ON agent_jobs(status, created_at);

            CREATE INDEX IF NOT EXISTS idx_tombstones_deleted ON sync_tombstones(deleted_at);
            "#,
        )?;

        // Run migrations for existing databases
        self.migrate()?;

        self.ensure_workflow_records()?;

        // Ensure inbox list exists
        self.ensure_inbox()?;

        Ok(())
    }

    /// Run database migrations
    fn migrate(&self) -> Result<()> {
        // Check if tags.updated_at column exists
        let has_updated_at: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('tags') WHERE name = 'updated_at'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !has_updated_at {
            // Add updated_at column with default value of created_at
            self.conn.execute_batch(
                r#"
                ALTER TABLE tags ADD COLUMN updated_at TEXT;
                UPDATE tags SET updated_at = created_at WHERE updated_at IS NULL;
                "#,
            )?;
        }

        // `CREATE TRIGGER IF NOT EXISTS` does not update an existing trigger.
        // Recreate this trigger so older databases also allow cancellation to
        // complete/ hide a task even when its prerequisites are still open.
        self.conn.execute_batch(
            r#"
            DROP TRIGGER IF EXISTS prevent_task_completion_with_open_dependencies;
            CREATE TRIGGER prevent_task_completion_with_open_dependencies
            BEFORE UPDATE OF completed ON tasks
            WHEN NEW.completed = 1
              AND COALESCE(
                    (SELECT status FROM task_workflow WHERE task_id = NEW.id),
                    'ready'
                  ) <> 'cancelled'
              AND EXISTS (
                SELECT 1
                FROM task_dependencies d
                LEFT JOIN task_workflow w ON w.task_id = d.depends_on
                WHERE d.task_id = NEW.id
                  AND COALESCE(w.status, 'ready') NOT IN ('done', 'verified')
              )
            BEGIN
                SELECT RAISE(ABORT, 'task has incomplete prerequisites');
            END;
            "#,
        )?;

        Ok(())
    }

    fn ensure_workflow_records(&self) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO task_workflow (task_id, status, updated_at)
             SELECT id, CASE WHEN completed = 1 THEN 'done' ELSE 'ready' END, updated_at
             FROM tasks",
            [],
        )?;
        Ok(())
    }

    /// Ensure the inbox list exists
    fn ensure_inbox(&self) -> Result<()> {
        let count: i32 =
            self.conn
                .query_row("SELECT COUNT(*) FROM lists WHERE is_inbox = 1", [], |row| {
                    row.get(0)
                })?;

        if count == 0 {
            let inbox = List::inbox();
            self.insert_list(&inbox)?;
        }

        Ok(())
    }

    // ==================== Lists ====================

    /// Insert a new list
    pub fn insert_list(&self, list: &List) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO lists (id, name, description, icon, color, is_inbox, sort_order, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            params![
                list.id.to_string(),
                list.name,
                list.description,
                list.icon,
                list.color,
                list.is_inbox as i32,
                list.sort_order,
                list.created_at.to_rfc3339(),
                list.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Get all lists
    pub fn get_lists(&self) -> Result<Vec<List>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, icon, color, is_inbox, sort_order, created_at, updated_at 
             FROM lists ORDER BY sort_order, name"
        )?;

        let lists = stmt.query_map([], |row| {
            Ok(List {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                name: row.get(1)?,
                description: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                is_inbox: row.get::<_, i32>(5)? != 0,
                sort_order: row.get(6)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            })
        })?;

        lists.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get the inbox list
    pub fn get_inbox(&self) -> Result<List> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, icon, color, is_inbox, sort_order, created_at, updated_at 
             FROM lists WHERE is_inbox = 1"
        )?;

        stmt.query_row([], |row| {
            Ok(List {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                name: row.get(1)?,
                description: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                is_inbox: row.get::<_, i32>(5)? != 0,
                sort_order: row.get(6)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            })
        })
        .map_err(Into::into)
    }

    /// Update a list
    pub fn update_list(&self, list: &List) -> Result<()> {
        self.conn.execute(
            r#"UPDATE lists SET name = ?2, description = ?3, icon = ?4, color = ?5, 
               sort_order = ?6, updated_at = ?7 WHERE id = ?1"#,
            params![
                list.id.to_string(),
                list.name,
                list.description,
                list.icon,
                list.color,
                list.sort_order,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Delete a list (moves tasks to inbox)
    pub fn delete_list(&self, list_id: Uuid) -> Result<()> {
        let inbox = self.get_inbox()?;

        // Move tasks to inbox
        self.conn.execute(
            "UPDATE tasks SET list_id = ?1 WHERE list_id = ?2",
            params![inbox.id.to_string(), list_id.to_string()],
        )?;

        // Delete the list
        self.conn.execute(
            "DELETE FROM project_tags WHERE list_id = ?1",
            params![list_id.to_string()],
        )?;
        self.conn.execute(
            "DELETE FROM lists WHERE id = ?1 AND is_inbox = 0",
            params![list_id.to_string()],
        )?;

        Ok(())
    }

    // ==================== Tags ====================

    /// Insert a new tag
    pub fn insert_tag(&self, tag: &Tag) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tags (id, name, color, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                tag.id.to_string(),
                tag.name,
                tag.color,
                tag.created_at.to_rfc3339(),
                tag.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Get all tags
    pub fn get_tags(&self) -> Result<Vec<Tag>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, color, created_at, updated_at FROM tags ORDER BY name")?;

        let tags = stmt.query_map([], |row| {
            let created_at = chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                .unwrap()
                .with_timezone(&chrono::Utc);
            let updated_at = row
                .get::<_, Option<String>>(4)?
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or(created_at);
            Ok(Tag {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                name: row.get(1)?,
                color: row.get(2)?,
                created_at,
                updated_at,
            })
        })?;

        tags.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Update a tag
    pub fn update_tag(&self, tag: &Tag) -> Result<()> {
        self.conn.execute(
            "UPDATE tags SET name = ?2, color = ?3, updated_at = ?4 WHERE id = ?1",
            params![
                tag.id.to_string(),
                tag.name,
                tag.color,
                tag.updated_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Delete a tag
    pub fn delete_tag(&self, tag_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM tags WHERE id = ?1",
            params![tag_id.to_string()],
        )?;
        Ok(())
    }

    // ==================== Tasks ====================

    fn validate_task_tags(&self, task: &Task) -> Result<()> {
        let list_exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM lists WHERE id = ?1)",
            params![task.list_id.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(list_exists, "project/list not found: {}", task.list_id);

        for tag_id in &task.tag_ids {
            let tag_exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM tags WHERE id = ?1)",
                params![tag_id.to_string()],
                |row| row.get(0),
            )?;
            anyhow::ensure!(tag_exists, "tag not found: {tag_id}");

            // An unscoped tag remains valid everywhere. Once a tag has one or
            // more project scopes, it is valid only in one of those projects;
            // sharing a tag across multiple projects is supported.
            let invalid_scope: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM project_tags WHERE tag_id = ?1)
                        AND NOT EXISTS(
                            SELECT 1 FROM project_tags WHERE tag_id = ?1 AND list_id = ?2
                        )",
                params![tag_id.to_string(), task.list_id.to_string()],
                |row| row.get(0),
            )?;
            anyhow::ensure!(
                !invalid_scope,
                "tag {tag_id} is scoped to a different project/list"
            );
        }
        Ok(())
    }

    /// Insert a new task
    pub fn insert_task(&self, task: &Task) -> Result<()> {
        self.insert_task_with_reminders(task, &[])
    }

    /// Get all tasks for a list
    pub fn get_tasks_for_list(&self, list_id: Uuid) -> Result<Vec<Task>> {
        self.get_tasks_with_filter(Some(list_id), None, None)
    }

    /// Get all tasks
    pub fn get_all_tasks(&self) -> Result<Vec<Task>> {
        self.get_tasks_with_filter(None, None, None)
    }

    /// Atomically claim a reminder delivery, returning false if it was already claimed.
    pub fn claim_reminder_delivery(
        &self,
        task_id: Uuid,
        reminder_kind: &str,
        due_date: &str,
    ) -> Result<bool> {
        let inserted = self.conn.execute(
            r#"INSERT OR IGNORE INTO reminder_deliveries
               (task_id, reminder_kind, due_date, delivered_at)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                task_id.to_string(),
                reminder_kind,
                due_date,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(inserted == 1)
    }

    /// Release a failed reminder delivery so a later check can retry it.
    pub fn release_reminder_delivery(
        &self,
        task_id: Uuid,
        reminder_kind: &str,
        due_date: &str,
    ) -> Result<()> {
        self.conn.execute(
            r#"DELETE FROM reminder_deliveries
               WHERE task_id = ?1 AND reminder_kind = ?2 AND due_date = ?3"#,
            params![task_id.to_string(), reminder_kind, due_date],
        )?;
        Ok(())
    }

    // ==================== Explicit reminders (local only) ====================

    pub fn create_reminder(&self, reminder: &Reminder) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO reminders
               (id, task_id, scheduled_at, created_at, updated_at, claimed_at, delivered_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
            params![
                reminder.id.to_string(),
                reminder.task_id.to_string(),
                reminder.scheduled_at.to_rfc3339(),
                reminder.created_at.to_rfc3339(),
                reminder.updated_at.to_rfc3339(),
                reminder.claimed_at.map(|value| value.to_rfc3339()),
                reminder.delivered_at.map(|value| value.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Insert a task and its local reminders atomically.
    pub fn insert_task_with_reminders(&self, task: &Task, reminders: &[Reminder]) -> Result<()> {
        self.validate_task_tags(task)?;
        for reminder in reminders {
            anyhow::ensure!(
                reminder.task_id == task.id,
                "reminder {} belongs to a different task",
                reminder.id
            );
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            r#"INSERT INTO tasks (id, title, description, url, priority, completed, list_id,
               created_at, updated_at, completed_at, due_date)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"#,
            params![
                task.id.to_string(),
                task.title,
                task.description,
                task.url,
                format!("{:?}", task.priority).to_lowercase(),
                task.completed as i32,
                task.list_id.to_string(),
                task.created_at.to_rfc3339(),
                task.updated_at.to_rfc3339(),
                task.completed_at.map(|v| v.to_rfc3339()),
                task.due_date.map(|v| v.to_rfc3339())
            ],
        )?;
        for tag_id in &task.tag_ids {
            tx.execute(
                "INSERT OR IGNORE INTO task_tags (task_id, tag_id) VALUES (?1, ?2)",
                params![task.id.to_string(), tag_id.to_string()],
            )?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO task_workflow (task_id, status, updated_at) VALUES (?1, ?2, ?3)",
            params![
                task.id.to_string(),
                if task.completed { "done" } else { "ready" },
                task.updated_at.to_rfc3339()
            ],
        )?;
        for reminder in reminders {
            tx.execute(
                r#"INSERT INTO reminders
                (id, task_id, scheduled_at, created_at, updated_at, claimed_at, delivered_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
                params![
                    reminder.id.to_string(),
                    reminder.task_id.to_string(),
                    reminder.scheduled_at.to_rfc3339(),
                    reminder.created_at.to_rfc3339(),
                    reminder.updated_at.to_rfc3339(),
                    reminder.claimed_at.map(|v| v.to_rfc3339()),
                    reminder.delivered_at.map(|v| v.to_rfc3339())
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Replace only pending reminders, retaining delivered history.
    pub fn replace_pending_reminders(&self, task_id: Uuid, reminders: &[Reminder]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM reminders WHERE task_id = ?1 AND delivered_at IS NULL",
            params![task_id.to_string()],
        )?;
        for reminder in reminders {
            tx.execute(
                r#"INSERT INTO reminders
                (id, task_id, scheduled_at, created_at, updated_at, claimed_at, delivered_at)
                VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)"#,
                params![
                    reminder.id.to_string(),
                    reminder.task_id.to_string(),
                    reminder.scheduled_at.to_rfc3339(),
                    reminder.created_at.to_rfc3339(),
                    reminder.updated_at.to_rfc3339()
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_reminder(&self, id: Uuid) -> Result<Option<Reminder>> {
        let mut reminders = self.query_reminders(
            "SELECT id, task_id, scheduled_at, created_at, updated_at, claimed_at, delivered_at FROM reminders WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(reminders.pop())
    }

    pub fn list_reminders(&self) -> Result<Vec<Reminder>> {
        self.query_reminders(
            "SELECT id, task_id, scheduled_at, created_at, updated_at, claimed_at, delivered_at FROM reminders ORDER BY scheduled_at, id",
            [],
        )
    }

    pub fn list_reminders_for_task(&self, task_id: Uuid) -> Result<Vec<Reminder>> {
        self.query_reminders(
            "SELECT id, task_id, scheduled_at, created_at, updated_at, claimed_at, delivered_at FROM reminders WHERE task_id = ?1 ORDER BY scheduled_at, id",
            params![task_id.to_string()],
        )
    }

    pub fn delete_reminder(&self, id: Uuid) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM reminders WHERE id = ?1",
            params![id.to_string()],
        )? == 1)
    }

    /// Claim eligible reminders in one SQLite statement. Existing live claims and
    /// reminders for completed or deleted tasks are excluded.
    pub fn claim_due_reminders(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        grace_cutoff: chrono::DateTime<chrono::Utc>,
        lease_cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<Reminder>> {
        let sql = r#"
            UPDATE reminders
               SET claimed_at = ?1, updated_at = ?1
             WHERE id IN (
                 SELECT r.id FROM reminders r
                 JOIN tasks t ON t.id = r.task_id
                 WHERE r.delivered_at IS NULL
                   AND r.scheduled_at <= ?1
                   AND r.scheduled_at >= ?2
                   AND (r.claimed_at IS NULL OR r.claimed_at <= ?3)
                   AND t.completed = 0
                   AND COALESCE(
                         (SELECT status FROM task_workflow WHERE task_id = t.id),
                         'ready'
                       ) <> 'cancelled'
             )
             RETURNING id, task_id, scheduled_at, created_at, updated_at, claimed_at, delivered_at
        "#;
        self.query_reminders(
            sql,
            params![
                now.to_rfc3339(),
                grace_cutoff.to_rfc3339(),
                lease_cutoff.to_rfc3339()
            ],
        )
    }

    pub fn mark_reminder_delivered(
        &self,
        id: Uuid,
        delivered_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE reminders SET delivered_at = ?2, claimed_at = NULL, updated_at = ?2 WHERE id = ?1 AND delivered_at IS NULL AND claimed_at IS NOT NULL",
            params![id.to_string(), delivered_at.to_rfc3339()],
        )? == 1)
    }

    pub fn release_reminder_claim(&self, id: Uuid) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE reminders SET claimed_at = NULL, updated_at = ?2 WHERE id = ?1 AND delivered_at IS NULL",
            params![id.to_string(), chrono::Utc::now().to_rfc3339()],
        )? == 1)
    }

    pub fn snooze_reminder(
        &self,
        id: Uuid,
        scheduled_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE reminders SET scheduled_at = ?2, claimed_at = NULL, delivered_at = NULL, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), scheduled_at.to_rfc3339(), chrono::Utc::now().to_rfc3339()],
        )? == 1)
    }

    fn query_reminders<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<Reminder>> {
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params)?;
        let mut reminders = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let task_id: String = row.get(1)?;
            let scheduled_at: String = row.get(2)?;
            let created_at: String = row.get(3)?;
            let updated_at: String = row.get(4)?;
            let claimed_at: Option<String> = row.get(5)?;
            let delivered_at: Option<String> = row.get(6)?;
            reminders.push(Reminder {
                id: Uuid::parse_str(&id).context("Invalid reminder ID in database")?,
                task_id: Uuid::parse_str(&task_id)
                    .context("Invalid reminder task ID in database")?,
                scheduled_at: chrono::DateTime::parse_from_rfc3339(&scheduled_at)
                    .context("Invalid reminder scheduled_at in database")?
                    .with_timezone(&chrono::Utc),
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                    .context("Invalid reminder created_at in database")?
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at)
                    .context("Invalid reminder updated_at in database")?
                    .with_timezone(&chrono::Utc),
                claimed_at: claimed_at
                    .map(|value| chrono::DateTime::parse_from_rfc3339(&value))
                    .transpose()
                    .context("Invalid reminder claimed_at in database")?
                    .map(|value| value.with_timezone(&chrono::Utc)),
                delivered_at: delivered_at
                    .map(|value| chrono::DateTime::parse_from_rfc3339(&value))
                    .transpose()
                    .context("Invalid reminder delivered_at in database")?
                    .map(|value| value.with_timezone(&chrono::Utc)),
            });
        }
        Ok(reminders)
    }

    /// Get tasks with optional filters
    pub fn get_tasks_with_filter(
        &self,
        list_id: Option<Uuid>,
        completed: Option<bool>,
        tag_id: Option<Uuid>,
    ) -> Result<Vec<Task>> {
        let mut sql = String::from(
            "SELECT DISTINCT t.id, t.title, t.description, t.url, t.priority, t.completed, 
             t.list_id, t.created_at, t.updated_at, t.completed_at, t.due_date
             FROM tasks t",
        );

        let mut conditions = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if tag_id.is_some() {
            sql.push_str(" JOIN task_tags tt ON t.id = tt.task_id");
        }

        if let Some(lid) = list_id {
            conditions.push("t.list_id = ?");
            params_vec.push(Box::new(lid.to_string()));
        }

        if let Some(c) = completed {
            conditions.push("t.completed = ?");
            params_vec.push(Box::new(c as i32));
        }

        if let Some(tid) = tag_id {
            conditions.push("tt.tag_id = ?");
            params_vec.push(Box::new(tid.to_string()));
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" ORDER BY t.completed, t.priority DESC, t.created_at DESC");

        let mut stmt = self.conn.prepare(&sql)?;

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        // First collect just the task IDs
        let task_ids: Vec<String> = stmt
            .query_map(params_refs.as_slice(), |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut result = Vec::new();
        for task_id in task_ids {
            // Get fresh row for this task
            let mut task_stmt = self.conn.prepare(
                "SELECT id, title, description, url, priority, completed, list_id, 
                 created_at, updated_at, completed_at, due_date FROM tasks WHERE id = ?1",
            )?;

            let task = task_stmt.query_row(params![task_id], |row| {
                let priority_str: String = row.get(4)?;
                let priority = match priority_str.as_str() {
                    "low" => Priority::Low,
                    "high" => Priority::High,
                    "urgent" => Priority::Urgent,
                    _ => Priority::Medium,
                };

                Ok(Task {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    title: row.get(1)?,
                    description: row.get(2)?,
                    url: row.get(3)?,
                    priority,
                    completed: row.get::<_, i32>(5)? != 0,
                    list_id: Uuid::parse_str(&row.get::<_, String>(6)?).unwrap(),
                    tag_ids: Vec::new(), // Filled below
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                    updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                    completed_at: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc)),
                    due_date: row
                        .get::<_, Option<String>>(10)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc)),
                })
            })?;

            // Get tags for this task
            let mut task = task;
            task.tag_ids = self.get_task_tags(task.id)?;
            result.push(task);
        }

        Ok(result)
    }

    /// Get tag IDs for a task
    fn get_task_tags(&self, task_id: Uuid) -> Result<Vec<Uuid>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag_id FROM task_tags WHERE task_id = ?1")?;

        let tags = stmt.query_map(params![task_id.to_string()], |row| {
            Ok(Uuid::parse_str(&row.get::<_, String>(0)?).unwrap())
        })?;

        tags.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Update a task
    pub fn update_task(&self, task: &Task) -> Result<()> {
        self.validate_task_tags(task)?;
        let current_status = self
            .get_task_workflow(task.id)?
            .map(|workflow| workflow.status);
        anyhow::ensure!(
            task.completed || current_status != Some(TaskStatus::Cancelled),
            "cancelled tasks are terminal; change workflow status explicitly to reopen"
        );
        let now = chrono::Utc::now();
        let workflow_status = match (current_status, task.completed) {
            (Some(TaskStatus::Cancelled), _) => TaskStatus::Cancelled,
            (Some(TaskStatus::Verified), true) => TaskStatus::Verified,
            (Some(TaskStatus::Verified), false) => TaskStatus::Ready,
            (Some(TaskStatus::Done), false) => TaskStatus::Ready,
            (Some(_status), true) => TaskStatus::Done,
            (Some(status), false) => status,
            (None, true) => TaskStatus::Done,
            (None, false) => TaskStatus::Ready,
        };
        let stored_completed = workflow_status == TaskStatus::Cancelled || task.completed;
        let completed_at = if stored_completed {
            task.completed_at.or(Some(now))
        } else {
            None
        };
        self.conn.execute(
            r#"UPDATE tasks SET title = ?2, description = ?3, url = ?4, priority = ?5, 
               completed = ?6, list_id = ?7, updated_at = ?8, completed_at = ?9, due_date = ?10 
               WHERE id = ?1"#,
            params![
                task.id.to_string(),
                task.title,
                task.description,
                task.url,
                format!("{:?}", task.priority).to_lowercase(),
                stored_completed as i32,
                task.list_id.to_string(),
                now.to_rfc3339(),
                completed_at.map(|dt| dt.to_rfc3339()),
                task.due_date.map(|dt| dt.to_rfc3339()),
            ],
        )?;

        // Update tag associations
        self.conn.execute(
            "DELETE FROM task_tags WHERE task_id = ?1",
            params![task.id.to_string()],
        )?;

        for tag_id in &task.tag_ids {
            self.conn.execute(
                "INSERT INTO task_tags (task_id, tag_id) VALUES (?1, ?2)",
                params![task.id.to_string(), tag_id.to_string()],
            )?;
        }

        self.conn.execute(
            "INSERT OR IGNORE INTO task_workflow (task_id, status, updated_at) VALUES (?1, ?2, ?3)",
            params![
                task.id.to_string(),
                workflow_status.as_str(),
                now.to_rfc3339()
            ],
        )?;
        self.conn.execute(
            "UPDATE task_workflow SET status = ?2, updated_at = ?3 WHERE task_id = ?1",
            params![
                task.id.to_string(),
                workflow_status.as_str(),
                now.to_rfc3339()
            ],
        )?;

        Ok(())
    }

    /// Delete a task
    pub fn delete_task(&self, task_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM reminders WHERE task_id = ?1",
            params![task_id.to_string()],
        )?;
        self.delete_task_workflow_records(task_id)?;
        self.conn.execute(
            "DELETE FROM tasks WHERE id = ?1",
            params![task_id.to_string()],
        )?;
        Ok(())
    }

    fn delete_task_workflow_records(&self, task_id: Uuid) -> Result<()> {
        for table in [
            "task_events",
            "agent_runs",
            "agent_jobs",
            "task_dependencies",
            "task_workflow",
        ] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE task_id = ?1"),
                params![task_id.to_string()],
            )?;
        }
        self.conn.execute(
            "DELETE FROM task_dependencies WHERE depends_on = ?1",
            params![task_id.to_string()],
        )?;
        Ok(())
    }

    /// Get task count for a list
    pub fn get_task_count(&self, list_id: Uuid, include_completed: bool) -> Result<i32> {
        let sql = if include_completed {
            "SELECT COUNT(*) FROM tasks WHERE list_id = ?1"
        } else {
            "SELECT COUNT(*) FROM tasks WHERE list_id = ?1 AND completed = 0"
        };

        self.conn
            .query_row(sql, params![list_id.to_string()], |row| row.get(0))
            .map_err(Into::into)
    }

    /// Get total task count
    pub fn get_total_task_count(&self, include_completed: bool) -> Result<i32> {
        let sql = if include_completed {
            "SELECT COUNT(*) FROM tasks"
        } else {
            "SELECT COUNT(*) FROM tasks WHERE completed = 0"
        };

        self.conn
            .query_row(sql, [], |row| row.get(0))
            .map_err(Into::into)
    }

    // ==================== Sync ====================

    /// Record a tombstone for a deleted record (for sync)
    pub fn record_tombstone(&self, id: Uuid, record_type: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_tombstones (id, record_type, deleted_at) VALUES (?1, ?2, ?3)",
            params![id.to_string(), record_type, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Get tombstones since a given time
    pub fn get_tombstones_since(
        &self,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<(Uuid, String, chrono::DateTime<chrono::Utc>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, record_type, deleted_at FROM sync_tombstones WHERE deleted_at > ?1",
        )?;

        let rows = stmt.query_map(params![since.to_rfc3339()], |row| {
            Ok((
                Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                row.get::<_, String>(1)?,
                chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            ))
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get all tombstones
    pub fn get_all_tombstones(&self) -> Result<Vec<(Uuid, String, chrono::DateTime<chrono::Utc>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, record_type, deleted_at FROM sync_tombstones")?;

        let rows = stmt.query_map([], |row| {
            Ok((
                Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                row.get::<_, String>(1)?,
                chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            ))
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Clear old tombstones (older than given time)
    pub fn clear_old_tombstones(&self, older_than: chrono::DateTime<chrono::Utc>) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM sync_tombstones WHERE deleted_at < ?1",
            params![older_than.to_rfc3339()],
        )?;
        Ok(count)
    }

    /// Get sync state value
    pub fn get_sync_state(&self, key: &str) -> Result<Option<String>> {
        let result = self.conn.query_row(
            "SELECT value FROM sync_state WHERE key = ?1",
            params![key],
            |row| row.get(0),
        );

        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Set sync state value
    pub fn set_sync_state(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_state (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Get last sync timestamp
    pub fn get_last_sync(&self) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
        if let Some(value) = self.get_sync_state("last_sync")? {
            Ok(chrono::DateTime::parse_from_rfc3339(&value)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc)))
        } else {
            Ok(None)
        }
    }

    /// Set last sync timestamp
    pub fn set_last_sync(&self, timestamp: chrono::DateTime<chrono::Utc>) -> Result<()> {
        self.set_sync_state("last_sync", &timestamp.to_rfc3339())
    }

    /// Get tasks modified since a given time
    pub fn get_tasks_since(&self, since: chrono::DateTime<chrono::Utc>) -> Result<Vec<Task>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM tasks WHERE updated_at > ?1")?;

        let task_ids: Vec<String> = stmt
            .query_map(params![since.to_rfc3339()], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut result = Vec::new();
        for task_id in task_ids {
            if let Ok(task) = self.get_task_by_id(&task_id) {
                result.push(task);
            }
        }

        Ok(result)
    }

    /// Get a task by ID
    fn get_task_by_id(&self, task_id: &str) -> Result<Task> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, description, url, priority, completed, list_id, 
             created_at, updated_at, completed_at, due_date FROM tasks WHERE id = ?1",
        )?;

        let task = stmt.query_row(params![task_id], |row| {
            let priority_str: String = row.get(4)?;
            let priority = match priority_str.as_str() {
                "low" => Priority::Low,
                "high" => Priority::High,
                "urgent" => Priority::Urgent,
                _ => Priority::Medium,
            };

            Ok(Task {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                title: row.get(1)?,
                description: row.get(2)?,
                url: row.get(3)?,
                priority,
                completed: row.get::<_, i32>(5)? != 0,
                list_id: Uuid::parse_str(&row.get::<_, String>(6)?).unwrap(),
                tag_ids: Vec::new(),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                completed_at: row
                    .get::<_, Option<String>>(9)?
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc)),
                due_date: row
                    .get::<_, Option<String>>(10)?
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc)),
            })
        })?;

        let mut task = task;
        task.tag_ids = self.get_task_tags(task.id)?;
        Ok(task)
    }

    /// Get lists modified since a given time
    pub fn get_lists_since(&self, since: chrono::DateTime<chrono::Utc>) -> Result<Vec<List>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, icon, color, is_inbox, sort_order, created_at, updated_at 
             FROM lists WHERE updated_at > ?1"
        )?;

        let lists = stmt.query_map(params![since.to_rfc3339()], |row| {
            Ok(List {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                name: row.get(1)?,
                description: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                is_inbox: row.get::<_, i32>(5)? != 0,
                sort_order: row.get(6)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            })
        })?;

        lists.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get tags modified since a given time
    pub fn get_tags_since(&self, since: chrono::DateTime<chrono::Utc>) -> Result<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, color, created_at, updated_at FROM tags WHERE updated_at > ?1",
        )?;

        let tags = stmt.query_map(params![since.to_rfc3339()], |row| {
            let created_at = chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                .unwrap()
                .with_timezone(&chrono::Utc);
            let updated_at = row
                .get::<_, Option<String>>(4)?
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or(created_at);
            Ok(Tag {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                name: row.get(1)?,
                color: row.get(2)?,
                created_at,
                updated_at,
            })
        })?;

        tags.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Upsert a task (insert or update based on updated_at)
    pub fn upsert_task(&self, task: &Task) -> Result<()> {
        // Check if task exists and compare timestamps
        let existing = self.conn.query_row(
            "SELECT updated_at FROM tasks WHERE id = ?1",
            params![task.id.to_string()],
            |row| row.get::<_, String>(0),
        );

        match existing {
            Ok(existing_updated) => {
                let existing_dt = chrono::DateTime::parse_from_rfc3339(&existing_updated)
                    .unwrap()
                    .with_timezone(&chrono::Utc);
                // Only update if incoming is newer
                if task.updated_at > existing_dt {
                    self.update_task(task)?;
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.insert_task(task)?;
            }
            Err(e) => return Err(e.into()),
        }

        Ok(())
    }

    /// Upsert a list (insert or update based on updated_at)
    pub fn upsert_list(&self, list: &List) -> Result<()> {
        // Special handling for inbox: if incoming list is an inbox, merge with local inbox
        if list.is_inbox {
            let local_inbox = self.get_inbox()?;
            if local_inbox.id != list.id {
                // Different inbox IDs - this is a sync from another device
                // Update all tasks that reference the remote inbox to use local inbox
                self.conn.execute(
                    "UPDATE tasks SET list_id = ?1 WHERE list_id = ?2",
                    params![local_inbox.id.to_string(), list.id.to_string()],
                )?;
                // Don't insert the remote inbox, keep using local one
                // But update local inbox metadata if remote is newer
                if list.updated_at > local_inbox.updated_at {
                    self.conn.execute(
                        "UPDATE lists SET name = ?1, description = ?2, icon = ?3 WHERE id = ?4",
                        params![
                            list.name,
                            list.description,
                            list.icon,
                            local_inbox.id.to_string()
                        ],
                    )?;
                }
                return Ok(());
            }
        }

        let existing = self.conn.query_row(
            "SELECT updated_at FROM lists WHERE id = ?1",
            params![list.id.to_string()],
            |row| row.get::<_, String>(0),
        );

        match existing {
            Ok(existing_updated) => {
                let existing_dt = chrono::DateTime::parse_from_rfc3339(&existing_updated)
                    .unwrap()
                    .with_timezone(&chrono::Utc);
                if list.updated_at > existing_dt {
                    self.update_list(list)?;
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.insert_list(list)?;
            }
            Err(e) => return Err(e.into()),
        }

        Ok(())
    }

    /// Delete a task by ID (used by sync to apply remote deletes)
    pub fn delete_task_by_id(&self, task_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM reminders WHERE task_id = ?1",
            params![task_id.to_string()],
        )?;
        self.delete_task_workflow_records(task_id)?;
        self.conn.execute(
            "DELETE FROM tasks WHERE id = ?1",
            params![task_id.to_string()],
        )?;
        Ok(())
    }

    /// Delete a list by ID (used by sync to apply remote deletes)
    pub fn delete_list_by_id(&self, list_id: Uuid) -> Result<()> {
        // Don't delete inbox
        let inbox = self.get_inbox()?;
        self.conn.execute(
            "UPDATE tasks SET list_id = ?1 WHERE list_id = ?2",
            params![inbox.id.to_string(), list_id.to_string()],
        )?;
        self.conn.execute(
            "DELETE FROM project_tags WHERE list_id = ?1",
            params![list_id.to_string()],
        )?;
        self.conn.execute(
            "DELETE FROM lists WHERE id = ?1 AND is_inbox = 0",
            params![list_id.to_string()],
        )?;
        Ok(())
    }

    /// Delete a tag by ID (used by sync to apply remote deletes)
    pub fn delete_tag_by_id(&self, tag_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM task_tags WHERE tag_id = ?1",
            params![tag_id.to_string()],
        )?;
        self.conn.execute(
            "DELETE FROM project_tags WHERE tag_id = ?1",
            params![tag_id.to_string()],
        )?;
        self.conn.execute(
            "DELETE FROM tags WHERE id = ?1",
            params![tag_id.to_string()],
        )?;
        Ok(())
    }

    /// Upsert a tag (insert or replace)
    pub fn upsert_tag(&self, tag: &Tag) -> Result<()> {
        self.conn.execute(
            r#"INSERT OR REPLACE INTO tags (id, name, color, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
            params![
                tag.id.to_string(),
                tag.name,
                tag.color,
                tag.created_at.to_rfc3339(),
                tag.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Upsert a task-tag link
    pub fn upsert_task_tag(&self, link: &crate::sync::TaskTagLink) -> Result<()> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)
                    AND EXISTS(SELECT 1 FROM tags WHERE id = ?2)",
            params![link.task_id.to_string(), link.tag_id.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(exists, "task-tag link references a missing task or tag");
        self.conn.execute(
            "INSERT OR IGNORE INTO task_tags (task_id, tag_id) VALUES (?1, ?2)",
            params![link.task_id.to_string(), link.tag_id.to_string()],
        )?;
        Ok(())
    }

    pub fn delete_task_tag(&self, task_id: Uuid, tag_id: Uuid) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM task_tags WHERE task_id = ?1 AND tag_id = ?2",
            params![task_id.to_string(), tag_id.to_string()],
        )? == 1)
    }
}

impl Database {
    // ==================== Project/workflow API ====================

    /// Associate an existing tag with a project/list. This is the only way a
    /// project-scoped tag is created, so agents can establish their own
    /// vocabulary without changing tags used by another project.
    pub fn scope_tag_to_list(&self, list_id: Uuid, tag_id: Uuid) -> Result<()> {
        let list_exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM lists WHERE id = ?1)",
            params![list_id.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(list_exists, "project/list not found: {list_id}");
        let tag_exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tags WHERE id = ?1)",
            params![tag_id.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(tag_exists, "tag not found: {tag_id}");
        self.conn.execute(
            "INSERT OR IGNORE INTO project_tags (list_id, tag_id) VALUES (?1, ?2)",
            params![list_id.to_string(), tag_id.to_string()],
        )?;
        Ok(())
    }

    pub fn unscoped_tag_from_list(&self, list_id: Uuid, tag_id: Uuid) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM project_tags WHERE list_id = ?1 AND tag_id = ?2",
            params![list_id.to_string(), tag_id.to_string()],
        )? == 1)
    }

    pub fn get_scoped_tag_ids(&self, list_id: Uuid) -> Result<Vec<Uuid>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag_id FROM project_tags WHERE list_id = ?1 ORDER BY tag_id")?;
        let ids = stmt
            .query_map(params![list_id.to_string()], |row| {
                Ok(Uuid::parse_str(&row.get::<_, String>(0)?).unwrap())
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    pub fn tag_has_project_scope(&self, tag_id: Uuid) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM project_tags WHERE tag_id = ?1)",
                params![tag_id.to_string()],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn get_task_workflow(&self, task_id: Uuid) -> Result<Option<TaskWorkflow>> {
        self.conn
            .query_row(
                "SELECT task_id, status, created_by, owner, review_required, blocked_reason, updated_at
                 FROM task_workflow WHERE task_id = ?1",
                params![task_id.to_string()],
                task_workflow_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Load workflow records with one query for the TUI refresh cache.
    pub fn get_task_workflows(&self) -> Result<HashMap<Uuid, TaskWorkflow>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_id, status, created_by, owner, review_required, blocked_reason, updated_at
             FROM task_workflow",
        )?;
        let rows = stmt
            .query_map([], task_workflow_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|workflow| (workflow.task_id, workflow))
            .collect())
    }

    pub fn set_task_workflow(
        &self,
        task_id: Uuid,
        status: TaskStatus,
        actor: &str,
        owner: Option<&str>,
        review_required: Option<bool>,
        blocked_reason: Option<&str>,
    ) -> Result<TaskWorkflow> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
            params![task_id.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(exists, "task not found: {task_id}");

        if matches!(status, TaskStatus::Done | TaskStatus::Verified) {
            let blockers: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM task_dependencies d
                 LEFT JOIN task_workflow w ON w.task_id = d.depends_on
                 WHERE d.task_id = ?1 AND COALESCE(w.status, 'ready') NOT IN ('done', 'verified')",
                params![task_id.to_string()],
                |row| row.get(0),
            )?;
            anyhow::ensure!(blockers == 0, "task has incomplete prerequisites");
        }

        let now = chrono::Utc::now();
        let current = self.get_task_workflow(task_id)?;
        let created_by = current
            .as_ref()
            .and_then(|workflow| workflow.created_by.as_deref())
            .unwrap_or(actor);
        let owner_value = owner.or(current.as_ref().and_then(|w| w.owner.as_deref()));
        let review_value = review_required
            .unwrap_or_else(|| current.as_ref().map(|w| w.review_required).unwrap_or(false));
        self.conn.execute(
            "INSERT INTO task_workflow (task_id, status, created_by, owner, review_required, blocked_reason, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(task_id) DO UPDATE SET status=excluded.status, owner=excluded.owner,
             review_required=excluded.review_required, blocked_reason=excluded.blocked_reason, updated_at=excluded.updated_at",
            params![
                task_id.to_string(),
                status.as_str(),
                created_by,
                owner_value,
                review_value as i32,
                blocked_reason,
                now.to_rfc3339()
            ],
        )?;

        if matches!(
            status,
            TaskStatus::Done | TaskStatus::Verified | TaskStatus::Cancelled
        ) {
            self.conn.execute(
                "UPDATE tasks SET completed = 1, completed_at = ?2, updated_at = ?2 WHERE id = ?1",
                params![task_id.to_string(), now.to_rfc3339()],
            )?;
        } else if matches!(
            status,
            TaskStatus::Cancelled
                | TaskStatus::Backlog
                | TaskStatus::Ready
                | TaskStatus::Claimed
                | TaskStatus::InProgress
                | TaskStatus::Blocked
                | TaskStatus::InReview
        ) {
            self.conn.execute(
                "UPDATE tasks SET completed = 0, completed_at = NULL, updated_at = ?2 WHERE id = ?1",
                params![task_id.to_string(), now.to_rfc3339()],
            )?;
        }

        self.add_task_event(
            task_id,
            actor,
            "status_changed",
            Some(status.as_str()),
            None,
        )?;
        self.get_task_workflow(task_id)?
            .context("workflow record missing")
    }

    pub fn add_dependency(&self, task_id: Uuid, depends_on: Uuid) -> Result<()> {
        anyhow::ensure!(task_id != depends_on, "a task cannot depend on itself");
        let found: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1) AND EXISTS(SELECT 1 FROM tasks WHERE id = ?2)",
            params![task_id.to_string(), depends_on.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(found, "both tasks must exist");
        let creates_cycle: bool = self.conn.query_row(
            "WITH RECURSIVE walk(id) AS (
                SELECT ?2 UNION SELECT d.depends_on FROM task_dependencies d JOIN walk w ON d.task_id = w.id
             ) SELECT EXISTS(SELECT 1 FROM walk WHERE id = ?1)",
            params![task_id.to_string(), depends_on.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(!creates_cycle, "dependency would create a cycle");
        self.conn.execute(
            "INSERT OR IGNORE INTO task_dependencies (task_id, depends_on, created_at) VALUES (?1, ?2, ?3)",
            params![task_id.to_string(), depends_on.to_string(), chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn remove_dependency(&self, task_id: Uuid, depends_on: Uuid) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM task_dependencies WHERE task_id = ?1 AND depends_on = ?2",
            params![task_id.to_string(), depends_on.to_string()],
        )? == 1)
    }

    pub fn list_dependencies(&self, task_id: Uuid) -> Result<Vec<TaskDependency>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_id, depends_on, created_at FROM task_dependencies WHERE task_id = ?1 ORDER BY created_at",
        )?;
        let values = stmt
            .query_map(params![task_id.to_string()], |row| {
                Ok(TaskDependency {
                    task_id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    depends_on: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn add_task_event(
        &self,
        task_id: Uuid,
        actor: &str,
        event_type: &str,
        message: Option<&str>,
        metadata: Option<&serde_json::Value>,
    ) -> Result<TaskEvent> {
        let event = TaskEvent {
            id: Uuid::new_v4(),
            task_id,
            actor: actor.to_string(),
            event_type: event_type.to_string(),
            message: message.map(str::to_string),
            metadata: metadata.cloned(),
            created_at: chrono::Utc::now(),
        };
        self.conn.execute(
            "INSERT INTO task_events (id, task_id, actor, event_type, message, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.id.to_string(), event.task_id.to_string(), event.actor,
                event.event_type, event.message,
                event.metadata.as_ref().map(serde_json::Value::to_string),
                event.created_at.to_rfc3339()
            ],
        )?;
        Ok(event)
    }

    pub fn list_task_events(&self, task_id: Uuid) -> Result<Vec<TaskEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, actor, event_type, message, metadata, created_at FROM task_events WHERE task_id = ?1 ORDER BY created_at, id",
        )?;
        let values = stmt
            .query_map(params![task_id.to_string()], |row| {
                let metadata = row
                    .get::<_, Option<String>>(5)?
                    .and_then(|value| serde_json::from_str(&value).ok());
                Ok(TaskEvent {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    task_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
                    actor: row.get(2)?,
                    event_type: row.get(3)?,
                    message: row.get(4)?,
                    metadata,
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn enqueue_agent_job(
        &self,
        task_id: Uuid,
        agent: &str,
        instructions: Option<&str>,
        actor: &str,
    ) -> Result<AgentJob> {
        let already_queued: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_jobs WHERE task_id = ?1 AND status IN ('queued', 'claimed', 'running'))",
            params![task_id.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(!already_queued, "task already has an active agent job");
        let now = chrono::Utc::now();
        let job = AgentJob {
            id: Uuid::new_v4(),
            task_id,
            agent: agent.to_string(),
            status: "queued".to_string(),
            instructions: instructions.map(str::to_string),
            claimed_at: None,
            created_at: now,
            updated_at: now,
        };
        self.conn.execute(
            "INSERT INTO agent_jobs (id, task_id, agent, status, instructions, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![job.id.to_string(), task_id.to_string(), job.agent, job.status, job.instructions, now.to_rfc3339(), now.to_rfc3339()],
        )?;
        self.set_task_workflow(task_id, TaskStatus::Claimed, actor, None, None, None)?;
        self.add_task_event(task_id, actor, "agent_queued", Some(agent), None)?;
        Ok(job)
    }

    pub fn claim_agent_job(&self, job_id: Uuid, actor: &str) -> Result<AgentJob> {
        let now = chrono::Utc::now();
        let changed = self.conn.execute(
            "UPDATE agent_jobs SET status = 'claimed', claimed_at = ?2, updated_at = ?2
             WHERE id = ?1 AND status = 'queued'",
            params![job_id.to_string(), now.to_rfc3339()],
        )?;
        anyhow::ensure!(changed == 1, "queued agent job not found: {job_id}");
        let job = self
            .list_agent_jobs(None)?
            .into_iter()
            .find(|job| job.id == job_id)
            .context("claimed agent job disappeared")?;
        self.set_task_workflow(
            job.task_id,
            TaskStatus::Claimed,
            actor,
            Some(&job.agent),
            None,
            None,
        )?;
        self.add_task_event(job.task_id, actor, "agent_claimed", Some(&job.agent), None)?;
        Ok(job)
    }

    /// Claim the oldest queued job, suitable for a single local orchestrator
    /// polling loop. The conditional update in `claim_agent_job` prevents a
    /// second worker from claiming the same row.
    pub fn claim_next_agent_job(&self, actor: &str) -> Result<Option<AgentJob>> {
        let id = self
            .conn
            .query_row(
                "SELECT id FROM agent_jobs WHERE status = 'queued' ORDER BY created_at, id LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match id {
            Some(id) => Ok(Some(self.claim_agent_job(
                Uuid::parse_str(&id).context("invalid queued job id")?,
                actor,
            )?)),
            None => Ok(None),
        }
    }

    pub fn start_agent_run(
        &self,
        task_id: Uuid,
        agent: &str,
        conversation_id: Option<&str>,
        actor: &str,
    ) -> Result<AgentRun> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
            params![task_id.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(exists, "task not found: {task_id}");
        let now = chrono::Utc::now();
        let run = AgentRun {
            id: Uuid::new_v4(),
            task_id,
            agent: agent.to_string(),
            conversation_id: conversation_id.map(str::to_string),
            status: "running".to_string(),
            workspace: None,
            branch: None,
            commit_sha: None,
            pull_request_url: None,
            error: None,
            created_at: now,
            updated_at: now,
        };
        self.conn.execute(
            "INSERT INTO agent_runs (id, task_id, agent, conversation_id, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![run.id.to_string(), task_id.to_string(), run.agent, run.conversation_id, run.status, now.to_rfc3339(), now.to_rfc3339()],
        )?;
        self.conn.execute(
            "UPDATE agent_jobs
                SET status = 'running', claimed_at = COALESCE(claimed_at, ?2), updated_at = ?2
              WHERE task_id = ?1 AND status IN ('claimed', 'queued')",
            params![task_id.to_string(), now.to_rfc3339()],
        )?;
        self.set_task_workflow(
            task_id,
            TaskStatus::InProgress,
            actor,
            Some(agent),
            None,
            None,
        )?;
        self.add_task_event(task_id, actor, "agent_started", Some(agent), None)?;
        Ok(run)
    }

    pub fn update_agent_run(&self, run_id: Uuid, update: &AgentRunUpdate) -> Result<AgentRun> {
        let current = self.get_agent_run(run_id)?.context("agent run not found")?;
        let now = chrono::Utc::now();
        self.conn.execute(
            "UPDATE agent_runs SET status = ?2, workspace = COALESCE(?3, workspace), branch = COALESCE(?4, branch),
             commit_sha = COALESCE(?5, commit_sha), pull_request_url = COALESCE(?6, pull_request_url),
             error = COALESCE(?7, error), updated_at = ?8 WHERE id = ?1",
            params![
                run_id.to_string(),
                update.status,
                update.workspace,
                update.branch,
                update.commit_sha,
                update.pull_request_url,
                update.error,
                now.to_rfc3339()
            ],
        )?;
        if update.status == "succeeded" {
            let workflow = self.get_task_workflow(current.task_id)?;
            let next = if workflow.as_ref().is_some_and(|w| w.review_required) {
                TaskStatus::InReview
            } else {
                TaskStatus::Verified
            };
            self.set_task_workflow(current.task_id, next, &update.actor, None, None, None)?;
        } else if matches!(update.status.as_str(), "failed" | "cancelled") {
            self.set_task_workflow(
                current.task_id,
                if update.status == "failed" {
                    TaskStatus::Blocked
                } else {
                    TaskStatus::Cancelled
                },
                &update.actor,
                None,
                None,
                update.error.as_deref(),
            )?;
        }
        if update.status == "running" {
            self.conn.execute(
                "UPDATE agent_jobs SET status = 'running', updated_at = ?2 WHERE task_id = ?1 AND status IN ('claimed', 'queued')",
                params![current.task_id.to_string(), now.to_rfc3339()],
            )?;
        } else if matches!(update.status.as_str(), "succeeded" | "failed" | "cancelled") {
            self.conn.execute(
                "UPDATE agent_jobs SET status = ?2, updated_at = ?3 WHERE task_id = ?1 AND status IN ('claimed', 'running')",
                params![current.task_id.to_string(), update.status, now.to_rfc3339()],
            )?;
        }
        self.add_task_event(
            current.task_id,
            &update.actor,
            "agent_updated",
            Some(&update.status),
            None,
        )?;
        self.get_agent_run(run_id)?.context("agent run disappeared")
    }

    pub fn get_agent_run(&self, run_id: Uuid) -> Result<Option<AgentRun>> {
        self.conn
            .query_row(
                "SELECT id, task_id, agent, conversation_id, status, workspace, branch, commit_sha, pull_request_url, error, created_at, updated_at FROM agent_runs WHERE id = ?1",
                params![run_id.to_string()],
                agent_run_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_agent_runs(&self, task_id: Option<Uuid>) -> Result<Vec<AgentRun>> {
        let (sql, task_param) = if let Some(task_id) = task_id {
            (
                "SELECT id, task_id, agent, conversation_id, status, workspace, branch, commit_sha, pull_request_url, error, created_at, updated_at FROM agent_runs WHERE task_id = ?1 ORDER BY created_at DESC",
                Some(task_id.to_string()),
            )
        } else {
            (
                "SELECT id, task_id, agent, conversation_id, status, workspace, branch, commit_sha, pull_request_url, error, created_at, updated_at FROM agent_runs ORDER BY created_at DESC",
                None,
            )
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match task_param {
            Some(value) => stmt
                .query_map(params![value], agent_run_from_row)?
                .collect::<Result<Vec<_>, _>>()?,
            None => stmt
                .query_map([], agent_run_from_row)?
                .collect::<Result<Vec<_>, _>>()?,
        };
        Ok(rows)
    }

    pub fn list_agent_jobs(&self, task_id: Option<Uuid>) -> Result<Vec<AgentJob>> {
        let (sql, task_param) = if let Some(task_id) = task_id {
            (
                "SELECT id, task_id, agent, status, instructions, claimed_at, created_at, updated_at FROM agent_jobs WHERE task_id = ?1 ORDER BY created_at DESC",
                Some(task_id.to_string()),
            )
        } else {
            (
                "SELECT id, task_id, agent, status, instructions, claimed_at, created_at, updated_at FROM agent_jobs ORDER BY created_at DESC",
                None,
            )
        };
        let mut stmt = self.conn.prepare(sql)?;
        let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<AgentJob> {
            Ok(AgentJob {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                task_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
                agent: row.get(2)?,
                status: row.get(3)?,
                instructions: row.get(4)?,
                claimed_at: row
                    .get::<_, Option<String>>(5)?
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc)),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            })
        };
        let rows = match task_param {
            Some(value) => stmt
                .query_map(params![value], mapper)?
                .collect::<Result<Vec<_>, _>>()?,
            None => stmt.query_map([], mapper)?.collect::<Result<Vec<_>, _>>()?,
        };
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use tempfile::tempdir;

    #[test]
    fn test_database_init() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        let db = Database::open_path(&path).unwrap();

        // Should have inbox list
        let lists = db.get_lists().unwrap();
        assert_eq!(lists.len(), 1);
        assert!(lists[0].is_inbox);
    }

    #[test]
    fn test_task_crud() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        let db = Database::open_path(&path).unwrap();

        let inbox = db.get_inbox().unwrap();

        // Create task
        let task = Task::new("Test task", inbox.id);
        db.insert_task(&task).unwrap();

        // Read tasks
        let tasks = db.get_tasks_for_list(inbox.id).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Test task");

        // Update task
        let mut updated = tasks[0].clone();
        updated.title = "Updated task".to_string();
        db.update_task(&updated).unwrap();

        let tasks = db.get_tasks_for_list(inbox.id).unwrap();
        assert_eq!(tasks[0].title, "Updated task");

        // Delete task
        db.delete_task(tasks[0].id).unwrap();
        let tasks = db.get_tasks_for_list(inbox.id).unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn reminder_delivery_can_only_be_claimed_once() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        let db = Database::open_path(&path).unwrap();
        let task_id = Uuid::new_v4();

        assert!(
            db.claim_reminder_delivery(task_id, "due_today", "2026-07-18")
                .unwrap()
        );
        assert!(
            !db.claim_reminder_delivery(task_id, "due_today", "2026-07-18")
                .unwrap()
        );
    }

    fn database_with_task() -> (tempfile::TempDir, Database, Task) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        let db = Database::open_path(&path).unwrap();
        let task = Task::new("Reminder task", db.get_inbox().unwrap().id);
        db.insert_task(&task).unwrap();
        (dir, db, task)
    }

    #[test]
    fn multiple_reminders_for_task_are_ordered() {
        let (_dir, db, task) = database_with_task();
        let later = Reminder::new(
            task.id,
            Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap(),
        );
        let earlier = Reminder::new(task.id, Utc.with_ymd_and_hms(2026, 7, 20, 9, 0, 0).unwrap());
        db.create_reminder(&later).unwrap();
        db.create_reminder(&earlier).unwrap();

        let reminders = db.list_reminders_for_task(task.id).unwrap();
        assert_eq!(
            reminders.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![earlier.id, later.id]
        );
        assert_eq!(db.get_reminder(earlier.id).unwrap().unwrap(), earlier);
    }

    #[test]
    fn replacing_pending_reminders_retains_delivered_history() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("test.sqlite")).unwrap();
        let task = Task::new("Replacement", db.get_inbox().unwrap().id);
        db.insert_task(&task).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let delivered = Reminder::new(task.id, now - Duration::hours(1));
        let pending = Reminder::new(task.id, now);
        db.create_reminder(&delivered).unwrap();
        db.create_reminder(&pending).unwrap();
        db.claim_due_reminders(now, now - Duration::days(1), now - Duration::minutes(5))
            .unwrap();
        db.mark_reminder_delivered(delivered.id, now).unwrap();
        let replacement = Reminder::new(task.id, now + Duration::hours(1));
        db.replace_pending_reminders(task.id, std::slice::from_ref(&replacement))
            .unwrap();
        let reminders = db.list_reminders_for_task(task.id).unwrap();
        assert!(
            reminders
                .iter()
                .any(|r| r.id == delivered.id && r.delivered_at.is_some())
        );
        assert!(reminders.iter().any(|r| r.id == replacement.id));
        assert!(!reminders.iter().any(|r| r.id == pending.id));
    }

    #[test]
    fn due_reminder_is_claimed_only_once_until_lease_expires() {
        let (_dir, db, task) = database_with_task();
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let reminder = Reminder::new(task.id, now - Duration::minutes(1));
        db.create_reminder(&reminder).unwrap();

        assert_eq!(
            db.claim_due_reminders(now, now - Duration::days(1), now - Duration::minutes(5))
                .unwrap()
                .len(),
            1
        );
        assert!(
            db.claim_due_reminders(now, now - Duration::days(1), now - Duration::minutes(5))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            db.claim_due_reminders(
                now + Duration::minutes(6),
                now - Duration::days(1),
                now + Duration::minutes(1)
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn completed_tasks_and_too_old_reminders_are_not_claimed() {
        let (_dir, db, mut task) = database_with_task();
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let old = Reminder::new(task.id, now - Duration::days(2));
        let recent = Reminder::new(task.id, now - Duration::minutes(1));
        db.create_reminder(&old).unwrap();
        db.create_reminder(&recent).unwrap();

        let claimed = db
            .claim_due_reminders(now, now - Duration::days(1), now - Duration::minutes(5))
            .unwrap();
        assert_eq!(
            claimed.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![recent.id]
        );
        db.release_reminder_claim(recent.id).unwrap();
        task.complete();
        db.update_task(&task).unwrap();
        assert!(
            db.claim_due_reminders(now, now - Duration::days(1), now - Duration::minutes(5))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn deleting_task_removes_reminders_without_foreign_keys() {
        let (_dir, db, task) = database_with_task();
        let reminder = Reminder::new(task.id, Utc::now());
        db.create_reminder(&reminder).unwrap();
        db.delete_task(task.id).unwrap();
        assert!(db.list_reminders_for_task(task.id).unwrap().is_empty());
    }

    #[test]
    fn workflow_status_and_dependency_gate_completion() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("workflow.sqlite")).unwrap();
        let inbox = db.get_inbox().unwrap();
        let prerequisite = Task::new("Prerequisite", inbox.id);
        let dependent = Task::new("Dependent", inbox.id);
        db.insert_task(&prerequisite).unwrap();
        db.insert_task(&dependent).unwrap();
        db.add_dependency(dependent.id, prerequisite.id).unwrap();

        let error = db
            .set_task_workflow(dependent.id, TaskStatus::Done, "test", None, None, None)
            .unwrap_err();
        assert!(error.to_string().contains("incomplete prerequisites"));

        db.set_task_workflow(
            prerequisite.id,
            TaskStatus::Done,
            "test",
            Some("agent-a"),
            Some(true),
            None,
        )
        .unwrap();
        let workflow = db
            .set_task_workflow(dependent.id, TaskStatus::Done, "test", None, None, None)
            .unwrap();
        assert_eq!(workflow.status, TaskStatus::Done);
        assert!(
            db.get_all_tasks()
                .unwrap()
                .into_iter()
                .find(|task| task.id == dependent.id)
                .unwrap()
                .completed
        );

        let mut reopened = db
            .get_all_tasks()
            .unwrap()
            .into_iter()
            .find(|task| task.id == dependent.id)
            .unwrap();
        reopened.uncomplete();
        db.update_task(&reopened).unwrap();
        assert_eq!(
            db.get_task_workflow(dependent.id).unwrap().unwrap().status,
            TaskStatus::Ready
        );
    }

    #[test]
    fn project_tag_scope_and_agent_job_round_trip() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("project.sqlite")).unwrap();
        let list = List::new("Browser");
        db.insert_list(&list).unwrap();
        let tag = Tag::new("bug");
        db.insert_tag(&tag).unwrap();
        db.scope_tag_to_list(list.id, tag.id).unwrap();
        assert_eq!(db.get_scoped_tag_ids(list.id).unwrap(), vec![tag.id]);

        let task = Task::new("Fix issue", list.id).with_tag(tag.id);
        db.insert_task(&task).unwrap();
        let job = db
            .enqueue_agent_job(task.id, "codex", Some("Implement and test"), "test")
            .unwrap();
        assert_eq!(job.status, "queued");
        assert_eq!(db.list_agent_jobs(Some(task.id)).unwrap()[0].id, job.id);
        assert!(!db.list_task_events(task.id).unwrap().is_empty());
    }

    #[test]
    fn a_tag_scoped_to_multiple_projects_is_valid_in_each_project_only() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("tag-scope.sqlite")).unwrap();
        let first = List::new("First");
        let second = List::new("Second");
        let other = List::new("Other");
        db.insert_list(&first).unwrap();
        db.insert_list(&second).unwrap();
        db.insert_list(&other).unwrap();
        let tag = Tag::new("shared");
        db.insert_tag(&tag).unwrap();
        db.scope_tag_to_list(first.id, tag.id).unwrap();
        db.scope_tag_to_list(second.id, tag.id).unwrap();

        db.insert_task(&Task::new("Allowed", second.id).with_tag(tag.id))
            .unwrap();
        let error = db
            .insert_task(&Task::new("Rejected", other.id).with_tag(tag.id))
            .unwrap_err();
        assert!(error.to_string().contains("scoped to a different"));
    }

    #[test]
    fn cancelled_tasks_complete_even_when_dependencies_are_open_and_reminders_skip_them() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("cancelled.sqlite")).unwrap();
        let inbox = db.get_inbox().unwrap();
        let prerequisite = Task::new("Prerequisite", inbox.id);
        let cancelled = Task::new("Cancelled", inbox.id);
        db.insert_task(&prerequisite).unwrap();
        db.insert_task(&cancelled).unwrap();
        db.add_dependency(cancelled.id, prerequisite.id).unwrap();
        db.create_reminder(&Reminder::new(cancelled.id, Utc::now()))
            .unwrap();

        db.set_task_workflow(
            cancelled.id,
            TaskStatus::Cancelled,
            "test",
            None,
            None,
            Some("no longer needed"),
        )
        .unwrap();

        let stored = db
            .get_all_tasks()
            .unwrap()
            .into_iter()
            .find(|task| task.id == cancelled.id)
            .unwrap();
        assert!(stored.completed);
        assert_eq!(
            db.claim_due_reminders(
                Utc::now(),
                Utc::now() - Duration::days(1),
                Utc::now() - Duration::minutes(5)
            )
            .unwrap()
            .len(),
            0
        );
    }

    #[test]
    fn migration_recreates_the_dependency_trigger_for_existing_databases() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cancelled-migration.sqlite");
        {
            let db = Database::open_path(&path).unwrap();
            db.execute_raw("DROP TRIGGER prevent_task_completion_with_open_dependencies")
                .unwrap();
            db.execute_raw(
                "CREATE TRIGGER prevent_task_completion_with_open_dependencies
                 BEFORE UPDATE OF completed ON tasks
                 WHEN NEW.completed = 1 AND EXISTS (
                     SELECT 1 FROM task_dependencies d
                     JOIN task_workflow w ON w.task_id = d.depends_on
                     WHERE d.task_id = NEW.id
                       AND w.status NOT IN ('done', 'verified')
                 )
                 BEGIN
                     SELECT RAISE(ABORT, 'task has incomplete prerequisites');
                 END",
            )
            .unwrap();
        }

        let db = Database::open_path(&path).unwrap();
        let inbox = db.get_inbox().unwrap();
        let prerequisite = Task::new("Prerequisite", inbox.id);
        let cancelled = Task::new("Cancelled", inbox.id);
        db.insert_task(&prerequisite).unwrap();
        db.insert_task(&cancelled).unwrap();
        db.add_dependency(cancelled.id, prerequisite.id).unwrap();
        db.set_task_workflow(
            cancelled.id,
            TaskStatus::Cancelled,
            "migration-test",
            None,
            None,
            None,
        )
        .unwrap();
        assert!(
            db.get_all_tasks()
                .unwrap()
                .into_iter()
                .find(|task| task.id == cancelled.id)
                .unwrap()
                .completed
        );
    }

    #[test]
    fn verified_status_survives_task_edits_and_reopening_resets_to_ready() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("verified.sqlite")).unwrap();
        let inbox = db.get_inbox().unwrap();
        let task = Task::new("Verified", inbox.id);
        db.insert_task(&task).unwrap();
        db.set_task_workflow(task.id, TaskStatus::Verified, "test", None, None, None)
            .unwrap();

        let mut edited = db
            .get_all_tasks()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == task.id)
            .unwrap();
        edited.title = "Verified edit".to_string();
        db.update_task(&edited).unwrap();
        assert_eq!(
            db.get_task_workflow(task.id).unwrap().unwrap().status,
            TaskStatus::Verified
        );

        edited.uncomplete();
        db.update_task(&edited).unwrap();
        assert_eq!(
            db.get_task_workflow(task.id).unwrap().unwrap().status,
            TaskStatus::Ready
        );
    }

    #[test]
    fn starting_a_run_moves_the_task_job_to_running() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("agent.sqlite")).unwrap();
        let task = Task::new("Agent task", db.get_inbox().unwrap().id);
        db.insert_task(&task).unwrap();
        let job = db
            .enqueue_agent_job(task.id, "worker-a", None, "test")
            .unwrap();
        let run = db
            .start_agent_run(task.id, "worker-a", Some("conversation"), "test")
            .unwrap();
        assert_eq!(run.status, "running");
        assert_eq!(
            db.list_agent_jobs(Some(task.id)).unwrap()[0].status,
            "running"
        );
        assert_eq!(db.list_agent_jobs(Some(task.id)).unwrap()[0].id, job.id);
    }

    #[test]
    fn sync_apply_report_retains_rejections_and_restores_foreign_keys() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("sync.sqlite")).unwrap();
        db.execute_raw("PRAGMA foreign_keys = ON").unwrap();
        let before = db.foreign_keys_enabled().unwrap();
        let valid_list = List::new("Remote");
        let invalid_task = Task::new("Missing project", Uuid::new_v4());

        let report = db
            .apply_sync_records(&[
                SyncRecord::Task(invalid_task.clone()),
                SyncRecord::List(valid_list.clone()),
            ])
            .unwrap();

        assert_eq!(report.applied, 1);
        assert_eq!(report.rejected.len(), 1);
        assert!(report.rejected[0].error.contains("project/list not found"));
        assert!(
            db.get_lists()
                .unwrap()
                .iter()
                .any(|list| list.id == valid_list.id)
        );
        assert!(db.get_task_by_id(&invalid_task.id.to_string()).is_err());
        assert_eq!(db.foreign_keys_enabled().unwrap(), before);
    }
}

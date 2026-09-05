//! Tickit CLI Application
//!
//! Terminal-based task manager with beautiful TUI and CLI modes.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;

use tickit::sync::SyncRecord;
use tickit::{Config, Database, ExportFormat, List, Priority, Tag, Task, TaskStatus};

#[derive(Parser, Debug)]
#[command(name = "tickit")]
#[command(author, version, about = "A stunning terminal-based task manager")]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the TUI (default)
    Ui,

    /// Add a new task
    Add {
        /// Task title
        title: String,

        /// Task description
        #[arg(short, long)]
        description: Option<String>,

        /// URL to attach
        #[arg(short, long)]
        url: Option<String>,

        /// Priority (low, medium, high, urgent)
        #[arg(short, long, default_value = "medium")]
        priority: String,

        /// List name to add task to
        #[arg(short, long)]
        list: Option<String>,

        /// Tags to attach (comma-separated)
        #[arg(short, long)]
        tags: Option<String>,

        /// Due date (YYYY-MM-DD format)
        #[arg(long)]
        due: Option<String>,

        /// Reminder time (RFC3339/local timestamp) or offset such as 2h-before; repeatable
        #[arg(long = "remind")]
        reminders: Vec<String>,

        /// Initial workflow owner (human or agent identity)
        #[arg(long)]
        owner: Option<String>,

        /// Actor recorded as the task author
        #[arg(long, default_value = "cli")]
        actor: String,
    },

    /// List tasks
    #[command(alias = "ls")]
    List {
        /// Filter by list name
        #[arg(short, long)]
        list: Option<String>,

        /// Show completed tasks
        #[arg(short, long)]
        all: bool,

        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Mark task as complete
    Done {
        /// Task ID or title (partial match)
        task: String,
    },

    /// Mark task as not complete
    Undo {
        /// Task ID or title (partial match)
        task: String,
    },

    /// Delete a task
    #[command(alias = "rm")]
    Delete {
        /// Task ID or title (partial match)
        task: String,

        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Manage lists
    Lists {
        #[command(subcommand)]
        command: Option<ListCommands>,
    },

    /// Manage tags
    Tags {
        #[command(subcommand)]
        command: Option<TagCommands>,
    },

    /// Export tasks
    Export {
        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Format (json, todotxt, markdown, csv)
        #[arg(short, long, default_value = "json")]
        format: String,

        /// Filter by list
        #[arg(short, long)]
        list: Option<String>,
    },

    /// Check for updates and install if available
    Update,

    /// Manually trigger a sync with the server
    Sync {
        /// Show sync status instead of syncing
        #[arg(long)]
        status: bool,

        /// Force full sync (ignore last_sync timestamp)
        #[arg(long)]
        force: bool,
    },

    /// Check and deliver pending task reminders
    Reminders {
        #[command(subcommand)]
        command: ReminderCommands,
    },

    /// Query project tasks in an LLM-friendly JSON format
    Query {
        /// Project/list name or UUID
        #[arg(long)]
        project: Option<String>,
        /// Project-scoped tag name
        #[arg(long)]
        tag: Option<String>,
        /// Workflow status
        #[arg(long)]
        status: Option<String>,
        /// Assigned owner/agent
        #[arg(long)]
        owner: Option<String>,
        /// Include completed legacy tasks
        #[arg(long)]
        all: bool,
    },

    /// Manage workflow statuses and dependencies
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommands,
    },

    /// Queue and inspect agent work
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },
}

#[derive(Subcommand, Debug)]
enum ReminderCommands {
    /// Check for due reminders and send desktop notifications
    Check,
    /// List reminders, optionally for one task
    List { task: Option<String> },
    /// Add a reminder to a task
    Add {
        task: String,
        #[arg(long, conflicts_with = "before", required_unless_present = "before")]
        at: Option<String>,
        #[arg(long, conflicts_with = "at", required_unless_present = "at")]
        before: Option<String>,
    },
    /// Delete a reminder by UUID
    Delete { reminder: uuid::Uuid },
    /// Snooze a reminder by a concise duration, e.g. 10m or 1h
    Snooze {
        reminder: uuid::Uuid,
        duration: String,
    },
    /// Install and enable the per-user systemd timer
    InstallSystemd,
    /// Disable and remove the per-user systemd timer
    UninstallSystemd,
}

#[derive(Subcommand, Debug)]
enum ListCommands {
    /// List all lists
    #[command(alias = "ls")]
    List,

    /// Add a new list
    Add {
        /// List name
        name: String,

        /// Icon/emoji
        #[arg(short, long, default_value = "📋")]
        icon: String,
    },

    /// Delete a list
    #[command(alias = "rm")]
    Delete {
        /// List name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum TagCommands {
    /// List all tags
    #[command(alias = "ls")]
    List,

    /// Add a new tag
    Add {
        /// Tag name
        name: String,

        /// Color (hex)
        #[arg(short, long)]
        color: Option<String>,

        /// Scope this tag to a project/list
        #[arg(short, long)]
        list: Option<String>,
    },

    /// Delete a tag
    #[command(alias = "rm")]
    Delete {
        /// Tag name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum WorkflowCommands {
    /// Set a task workflow status
    Set {
        task: String,
        status: String,
        #[arg(long, default_value = "cli")]
        actor: String,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        review_required: Option<bool>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Add a prerequisite dependency
    DependsOn { task: String, prerequisite: String },
    /// Remove a prerequisite dependency
    Undepends { task: String, prerequisite: String },
    /// List prerequisites
    Dependencies { task: String },
    /// Show task activity history
    Events { task: String },
}

#[derive(Subcommand, Debug)]
enum AgentCommands {
    /// Queue a task for an external agent/orchestrator
    Enqueue {
        task: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        instructions: Option<String>,
        #[arg(long, default_value = "cli")]
        actor: String,
    },
    /// Claim a queued job for a worker
    Claim {
        job: uuid::Uuid,
        #[arg(long, default_value = "worker")]
        actor: String,
    },
    /// Atomically claim the oldest queued job
    Next {
        #[arg(long, default_value = "orchestrator")]
        actor: String,
    },
    /// Start an agent run for a task
    Start {
        task: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        conversation_id: Option<String>,
        #[arg(long, default_value = "worker")]
        actor: String,
    },
    /// Update a run with execution/review evidence
    Update {
        run: uuid::Uuid,
        status: String,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        commit_sha: Option<String>,
        #[arg(long)]
        pull_request_url: Option<String>,
        #[arg(long)]
        error: Option<String>,
        #[arg(long, default_value = "worker")]
        actor: String,
    },
    /// List queued and historical agent jobs
    Jobs { task: Option<String> },
    /// List agent runs
    Runs { task: Option<String> },
}

#[derive(Serialize)]
struct QueryTask {
    task: Task,
    project: List,
    tags: Vec<Tag>,
    workflow: Option<tickit::TaskWorkflow>,
    dependencies: Vec<tickit::TaskDependency>,
    events: Vec<tickit::TaskEvent>,
    agent_jobs: Vec<tickit::AgentJob>,
    agent_runs: Vec<tickit::AgentRun>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    if cli.verbose {
        tracing_subscriber::fmt()
            .with_env_filter("tickit=debug")
            .init();
    }

    match cli.command {
        None | Some(Commands::Ui) => {
            // Start TUI
            tickit::app::run()?;
        }

        Some(Commands::Add {
            title,
            description,
            url,
            priority,
            list,
            tags,
            due,
            reminders,
            owner,
            actor,
        }) => {
            let db = Database::open()?;

            // Find list
            let list_id = if let Some(list_name) = list {
                let lists = db.get_lists()?;
                lists
                    .iter()
                    .find(|l| l.name.to_lowercase() == list_name.to_lowercase())
                    .map(|l| l.id)
                    .unwrap_or_else(|| db.get_inbox().unwrap().id)
            } else {
                db.get_inbox()?.id
            };

            // Parse priority
            let priority = match priority.to_lowercase().as_str() {
                "low" | "l" => Priority::Low,
                "high" | "h" => Priority::High,
                "urgent" | "u" => Priority::Urgent,
                _ => Priority::Medium,
            };

            // Parse due date
            let due_date = due
                .as_deref()
                .map(tickit::notifications::parse_datetime)
                .transpose()?;

            // Create task
            let mut task = Task::new(&title, list_id);
            task.priority = priority;
            task.description = description;
            task.url = url;
            task.due_date = due_date;

            // Add tags
            if let Some(tag_str) = tags {
                let db_tags = db.get_tags()?;
                for tag_name in tag_str.split(',').map(|s| s.trim()) {
                    if let Some(tag) = db_tags
                        .iter()
                        .find(|t| t.name.to_lowercase() == tag_name.to_lowercase())
                    {
                        task.tag_ids.push(tag.id);
                    }
                }
            }

            let reminders = reminders
                .iter()
                .map(|value| {
                    tickit::notifications::parse_reminder_spec(value, due_date)
                        .map(|at| tickit::models::Reminder::new(task.id, at))
                })
                .collect::<Result<Vec<_>>>()?;
            db.insert_task_with_reminders(&task, &reminders)?;
            db.set_task_workflow(
                task.id,
                if task.completed {
                    TaskStatus::Done
                } else {
                    TaskStatus::Ready
                },
                &actor,
                owner.as_deref(),
                None,
                None,
            )?;
            println!("✓ Added: {}", title);
        }

        Some(Commands::List {
            list,
            all,
            tag,
            json,
        }) => {
            let db = Database::open()?;
            let lists = db.get_lists()?;
            let tags = db.get_tags()?;

            // Find list filter
            let list_id = list.and_then(|name| {
                lists
                    .iter()
                    .find(|l| l.name.to_lowercase() == name.to_lowercase())
                    .map(|l| l.id)
            });

            // Find tag filter
            let tag_id = tag.and_then(|name| {
                tags.iter()
                    .find(|t| t.name.to_lowercase() == name.to_lowercase())
                    .map(|t| t.id)
            });

            let completed = if all { None } else { Some(false) };
            let tasks = db.get_tasks_with_filter(list_id, completed, tag_id)?;

            if json {
                let output = serde_json::to_string_pretty(&tasks)?;
                println!("{}", output);
            } else if tasks.is_empty() {
                println!("No tasks found.");
            } else {
                for task in tasks {
                    let checkbox = if task.completed { "☑" } else { "☐" };
                    let priority = task.priority.icon();
                    let list_name = lists
                        .iter()
                        .find(|l| l.id == task.list_id)
                        .map(|l| l.name.as_str())
                        .unwrap_or("?");

                    println!("{} {} {} [{}]", checkbox, priority, task.title, list_name);
                }
            }
        }

        Some(Commands::Done { task }) => {
            let db = Database::open()?;
            let tasks = db.get_all_tasks()?;

            if let Some(mut t) = find_task(&tasks, &task) {
                t.complete();
                db.update_task(&t)?;
                println!("✓ Completed: {}", t.title);
            } else {
                println!("Task not found: {}", task);
            }
        }

        Some(Commands::Undo { task }) => {
            let db = Database::open()?;
            let tasks = db.get_all_tasks()?;

            if let Some(mut t) = find_task(&tasks, &task) {
                t.uncomplete();
                db.update_task(&t)?;
                println!("↺ Reopened: {}", t.title);
            } else {
                println!("Task not found: {}", task);
            }
        }

        Some(Commands::Delete { task, force }) => {
            let db = Database::open()?;
            let tasks = db.get_all_tasks()?;

            if let Some(t) = find_task(&tasks, &task) {
                if !force {
                    print!("Delete \"{}\"? [y/N] ", t.title);
                    use std::io::{self, Write};
                    io::stdout().flush()?;
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    if !input.trim().eq_ignore_ascii_case("y") {
                        println!("Cancelled.");
                        return Ok(());
                    }
                }
                db.delete_task(t.id)?;
                println!("✗ Deleted: {}", t.title);
            } else {
                println!("Task not found: {}", task);
            }
        }

        Some(Commands::Lists { command }) => {
            let db = Database::open()?;

            match command {
                None | Some(ListCommands::List) => {
                    let lists = db.get_lists()?;
                    for list in lists {
                        let inbox = if list.is_inbox { " (default)" } else { "" };
                        let count = db.get_task_count(list.id, false)?;
                        println!("{} {} ({} tasks){}", list.icon, list.name, count, inbox);
                    }
                }
                Some(ListCommands::Add { name, icon }) => {
                    let list = List::new(&name).with_icon(&icon);
                    db.insert_list(&list)?;
                    println!("✓ Created list: {} {}", icon, name);
                }
                Some(ListCommands::Delete { name }) => {
                    let lists = db.get_lists()?;
                    if let Some(list) = lists
                        .iter()
                        .find(|l| l.name.to_lowercase() == name.to_lowercase())
                    {
                        if list.is_inbox {
                            println!("Cannot delete inbox.");
                        } else {
                            db.delete_list(list.id)?;
                            println!("✗ Deleted list: {}", name);
                        }
                    } else {
                        println!("List not found: {}", name);
                    }
                }
            }
        }

        Some(Commands::Tags { command }) => {
            let db = Database::open()?;

            match command {
                None | Some(TagCommands::List) => {
                    let tags = db.get_tags()?;
                    if tags.is_empty() {
                        println!("No tags yet.");
                    } else {
                        for tag in tags {
                            println!("● {} ({})", tag.name, tag.color);
                        }
                    }
                }
                Some(TagCommands::Add { name, color, list }) => {
                    let scoped_list = if let Some(project) = list.as_deref() {
                        Some(unique_list(&db.get_lists()?, project)?)
                    } else {
                        None
                    };
                    let existing = db
                        .get_tags()?
                        .into_iter()
                        .find(|tag| tag.name.eq_ignore_ascii_case(&name));
                    if let Some(tag) = existing {
                        if let Some(list) = scoped_list {
                            db.scope_tag_to_list(list.id, tag.id)?;
                            println!("✓ Scoped existing tag '{}' to {}", tag.name, list.name);
                        } else {
                            anyhow::bail!("tag already exists: {}", tag.name);
                        }
                    } else {
                        let mut tag = Tag::new(&name);
                        if let Some(c) = color {
                            tag = tag.with_color(&c);
                        }
                        db.insert_tag(&tag)?;
                        if let Some(list) = scoped_list {
                            db.scope_tag_to_list(list.id, tag.id)?;
                        }
                        println!("✓ Created tag: {}", name);
                    }
                }
                Some(TagCommands::Delete { name }) => {
                    let tags = db.get_tags()?;
                    if let Some(tag) = tags
                        .iter()
                        .find(|t| t.name.to_lowercase() == name.to_lowercase())
                    {
                        db.delete_tag(tag.id)?;
                        println!("✗ Deleted tag: {}", name);
                    } else {
                        println!("Tag not found: {}", name);
                    }
                }
            }
        }

        Some(Commands::Export {
            output,
            format,
            list,
        }) => {
            let db = Database::open()?;
            let lists = db.get_lists()?;
            let tags = db.get_tags()?;

            // Filter by list
            let list_id = list.and_then(|name| {
                lists
                    .iter()
                    .find(|l| l.name.to_lowercase() == name.to_lowercase())
                    .map(|l| l.id)
            });

            let tasks = if let Some(lid) = list_id {
                db.get_tasks_for_list(lid)?
            } else {
                db.get_all_tasks()?
            };

            // Parse format
            let fmt = match format.to_lowercase().as_str() {
                "todotxt" | "todo.txt" | "txt" => ExportFormat::TodoTxt,
                "markdown" | "md" => ExportFormat::Markdown,
                "csv" => ExportFormat::Csv,
                _ => ExportFormat::Json,
            };

            // Export
            if let Some(path) = output {
                let mut file = std::fs::File::create(&path)?;
                tickit::export::export_tasks(&mut file, &tasks, &lists, &tags, fmt)?;
                println!("Exported {} tasks to {}", tasks.len(), path.display());
            } else {
                let mut stdout = std::io::stdout();
                tickit::export::export_tasks(&mut stdout, &tasks, &lists, &tags, fmt)?;
            }
        }

        Some(Commands::Update) => {
            run_update_command();
        }

        Some(Commands::Sync { status, force }) => {
            run_sync_command(status, force)?;
        }

        Some(Commands::Reminders { command }) => run_reminder_command(command)?,

        Some(Commands::Query {
            project,
            tag,
            status,
            owner,
            all,
        }) => run_query_command(project, tag, status, owner, all)?,

        Some(Commands::Workflow { command }) => run_workflow_command(command)?,

        Some(Commands::Agent { command }) => run_agent_command(command)?,
    }

    Ok(())
}

fn run_query_command(
    project: Option<String>,
    tag: Option<String>,
    status: Option<String>,
    owner: Option<String>,
    include_completed: bool,
) -> Result<()> {
    let db = Database::open()?;
    let lists = db.get_lists()?;
    let tags = db.get_tags()?;
    let project_list = project
        .as_deref()
        .map(|value| unique_list(&lists, value))
        .transpose()?;
    let requested_status = status
        .as_deref()
        .map(|value| TaskStatus::parse(value).context("invalid workflow status"))
        .transpose()?;
    let requested_owner = owner.map(|value| value.to_lowercase());
    let requested_tag = tag.map(|value| value.to_lowercase());
    let tasks = db.get_tasks_with_filter(
        project_list.as_ref().map(|list| list.id),
        if include_completed { None } else { Some(false) },
        None,
    )?;

    let mut output = Vec::new();
    for task in tasks {
        let list = lists
            .iter()
            .find(|list| list.id == task.list_id)
            .cloned()
            .context("task references missing project/list")?;
        let workflow = db.get_task_workflow(task.id)?;
        if requested_status
            .is_some_and(|wanted| workflow.as_ref().map(|w| w.status) != Some(wanted))
        {
            continue;
        }
        if requested_owner.as_deref().is_some_and(|wanted| {
            workflow
                .as_ref()
                .and_then(|w| w.owner.as_deref())
                .map(str::to_lowercase)
                .as_deref()
                != Some(wanted)
        }) {
            continue;
        }
        let task_tags: Vec<Tag> = task
            .tag_ids
            .iter()
            .filter_map(|id| tags.iter().find(|tag| tag.id == *id).cloned())
            .collect();
        if let Some(wanted) = requested_tag.as_deref() {
            let scoped = db.get_scoped_tag_ids(list.id)?;
            let mut tag_matches = false;
            for candidate in &task_tags {
                if candidate.name.to_lowercase() == wanted
                    && (!db.tag_has_project_scope(candidate.id)? || scoped.contains(&candidate.id))
                {
                    tag_matches = true;
                    break;
                }
            }
            if !tag_matches {
                continue;
            }
        }
        output.push(QueryTask {
            dependencies: db.list_dependencies(task.id)?,
            events: db.list_task_events(task.id)?,
            agent_jobs: db.list_agent_jobs(Some(task.id))?,
            agent_runs: db.list_agent_runs(Some(task.id))?,
            task,
            project: list,
            tags: task_tags,
            workflow,
        });
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "tasks": output,
        }))?
    );
    Ok(())
}

fn run_workflow_command(command: WorkflowCommands) -> Result<()> {
    let db = Database::open()?;
    match command {
        WorkflowCommands::Set {
            task,
            status,
            actor,
            owner,
            review_required,
            reason,
        } => {
            let task = unique_task(&db.get_all_tasks()?, &task)?;
            let status = TaskStatus::parse(&status).context("invalid workflow status")?;
            let workflow = db.set_task_workflow(
                task.id,
                status,
                &actor,
                owner.as_deref(),
                review_required,
                reason.as_deref(),
            )?;
            println!("{}", serde_json::to_string_pretty(&workflow)?);
        }
        WorkflowCommands::DependsOn { task, prerequisite } => {
            let tasks = db.get_all_tasks()?;
            let task = unique_task(&tasks, &task)?;
            let prerequisite = unique_task(&tasks, &prerequisite)?;
            db.add_dependency(task.id, prerequisite.id)?;
            println!("✓ {} now depends on {}", task.id, prerequisite.id);
        }
        WorkflowCommands::Undepends { task, prerequisite } => {
            let tasks = db.get_all_tasks()?;
            let task = unique_task(&tasks, &task)?;
            let prerequisite = unique_task(&tasks, &prerequisite)?;
            if db.remove_dependency(task.id, prerequisite.id)? {
                println!("✓ Dependency removed");
            } else {
                println!("Dependency not found");
            }
        }
        WorkflowCommands::Dependencies { task } => {
            let task = unique_task(&db.get_all_tasks()?, &task)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&db.list_dependencies(task.id)?)?
            );
        }
        WorkflowCommands::Events { task } => {
            let task = unique_task(&db.get_all_tasks()?, &task)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&db.list_task_events(task.id)?)?
            );
        }
    }
    Ok(())
}

fn run_agent_command(command: AgentCommands) -> Result<()> {
    let db = Database::open()?;
    match command {
        AgentCommands::Enqueue {
            task,
            agent,
            instructions,
            actor,
        } => {
            let task = unique_task(&db.get_all_tasks()?, &task)?;
            let job = db.enqueue_agent_job(task.id, &agent, instructions.as_deref(), &actor)?;
            println!("{}", serde_json::to_string_pretty(&job)?);
        }
        AgentCommands::Claim { job, actor } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&db.claim_agent_job(job, &actor)?)?
            );
        }
        AgentCommands::Next { actor } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&db.claim_next_agent_job(&actor)?)?
            );
        }
        AgentCommands::Start {
            task,
            agent,
            conversation_id,
            actor,
        } => {
            let task = unique_task(&db.get_all_tasks()?, &task)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&db.start_agent_run(
                    task.id,
                    &agent,
                    conversation_id.as_deref(),
                    &actor,
                )?)?
            );
        }
        AgentCommands::Update {
            run,
            status,
            workspace,
            branch,
            commit_sha,
            pull_request_url,
            error,
            actor,
        } => {
            let update = tickit::AgentRunUpdate {
                status,
                workspace,
                branch,
                commit_sha,
                pull_request_url,
                error,
                actor,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&db.update_agent_run(run, &update)?)?
            );
        }
        AgentCommands::Jobs { task } => {
            let task = task
                .map(|query| unique_task(&db.get_all_tasks()?, &query).map(|task| task.id))
                .transpose()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&db.list_agent_jobs(task)?)?
            );
        }
        AgentCommands::Runs { task } => {
            let task = task
                .map(|query| unique_task(&db.get_all_tasks()?, &query).map(|task| task.id))
                .transpose()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&db.list_agent_runs(task)?)?
            );
        }
    }
    Ok(())
}

fn unique_list(lists: &[List], query: &str) -> Result<List> {
    if let Ok(id) = uuid::Uuid::parse_str(query) {
        return lists
            .iter()
            .find(|list| list.id == id)
            .cloned()
            .context("project/list not found");
    }
    let needle = query.to_lowercase();
    let matches: Vec<_> = lists
        .iter()
        .filter(|list| list.name.to_lowercase() == needle)
        .cloned()
        .collect();
    match matches.as_slice() {
        [] => anyhow::bail!("project/list not found: {query}"),
        [list] => Ok(list.clone()),
        _ => anyhow::bail!("project/list match is ambiguous; use its UUID"),
    }
}

fn unique_task(tasks: &[Task], query: &str) -> Result<Task> {
    if let Ok(id) = uuid::Uuid::parse_str(query) {
        return tasks
            .iter()
            .find(|task| task.id == id)
            .cloned()
            .context("task not found");
    }
    let needle = query.to_lowercase();
    let matches: Vec<_> = tasks
        .iter()
        .filter(|task| task.title.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    match matches.as_slice() {
        [] => anyhow::bail!("task not found: {query}"),
        [task] => Ok(task.clone()),
        _ => anyhow::bail!("task match is ambiguous; use its UUID"),
    }
}

fn run_reminder_command(command: ReminderCommands) -> Result<()> {
    use chrono::Utc;
    use std::{fs, process::Command};
    let db = Database::open()?;
    match command {
        ReminderCommands::Check => {
            let config = Config::load()?;
            if !config.notifications {
                println!("Desktop notifications are disabled.");
                return Ok(());
            }
            let explicit = tickit::notifications::process_explicit_reminders(
                &db,
                Utc::now(),
                config.reminder_grace_minutes,
                config.reminder_claim_lease_minutes,
            )?;
            let legacy = tickit::notifications::check_due_tasks(&db);
            let mut failures = explicit.failures;
            let legacy_count = match legacy {
                Ok(count) => count,
                Err(error) => {
                    failures.push(format!("legacy due alert: {error:#}"));
                    0
                }
            };
            println!(
                "Explicit reminders: {} delivered, {} snoozed, {} completed; {} legacy alert(s).",
                explicit.delivered, explicit.snoozed, explicit.completed, legacy_count
            );
            if !failures.is_empty() {
                anyhow::bail!(
                    "{} delivery failure(s): {}",
                    failures.len(),
                    failures.join("; ")
                );
            }
        }
        ReminderCommands::List { task } => {
            let reminders = if let Some(query) = task {
                let task = unique_task(&db.get_all_tasks()?, &query)?;
                db.list_reminders_for_task(task.id)?
            } else {
                db.list_reminders()?
            };
            for reminder in reminders {
                println!(
                    "{}  {}  {}{}",
                    reminder.id,
                    reminder.task_id,
                    reminder.scheduled_at.to_rfc3339(),
                    if reminder.delivered_at.is_some() {
                        "  delivered"
                    } else {
                        ""
                    }
                );
            }
        }
        ReminderCommands::Add { task, at, before } => {
            let task = unique_task(&db.get_all_tasks()?, &task)?;
            let scheduled_at = if let Some(at) = at {
                tickit::notifications::parse_datetime(&at)?
            } else {
                let duration = tickit::notifications::parse_duration(
                    before.as_deref().expect("clap requires it"),
                )?;
                task.due_date
                    .context("--before requires the task to have a due date")?
                    .checked_sub_signed(duration)
                    .context("reminder is outside supported range")?
            };
            let reminder = tickit::models::Reminder::new(task.id, scheduled_at);
            db.create_reminder(&reminder)?;
            println!("✓ Added reminder {}", reminder.id);
        }
        ReminderCommands::Delete { reminder } => {
            if !db.delete_reminder(reminder)? {
                anyhow::bail!("reminder not found");
            }
        }
        ReminderCommands::Snooze { reminder, duration } => {
            let at = Utc::now()
                .checked_add_signed(tickit::notifications::parse_duration(&duration)?)
                .context("snooze is outside supported range")?;
            if !db.snooze_reminder(reminder, at)? {
                anyhow::bail!("reminder not found");
            }
        }
        ReminderCommands::InstallSystemd => {
            let exe = std::env::current_exe().context("cannot determine current executable")?;
            let dir = dirs::config_dir()
                .context("cannot determine config directory")?
                .join("systemd/user");
            fs::create_dir_all(&dir)?;
            let (service, timer) = render_systemd_units(&exe);
            fs::write(dir.join("tickit-reminders.service"), service)?;
            fs::write(dir.join("tickit-reminders.timer"), timer)?;
            systemctl(&["daemon-reload"])?;
            systemctl(&["enable", "--now", "tickit-reminders.timer"])?;
            println!("✓ Installed tickit-reminders.timer");
        }
        ReminderCommands::UninstallSystemd => {
            let dir = dirs::config_dir()
                .context("cannot determine config directory")?
                .join("systemd/user");
            let _ = Command::new("systemctl")
                .args(["--user", "disable", "--now", "tickit-reminders.timer"])
                .status();
            for name in ["tickit-reminders.service", "tickit-reminders.timer"] {
                match fs::remove_file(dir.join(name)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            systemctl(&["daemon-reload"])?;
            println!("✓ Uninstalled reminder timer");
        }
    }
    Ok(())
}

fn render_systemd_units(exe: &std::path::Path) -> (String, String) {
    let escaped = exe
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    (format!("[Unit]\nDescription=Check Tickit reminders\n\n[Service]\nType=oneshot\nExecStart=\"{}\" reminders check\n", escaped),
     "[Unit]\nDescription=Check Tickit reminders every minute\n\n[Timer]\nOnCalendar=*-*-* *:*:00\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n".to_string())
}

fn systemctl(args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .context("failed to execute systemctl")?;
    if !status.success() {
        anyhow::bail!("systemctl --user {} failed", args.join(" "));
    }
    Ok(())
}

/// Run the sync command
fn run_sync_command(status_only: bool, force: bool) -> Result<()> {
    use tickit::{
        Config, Database,
        sync::{SyncClient, SyncRecord},
    };

    let config = Config::load()?;
    let db = Database::open()?;

    if !config.sync.enabled {
        println!("⚠ Sync is disabled in config.");
        println!("\nTo enable sync, add to ~/.config/tickit/config.toml:");
        println!();
        println!("  [sync]");
        println!("  enabled = true");
        println!("  server = \"http://your-server:3030\"");
        println!("  token = \"your-token\"");
        return Ok(());
    }

    if config.sync.server.is_none() || config.sync.token.is_none() {
        println!("⚠ Sync is enabled but not configured.");
        println!("\nMissing server and/or token in config.");
        return Ok(());
    }

    let mut client = SyncClient::new(config.sync.clone());

    if status_only {
        let last_sync = db.get_last_sync()?;
        println!("Sync Status:");
        println!(
            "  Server: {}",
            config.sync.server.as_deref().unwrap_or("not set")
        );
        println!("  Enabled: {}", config.sync.enabled);
        println!(
            "  Last sync: {}",
            last_sync
                .map(|t| t.to_string())
                .unwrap_or_else(|| "never".to_string())
        );
        return Ok(());
    }

    if force {
        println!("⟳ Force syncing (ignoring last_sync)...");
    } else {
        println!("⟳ Syncing...");
    }

    // Gather local changes - use None for force sync to get everything
    let last_sync = if force { None } else { db.get_last_sync()? };
    let mut changes: Vec<SyncRecord> = Vec::new();

    // Get all data for full sync, or changes since last sync
    let tasks = if let Some(since) = last_sync {
        db.get_tasks_since(since)?
    } else {
        db.get_all_tasks()?
    };
    for task in tasks {
        changes.push(SyncRecord::Task(task));
    }

    let lists = if let Some(since) = last_sync {
        db.get_lists_since(since)?
    } else {
        db.get_lists()?
    };
    for list in lists {
        changes.push(SyncRecord::List(list));
    }

    let tags = if let Some(since) = last_sync {
        db.get_tags_since(since)?
    } else {
        db.get_tags()?
    };
    for tag in tags {
        changes.push(SyncRecord::Tag(tag));
    }

    // Get tombstones
    if let Some(since) = last_sync {
        let tombstones = db.get_tombstones_since(since)?;
        for tomb in tombstones {
            let record_type = match tomb.1.as_str() {
                "task" => tickit::sync::RecordType::Task,
                "list" => tickit::sync::RecordType::List,
                "tag" => tickit::sync::RecordType::Tag,
                "task_tag" => tickit::sync::RecordType::TaskTag,
                _ => continue,
            };
            changes.push(SyncRecord::Deleted {
                id: tomb.0,
                record_type,
                deleted_at: tomb.2,
            });
        }
    }

    println!("  Uploading {} changes...", changes.len());

    // Sync - pass None for force sync to get all changes from server
    match client.sync(changes, if force { None } else { db.get_last_sync()? }) {
        Ok(response) => {
            println!("  Received {} changes from server", response.changes.len());

            let report = db.apply_sync_records(&response.changes)?;
            if !report.rejected.is_empty() {
                println!(
                    "✗ Sync incomplete: applied {}, rejected {} incoming record(s).",
                    report.applied,
                    report.rejected.len()
                );
                for failure in &report.rejected {
                    println!(
                        "  - {}: {}",
                        sync_record_label(&failure.record),
                        failure.error
                    );
                }
                anyhow::bail!(
                    "sync cursor was not advanced; retry after resolving rejected records"
                );
            }

            // Update last sync time only after every incoming record was
            // applied and foreign-key mode was restored.
            db.set_last_sync(response.server_time)?;

            if !response.conflicts.is_empty() {
                println!("  ⚠ {} conflicts (server won)", response.conflicts.len());
            }

            println!("✓ Sync complete! Applied {} changes.", report.applied);
        }
        Err(e) => {
            println!("✗ Sync failed: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn sync_record_label(record: &SyncRecord) -> &'static str {
    match record {
        SyncRecord::Task(_) => "task",
        SyncRecord::List(_) => "list",
        SyncRecord::Tag(_) => "tag",
        SyncRecord::TaskTag(_) => "task_tag",
        SyncRecord::Deleted { .. } => "deletion",
    }
}

/// Run the update command
fn run_update_command() {
    use tickit::{
        VERSION, VersionCheck, check_for_updates_crates_io, detect_package_manager, run_update,
    };

    println!("✓ Checking for updates...\n");

    let pm = detect_package_manager();
    println!("  Installed via: {}", pm.name());
    println!("  Current version: {}", VERSION);

    // Use crates.io API (no rate limits, more reliable)
    let check = check_for_updates_crates_io();

    match check {
        VersionCheck::UpdateAvailable { latest, .. } => {
            println!("  Latest version: {}", latest);
            println!("\n⬆ Update available! Installing...\n");

            match run_update(&pm) {
                Ok(()) => {
                    println!("✓ Successfully updated to {}!", latest);
                    println!("\nRestart tickit to use the new version.");
                }
                Err(e) => {
                    println!("✗ Update failed: {}", e);
                    println!("\nYou can manually update with:");
                    println!("  {}", pm.update_command());
                    std::process::exit(1);
                }
            }
        }
        VersionCheck::UpToDate => {
            println!("\n✓ Already on the latest version!");
        }
        VersionCheck::CheckFailed(msg) => {
            println!("\n⚠ Could not check for updates: {}", msg);
            std::process::exit(1);
        }
    }
}

/// Find a task by ID or partial title match
fn find_task(tasks: &[Task], query: &str) -> Option<Task> {
    // Try UUID first
    if let Ok(uuid) = uuid::Uuid::parse_str(query) {
        return tasks.iter().find(|t| t.id == uuid).cloned();
    }

    // Try partial title match
    let query_lower = query.to_lowercase();
    tasks
        .iter()
        .find(|t| t.title.to_lowercase().contains(&query_lower))
        .cloned()
}

#[cfg(test)]
mod reminder_cli_tests {
    use super::*;

    #[test]
    fn parses_repeatable_reminders() {
        let cli = Cli::try_parse_from([
            "tickit",
            "add",
            "Task",
            "--due",
            "2026-07-20 12:00",
            "--remind",
            "2h-before",
            "--remind",
            "2026-07-20T09:00:00Z",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Add { reminders, .. }) => assert_eq!(reminders.len(), 2),
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn systemd_unit_quotes_exact_executable_and_is_persistent() {
        let (service, timer) =
            render_systemd_units(std::path::Path::new("/tmp/tickit build/tickit"));
        assert!(service.contains("ExecStart=\"/tmp/tickit build/tickit\" reminders check"));
        assert!(timer.contains("OnCalendar=*-*-* *:*:00"));
        assert!(timer.contains("Persistent=true"));
    }
}

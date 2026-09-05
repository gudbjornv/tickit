//! Data models for Tickit

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Custom deserializer for due_date that handles both date-only and full timestamp formats
mod date_or_datetime {
    use chrono::{DateTime, NaiveDate, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(date: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match date {
            Some(dt) => serializer.serialize_some(&dt.to_rfc3339()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            None => Ok(None),
            Some(s) if s.is_empty() => Ok(None),
            Some(s) => {
                // Try full datetime first
                if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
                    return Ok(Some(dt.with_timezone(&Utc)));
                }
                // Try date-only format (YYYY-MM-DD)
                if let Ok(date) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                    let dt = date.and_hms_opt(23, 59, 59).unwrap().and_utc();
                    return Ok(Some(dt));
                }
                // Try other common formats
                if let Ok(dt) = s.parse::<DateTime<Utc>>() {
                    return Ok(Some(dt));
                }
                Err(serde::de::Error::custom(format!(
                    "invalid date format: {}",
                    s
                )))
            }
        }
    }
}

/// Priority level for tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Low priority
    Low,
    /// Normal/Medium priority (default)
    #[default]
    Medium,
    /// High priority
    High,
    /// Urgent priority
    Urgent,
}

impl Priority {
    /// Get all priority levels
    pub const fn all() -> &'static [Self] {
        &[Self::Low, Self::Medium, Self::High, Self::Urgent]
    }

    /// Get the display name
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Urgent => "Urgent",
        }
    }

    /// Get the icon for this priority
    pub const fn icon(&self) -> &'static str {
        match self {
            Self::Low => "○",
            Self::Medium => "◐",
            Self::High => "●",
            Self::Urgent => "◉",
        }
    }

    /// Get next priority (cycles)
    pub fn next(&self) -> Self {
        match self {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Urgent,
            Self::Urgent => Self::Low,
        }
    }

    /// Get previous priority (cycles)
    pub fn prev(&self) -> Self {
        match self {
            Self::Low => Self::Urgent,
            Self::Medium => Self::Low,
            Self::High => Self::Medium,
            Self::Urgent => Self::High,
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// A task/todo item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier
    pub id: Uuid,
    /// Task title
    pub title: String,
    /// Optional description
    pub description: Option<String>,
    /// Optional URL (can be opened in browser)
    pub url: Option<String>,
    /// Priority level
    pub priority: Priority,
    /// Whether the task is completed
    pub completed: bool,
    /// ID of the list this task belongs to
    pub list_id: Uuid,
    /// IDs of tags attached to this task
    pub tag_ids: Vec<Uuid>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
    /// Completion timestamp (if completed)
    pub completed_at: Option<DateTime<Utc>>,
    /// Optional due date
    #[serde(default, with = "date_or_datetime")]
    pub due_date: Option<DateTime<Utc>>,
}

impl Task {
    /// Create a new task with the given title
    pub fn new(title: impl Into<String>, list_id: Uuid) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            description: None,
            url: None,
            priority: Priority::default(),
            completed: false,
            list_id,
            tag_ids: Vec::new(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            due_date: None,
        }
    }

    /// Mark the task as completed
    pub fn complete(&mut self) {
        self.completed = true;
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark the task as not completed
    pub fn uncomplete(&mut self) {
        self.completed = false;
        self.completed_at = None;
        self.updated_at = Utc::now();
    }

    /// Toggle completion status
    pub fn toggle(&mut self) {
        if self.completed {
            self.uncomplete();
        } else {
            self.complete();
        }
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the URL
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Set the priority
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag_id: Uuid) -> Self {
        if !self.tag_ids.contains(&tag_id) {
            self.tag_ids.push(tag_id);
        }
        self
    }

    /// Set the due date
    pub fn with_due_date(mut self, due_date: DateTime<Utc>) -> Self {
        self.due_date = Some(due_date);
        self
    }
}

/// Workflow state used by project-aware and agent-managed tasks.
///
/// The legacy `Task.completed` flag remains the source of truth for the
/// existing UI and sync protocol; workflow records provide richer state for
/// automation without breaking older databases or servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Backlog,
    Ready,
    Claimed,
    InProgress,
    Blocked,
    InReview,
    Verified,
    Done,
    Cancelled,
}

impl TaskStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Ready => "ready",
            Self::Claimed => "claimed",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::InReview => "in_review",
            Self::Verified => "verified",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "backlog" => Some(Self::Backlog),
            "ready" => Some(Self::Ready),
            "claimed" => Some(Self::Claimed),
            "in_progress" | "in-progress" | "inprogress" => Some(Self::InProgress),
            "blocked" => Some(Self::Blocked),
            "in_review" | "in-review" | "inreview" => Some(Self::InReview),
            "verified" => Some(Self::Verified),
            "done" | "complete" | "completed" => Some(Self::Done),
            "cancelled" | "canceled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Rich workflow metadata stored separately from the legacy task record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskWorkflow {
    pub task_id: Uuid,
    pub status: TaskStatus,
    pub created_by: Option<String>,
    pub owner: Option<String>,
    pub review_required: bool,
    pub blocked_reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskDependency {
    pub task_id: Uuid,
    pub depends_on: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEvent {
    pub id: Uuid,
    pub task_id: Uuid,
    pub actor: String,
    pub event_type: String,
    pub message: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRun {
    pub id: Uuid,
    pub task_id: Uuid,
    pub agent: String,
    pub conversation_id: Option<String>,
    pub status: String,
    pub workspace: Option<String>,
    pub branch: Option<String>,
    pub commit_sha: Option<String>,
    pub pull_request_url: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRunUpdate {
    pub status: String,
    pub workspace: Option<String>,
    pub branch: Option<String>,
    pub commit_sha: Option<String>,
    pub pull_request_url: Option<String>,
    pub error: Option<String>,
    pub actor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentJob {
    pub id: Uuid,
    pub task_id: Uuid,
    pub agent: String,
    pub status: String,
    pub instructions: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A local, non-synced notification scheduled for a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub id: Uuid,
    pub task_id: Uuid,
    pub scheduled_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
}

impl Reminder {
    pub fn new(task_id: Uuid, scheduled_at: DateTime<Utc>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            task_id,
            scheduled_at,
            created_at: now,
            updated_at: now,
            claimed_at: None,
            delivered_at: None,
        }
    }
}

/// A list/project that contains tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct List {
    /// Unique identifier
    pub id: Uuid,
    /// List name
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Icon/emoji for the list
    pub icon: String,
    /// Color for the list (hex code or name)
    pub color: Option<String>,
    /// Whether this is the default inbox list
    pub is_inbox: bool,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
    /// Sort order
    pub sort_order: i32,
}

impl List {
    /// Create a new list with the given name
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            icon: "📋".to_string(),
            color: None,
            is_inbox: false,
            created_at: now,
            updated_at: now,
            sort_order: 0,
        }
    }

    /// Create the default Inbox list
    pub fn inbox() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: "Inbox".to_string(),
            description: Some("Default list for new tasks".to_string()),
            icon: "📥".to_string(),
            color: None,
            is_inbox: true,
            created_at: now,
            updated_at: now,
            sort_order: -1, // Always first
        }
    }

    /// Set the icon
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Set the color
    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// A tag that can be attached to tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    /// Unique identifier
    pub id: Uuid,
    /// Tag name
    pub name: String,
    /// Color for the tag (hex code)
    pub color: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

impl Tag {
    /// Create a new tag with the given name
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            color: Self::random_color(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Set the color
    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = color.into();
        self
    }

    /// Mark as updated (sets updated_at to now)
    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    /// Generate a random pleasant color
    fn random_color() -> String {
        const COLORS: &[&str] = &[
            "#f38ba8", // Red
            "#fab387", // Peach
            "#f9e2af", // Yellow
            "#a6e3a1", // Green
            "#94e2d5", // Teal
            "#89b4fa", // Blue
            "#cba6f7", // Mauve
            "#f5c2e7", // Pink
            "#eba0ac", // Maroon
            "#89dceb", // Sky
        ];
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as usize;
        COLORS[seed % COLORS.len()].to_string()
    }
}

/// Export format for tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// JSON format
    Json,
    /// todo.txt format
    TodoTxt,
    /// Markdown format
    Markdown,
    /// CSV format
    Csv,
}

impl ExportFormat {
    /// Get all export formats
    pub const fn all() -> &'static [Self] {
        &[Self::Json, Self::TodoTxt, Self::Markdown, Self::Csv]
    }

    /// Get the display name
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::TodoTxt => "todo.txt",
            Self::Markdown => "Markdown",
            Self::Csv => "CSV",
        }
    }

    /// Get the file extension
    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::TodoTxt => "txt",
            Self::Markdown => "md",
            Self::Csv => "csv",
        }
    }
}

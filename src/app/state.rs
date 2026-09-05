//! Application state management

use anyhow::Result;
use std::collections::HashMap;
use uuid::Uuid;

use crate::config::Config;
use crate::db::Database;
use crate::models::{List, Priority, Reminder, Tag, Task, TaskStatus, TaskWorkflow};
use crate::sync::SyncStatus;
use crate::theme::Theme;

/// Settings menu items
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsItem {
    Theme,
    SyncEnabled,
    SyncServer,
    SyncToken,
    SyncInterval,
    Notifications,
    ShowCompletedDefault,
}

impl SettingsItem {
    pub fn all() -> &'static [SettingsItem] {
        &[
            SettingsItem::Theme,
            SettingsItem::SyncEnabled,
            SettingsItem::SyncServer,
            SettingsItem::SyncToken,
            SettingsItem::SyncInterval,
            SettingsItem::Notifications,
            SettingsItem::ShowCompletedDefault,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            SettingsItem::Theme => "Theme",
            SettingsItem::SyncEnabled => "Sync Enabled",
            SettingsItem::SyncServer => "Sync Server",
            SettingsItem::SyncToken => "Sync Token",
            SettingsItem::SyncInterval => "Sync Interval",
            SettingsItem::Notifications => "Notifications",
            SettingsItem::ShowCompletedDefault => "Show Completed",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            SettingsItem::Theme => "🎨",
            SettingsItem::SyncEnabled => "🔄",
            SettingsItem::SyncServer => "🌐",
            SettingsItem::SyncToken => "🔑",
            SettingsItem::SyncInterval => "⏱️",
            SettingsItem::Notifications => "🔔",
            SettingsItem::ShowCompletedDefault => "✓",
        }
    }
}

/// Input mode for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Normal navigation mode
    #[default]
    Normal,
    /// Theme picker dialog
    ThemePicker,
    /// Help dialog
    Help,
    /// Settings dialog
    Settings,
    /// Settings text input (for server/token)
    SettingsInput,
    /// Adding a new task
    AddTask,
    /// Editing a task
    EditTask,
    /// Adding a new list
    AddList,
    /// Editing a list
    EditList,
    /// Adding a new tag
    AddTag,
    /// Editing a tag
    EditTag,
    /// Confirmation dialog
    Confirm,
    /// Export dialog
    Export,
    /// About dialog
    About,
    /// Update confirmation dialog
    UpdateConfirm,
    /// Update in progress
    Updating,
}

/// Current view/tab
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// All tasks view
    #[default]
    Tasks,
    /// Lists view
    Lists,
    /// Tags view
    Tags,
}

impl View {
    pub const fn all() -> &'static [Self] {
        &[Self::Tasks, Self::Lists, Self::Tags]
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Tasks => "Tasks",
            Self::Lists => "Lists",
            Self::Tags => "Tags",
        }
    }

    pub const fn icon(&self) -> &'static str {
        match self {
            Self::Tasks => "✓",
            Self::Lists => "📋",
            Self::Tags => "🏷",
        }
    }
}

/// Focus area within a view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// Sidebar (lists)
    Sidebar,
    /// Main content area
    #[default]
    Main,
}

/// Editor field being edited
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorField {
    #[default]
    Title,
    Description,
    Url,
    Priority,
    List,
    Tags,
    DueDate,
    Reminders,
    Name,
    Icon,
    Color,
}

/// Application state
pub struct AppState {
    /// Configuration
    pub config: Config,
    /// Database connection
    pub db: Database,
    /// Current theme (cached from config)
    pub theme: Theme,
    /// Whether to quit the app
    pub should_quit: bool,
    /// Current input mode
    pub mode: Mode,
    /// Current view
    pub view: View,
    /// Current focus area
    pub focus: Focus,

    // Data
    /// All lists
    pub lists: Vec<List>,
    /// All tags
    pub tags: Vec<Tag>,
    /// Current tasks (filtered by selected list)
    pub tasks: Vec<Task>,
    /// Workflow records loaded during the last refresh (used by rendering)
    pub workflows: HashMap<Uuid, TaskWorkflow>,
    /// Currently selected list ID (None = all tasks)
    pub selected_list_id: Option<Uuid>,

    // Selection indices
    /// Selected list index in sidebar
    pub list_index: usize,
    /// Selected task index in main view
    pub task_index: usize,
    /// Selected tag index in tags view
    pub tag_index: usize,
    /// Theme picker index
    pub theme_index: usize,
    /// Settings menu index
    pub settings_index: usize,
    /// Which settings item is being edited (for text input)
    pub settings_editing: Option<SettingsItem>,

    // Editor state
    /// Current editor field
    pub editor_field: EditorField,
    /// Input buffer for text editing
    pub input_buffer: String,
    /// Cursor position in input buffer
    pub cursor_pos: usize,
    /// Task being edited (for edit mode)
    pub editing_task: Option<Task>,
    /// List being edited (for edit mode)
    pub editing_list: Option<List>,
    /// Tag being edited (for edit mode)
    pub editing_tag: Option<Tag>,
    /// Selected priority in editor
    pub editor_priority: Priority,
    /// Selected list in task editor
    pub editor_list_index: usize,
    /// Selected tags in task editor
    pub editor_tag_indices: Vec<usize>,
    /// Current tag cursor in task editor (for tag selection)
    pub editor_tag_cursor: usize,
    /// Adding new tag inline in task editor
    pub editor_adding_tag: bool,
    /// New tag name buffer
    pub editor_new_tag_buffer: String,
    /// Title buffer for new tasks (preserved when switching fields)
    pub editor_title_buffer: String,
    /// Description buffer for tasks
    pub editor_description_buffer: String,
    /// Due date buffer for tasks (YYYY-MM-DD format)
    pub editor_due_date_buffer: String,
    /// Comma-separated absolute reminder times or offsets such as `2h-before`.
    pub editor_reminders_buffer: String,

    // UI state
    /// Show completed tasks
    pub show_completed: bool,
    /// Confirmation message
    pub confirm_message: String,
    /// Confirmation callback action
    pub confirm_action: Option<ConfirmAction>,
    /// Status message
    pub status_message: Option<String>,
    /// Status message expiry tick
    pub status_expiry: usize,
    /// Animation tick counter
    pub tick: usize,
    /// Show help overlay
    pub show_help: bool,

    // Update state
    /// Available update version (if any)
    pub update_available: Option<String>,
    /// Whether user accepted the update
    pub pending_update: bool,
    /// Update result message
    pub update_result: Option<String>,

    // Sync state
    /// Sync status for UI display
    pub sync_status: SyncStatus,
    /// Flag to trigger sync after data changes
    pub sync_pending: bool,
}

/// Actions that need confirmation
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteTask(Uuid),
    DeleteList(Uuid),
    DeleteTag(Uuid),
}

impl AppState {
    /// Create a new app state
    pub fn new(config: Config, db: Database) -> Result<Self> {
        let theme = config.theme;
        let show_completed = config.show_completed;

        let mut state = Self {
            config,
            db,
            theme,
            should_quit: false,
            mode: Mode::Normal,
            view: View::Tasks,
            focus: Focus::Main,
            lists: Vec::new(),
            tags: Vec::new(),
            tasks: Vec::new(),
            workflows: HashMap::new(),
            selected_list_id: None,
            list_index: 0,
            task_index: 0,
            tag_index: 0,
            theme_index: 0,
            settings_index: 0,
            settings_editing: None,
            editor_field: EditorField::Title,
            input_buffer: String::new(),
            cursor_pos: 0,
            editing_task: None,
            editing_list: None,
            editing_tag: None,
            editor_priority: Priority::Medium,
            editor_list_index: 0,
            editor_tag_indices: Vec::new(),
            editor_tag_cursor: 0,
            editor_adding_tag: false,
            editor_new_tag_buffer: String::new(),
            editor_title_buffer: String::new(),
            editor_description_buffer: String::new(),
            editor_due_date_buffer: String::new(),
            editor_reminders_buffer: String::new(),
            show_completed,
            confirm_message: String::new(),
            confirm_action: None,
            status_message: None,
            status_expiry: 0,
            tick: 0,
            show_help: false,
            update_available: None,
            pending_update: false,
            update_result: None,
            sync_status: SyncStatus::default(),
            sync_pending: false,
        };

        state.refresh_data()?;

        // Set theme index
        state.theme_index = Theme::all()
            .iter()
            .position(|t| *t == state.theme.inner())
            .unwrap_or(0);

        Ok(state)
    }

    /// Refresh all data from database
    pub fn refresh_data(&mut self) -> Result<()> {
        self.lists = self.db.get_lists()?;
        self.tags = self.db.get_tags()?;
        self.refresh_tasks()?;

        // Clamp indices (no more "All" so max is lists.len() - 1)
        if !self.lists.is_empty() && self.list_index >= self.lists.len() {
            self.list_index = self.lists.len() - 1;
        }
        if !self.tasks.is_empty() && self.task_index >= self.tasks.len() {
            self.task_index = self.tasks.len() - 1;
        }
        if !self.tags.is_empty() && self.tag_index >= self.tags.len() {
            self.tag_index = self.tags.len() - 1;
        }

        Ok(())
    }

    /// Refresh tasks based on current filter
    pub fn refresh_tasks(&mut self) -> Result<()> {
        let completed_filter = if self.show_completed {
            None
        } else {
            Some(false)
        };

        // Check if Inbox is selected - if so, show all tasks (like "All" did before)
        let is_inbox_selected = self.selected_list().map(|l| l.is_inbox).unwrap_or(false);

        self.tasks = if is_inbox_selected {
            // Inbox shows all tasks from all lists
            self.db
                .get_tasks_with_filter(None, completed_filter, None)?
        } else if let Some(list_id) = self.selected_list_id {
            self.db
                .get_tasks_with_filter(Some(list_id), completed_filter, None)?
        } else {
            self.db
                .get_tasks_with_filter(None, completed_filter, None)?
        };

        // Rendering must use this refresh-time cache rather than issuing one
        // database query per task on every frame.
        self.workflows = self.db.get_task_workflows()?;

        // Clamp task index
        if !self.tasks.is_empty() && self.task_index >= self.tasks.len() {
            self.task_index = self.tasks.len() - 1;
        }

        Ok(())
    }

    /// Get the currently selected task
    pub fn selected_task(&self) -> Option<&Task> {
        self.tasks.get(self.task_index)
    }

    /// Get the currently selected list
    pub fn selected_list(&self) -> Option<&List> {
        self.lists.get(self.list_index)
    }

    /// Get the currently selected tag
    pub fn selected_tag(&self) -> Option<&Tag> {
        self.tags.get(self.tag_index)
    }

    /// Toggle show completed tasks
    pub fn toggle_show_completed(&mut self) {
        self.show_completed = !self.show_completed;
        self.config.show_completed = self.show_completed;
        let _ = self.refresh_tasks();
    }

    /// Queue the selected task for the configured local agent bridge.
    pub fn dispatch_selected_task(&mut self) -> Result<()> {
        let task = self
            .selected_task()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no task selected"))?;
        self.db.enqueue_agent_job(task.id, "codex", None, "tui")?;
        self.set_status(format!("Queued agent work for {}", task.title));
        Ok(())
    }

    /// Set a status message
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
        self.status_expiry = self.tick + 30; // ~3 seconds
    }

    /// Tick for animations and status expiry
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if self.status_expiry > 0 && self.tick >= self.status_expiry {
            self.status_message = None;
        }
    }

    /// Start adding a new task
    pub fn start_add_task(&mut self) {
        self.mode = Mode::AddTask;
        self.editor_field = EditorField::Title;
        self.input_buffer.clear();
        self.cursor_pos = 0;
        self.editing_task = None;
        self.editor_priority = Priority::Medium;
        self.editor_tag_indices.clear();
        self.editor_tag_cursor = 0;
        self.editor_adding_tag = false;
        self.editor_new_tag_buffer.clear();
        self.editor_title_buffer.clear();
        self.editor_description_buffer.clear();
        self.editor_due_date_buffer.clear();
        self.editor_reminders_buffer.clear();

        // Set editor list to current selected list or inbox
        if let Some(list_id) = self.selected_list_id {
            self.editor_list_index = self.lists.iter().position(|l| l.id == list_id).unwrap_or(0);
        } else {
            self.editor_list_index = self.lists.iter().position(|l| l.is_inbox).unwrap_or(0);
        }
    }

    /// Start editing the selected task
    pub fn start_edit_task(&mut self) {
        if let Some(task) = self.selected_task().cloned() {
            self.mode = Mode::EditTask;
            self.editor_field = EditorField::Title;
            self.input_buffer = task.title.clone();
            self.cursor_pos = self.input_buffer.len();
            self.editor_priority = task.priority;
            self.editor_list_index = self
                .lists
                .iter()
                .position(|l| l.id == task.list_id)
                .unwrap_or(0);
            self.editor_tag_indices = task
                .tag_ids
                .iter()
                .filter_map(|tid| self.tags.iter().position(|t| t.id == *tid))
                .collect();
            self.editor_tag_cursor = 0;
            self.editor_adding_tag = false;
            self.editor_new_tag_buffer.clear();
            self.editor_title_buffer = task.title.clone();
            self.editor_description_buffer = task.description.clone().unwrap_or_default();
            self.editor_due_date_buffer = task
                .due_date
                .map(format_editor_datetime)
                .unwrap_or_default();
            self.editor_reminders_buffer = self
                .db
                .list_reminders_for_task(task.id)
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.delivered_at.is_none())
                .map(|r| format_reminder_datetime(r.scheduled_at))
                .collect::<Vec<_>>()
                .join(", ");
            self.editing_task = Some(task);
        }
    }

    /// Save the current task being edited
    pub fn save_task(&mut self) -> Result<()> {
        // Save current field first
        self.save_current_field_to_buffer();

        let list_id = self
            .lists
            .get(self.editor_list_index)
            .map(|l| l.id)
            .unwrap_or_else(|| self.lists.iter().find(|l| l.is_inbox).unwrap().id);

        let tag_ids: Vec<Uuid> = self
            .editor_tag_indices
            .iter()
            .filter_map(|&i| self.tags.get(i).map(|t| t.id))
            .collect();

        // Get title from the appropriate source
        let title = if self.editor_field == EditorField::Title {
            self.input_buffer.clone()
        } else {
            self.editor_title_buffer.clone()
        };

        // Get description from the appropriate source
        let description = if self.editor_field == EditorField::Description {
            if self.input_buffer.is_empty() {
                None
            } else {
                Some(self.input_buffer.clone())
            }
        } else if self.editor_description_buffer.is_empty() {
            None
        } else {
            Some(self.editor_description_buffer.clone())
        };

        // Parse due date from buffer
        let due_value = if self.editor_field == EditorField::DueDate {
            self.input_buffer.clone()
        } else {
            self.editor_due_date_buffer.clone()
        };
        let due_date = if due_value.trim().is_empty() {
            None
        } else {
            match crate::notifications::parse_datetime(&due_value) {
                Ok(value) => Some(value),
                Err(error) => {
                    self.set_status(format!("Invalid due time: {error}"));
                    return Ok(());
                }
            }
        };
        let reminder_value = if self.editor_field == EditorField::Reminders {
            self.input_buffer.clone()
        } else {
            self.editor_reminders_buffer.clone()
        };

        if title.is_empty() {
            self.set_status("Task title cannot be empty");
            return Ok(());
        }

        let reminder_times = match reminder_value
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|value| crate::notifications::parse_reminder_spec(value, due_date))
            .collect::<Result<Vec<_>>>()
        {
            Ok(values) => values,
            Err(error) => {
                self.set_status(format!("Invalid reminder: {error}"));
                return Ok(());
            }
        };

        if let Some(mut task) = self.editing_task.take() {
            // Update existing task
            task.title = title;
            task.description = description;
            task.priority = self.editor_priority;
            task.list_id = list_id;
            task.tag_ids = tag_ids;
            task.due_date = due_date;
            task.updated_at = chrono::Utc::now();
            self.db.update_task(&task)?;
            let reminders: Vec<_> = reminder_times
                .into_iter()
                .map(|at| Reminder::new(task.id, at))
                .collect();
            self.db.replace_pending_reminders(task.id, &reminders)?;
            self.set_status("Task updated");
        } else {
            // Create new task
            let mut task = Task::new(&title, list_id);
            task.description = description;
            task.priority = self.editor_priority;
            task.tag_ids = tag_ids;
            task.due_date = due_date;
            let reminders: Vec<_> = reminder_times
                .into_iter()
                .map(|at| Reminder::new(task.id, at))
                .collect();
            self.db.insert_task_with_reminders(&task, &reminders)?;
            self.set_status("Task created");
        }

        self.mode = Mode::Normal;
        self.refresh_data()?;
        self.mark_sync_pending();
        Ok(())
    }

    /// Toggle completion of the selected task
    pub fn toggle_task(&mut self) -> Result<()> {
        let Some(current) = self.tasks.get(self.task_index).cloned() else {
            return Ok(());
        };
        if self
            .workflows
            .get(&current.id)
            .is_some_and(|workflow| workflow.status == TaskStatus::Cancelled)
        {
            anyhow::bail!("cancelled tasks are terminal; change workflow status explicitly");
        }

        let mut updated = current;
        updated.toggle();
        // Persist first. If dependency validation rejects the completion, the
        // in-memory checkbox remains exactly as it was before the keypress.
        self.db.update_task(&updated)?;
        let status = if updated.completed {
            "completed"
        } else {
            "reopened"
        };
        self.set_status(format!("Task {}", status));
        self.refresh_tasks()?;
        self.mark_sync_pending();
        Ok(())
    }

    /// Delete the selected task (with confirmation)
    pub fn confirm_delete_task(&mut self) {
        if let Some(task) = self.selected_task() {
            let title = task.title.clone();
            let id = task.id;
            self.confirm_message = format!("Delete task \"{}\"?", title);
            self.confirm_action = Some(ConfirmAction::DeleteTask(id));
            self.mode = Mode::Confirm;
        }
    }

    /// Execute confirmed action
    pub fn execute_confirm(&mut self) -> Result<()> {
        if let Some(action) = self.confirm_action.take() {
            match action {
                ConfirmAction::DeleteTask(id) => {
                    self.db.delete_task(id)?;
                    self.db.record_tombstone(id, "task")?;
                    self.set_status("Task deleted");
                }
                ConfirmAction::DeleteList(id) => {
                    self.db.delete_list(id)?;
                    self.db.record_tombstone(id, "list")?;
                    self.selected_list_id = None;
                    self.list_index = 0;
                    self.set_status("List deleted");
                }
                ConfirmAction::DeleteTag(id) => {
                    self.db.delete_tag(id)?;
                    self.db.record_tombstone(id, "tag")?;
                    self.set_status("Tag deleted");
                }
            }
            self.mode = Mode::Normal;
            self.refresh_data()?;
            self.mark_sync_pending();
        }
        Ok(())
    }

    /// Cancel confirmation
    pub fn cancel_confirm(&mut self) {
        self.confirm_action = None;
        self.mode = Mode::Normal;
    }

    /// Open URL of selected task
    pub fn open_task_url(&mut self) {
        if let Some(task) = self.selected_task() {
            if let Some(url) = &task.url {
                if let Err(e) = open::that(url) {
                    self.set_status(format!("Failed to open URL: {}", e));
                } else {
                    self.set_status("Opening URL in browser...");
                }
            } else {
                self.set_status("Task has no URL");
            }
        }
    }

    /// Cycle task priority
    pub fn cycle_task_priority(&mut self) -> Result<()> {
        if let Some(task) = self.tasks.get_mut(self.task_index) {
            task.priority = task.priority.next();
            task.updated_at = chrono::Utc::now();
            self.db.update_task(task)?;
            self.mark_sync_pending();
        }
        if let Some(task) = self.tasks.get(self.task_index) {
            self.set_status(format!("Priority: {}", task.priority.name()));
        }
        Ok(())
    }

    /// Start adding a new list
    pub fn start_add_list(&mut self) {
        self.mode = Mode::AddList;
        self.editor_field = EditorField::Name;
        self.input_buffer.clear();
        self.cursor_pos = 0;
        self.editing_list = None;
    }

    /// Start editing the selected list
    pub fn start_edit_list(&mut self) {
        if let Some(list) = self.selected_list().cloned() {
            if list.is_inbox {
                self.set_status("Cannot edit inbox");
                return;
            }
            self.mode = Mode::EditList;
            self.editor_field = EditorField::Name;
            self.input_buffer = list.name.clone();
            self.cursor_pos = self.input_buffer.len();
            self.editing_list = Some(list);
        }
    }

    /// Save the current list being edited
    pub fn save_list(&mut self) -> Result<()> {
        if self.input_buffer.is_empty() {
            self.set_status("List name cannot be empty");
            return Ok(());
        }

        if let Some(mut list) = self.editing_list.take() {
            list.name = self.input_buffer.clone();
            list.updated_at = chrono::Utc::now();
            self.db.update_list(&list)?;
            self.set_status("List updated");
        } else {
            let list = List::new(&self.input_buffer);
            self.db.insert_list(&list)?;
            self.set_status("List created");
        }

        self.mode = Mode::Normal;
        self.refresh_data()?;
        self.mark_sync_pending();
        Ok(())
    }

    /// Confirm delete list
    pub fn confirm_delete_list(&mut self) {
        if let Some(list) = self.selected_list() {
            if list.is_inbox {
                self.set_status("Cannot delete inbox");
                return;
            }
            let name = list.name.clone();
            let id = list.id;
            self.confirm_message =
                format!("Delete list \"{}\"? Tasks will be moved to Inbox.", name);
            self.confirm_action = Some(ConfirmAction::DeleteList(id));
            self.mode = Mode::Confirm;
        }
    }

    /// Start adding a new tag
    pub fn start_add_tag(&mut self) {
        self.mode = Mode::AddTag;
        self.editor_field = EditorField::Name;
        self.input_buffer.clear();
        self.cursor_pos = 0;
        self.editing_tag = None;
    }

    /// Start editing the selected tag
    pub fn start_edit_tag(&mut self) {
        if let Some(tag) = self.selected_tag().cloned() {
            self.mode = Mode::EditTag;
            self.editor_field = EditorField::Name;
            self.input_buffer = tag.name.clone();
            self.cursor_pos = self.input_buffer.len();
            self.editing_tag = Some(tag);
        }
    }

    /// Save the current tag being edited
    pub fn save_tag(&mut self) -> Result<()> {
        if self.input_buffer.is_empty() {
            self.set_status("Tag name cannot be empty");
            return Ok(());
        }

        if let Some(mut tag) = self.editing_tag.take() {
            tag.name = self.input_buffer.clone();
            tag.touch(); // Update the updated_at timestamp
            self.db.update_tag(&tag)?;
            self.set_status("Tag updated");
        } else {
            let tag = Tag::new(&self.input_buffer);
            self.db.insert_tag(&tag)?;
            self.set_status("Tag created");
        }

        self.mode = Mode::Normal;
        self.refresh_data()?;
        self.mark_sync_pending();
        Ok(())
    }

    /// Confirm delete tag
    pub fn confirm_delete_tag(&mut self) {
        if let Some(tag) = self.selected_tag() {
            let name = tag.name.clone();
            let id = tag.id;
            self.confirm_message = format!("Delete tag \"{}\"?", name);
            self.confirm_action = Some(ConfirmAction::DeleteTag(id));
            self.mode = Mode::Confirm;
        }
    }

    /// Change theme
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.config.theme = theme;
    }

    /// Toggle tag selection at current cursor in task editor
    pub fn toggle_editor_tag(&mut self) {
        if self.tags.is_empty() {
            return;
        }
        let cursor = self.editor_tag_cursor;
        if cursor < self.tags.len() {
            if let Some(pos) = self.editor_tag_indices.iter().position(|&i| i == cursor) {
                self.editor_tag_indices.remove(pos);
            } else {
                self.editor_tag_indices.push(cursor);
            }
        }
    }

    /// Move tag cursor up
    pub fn editor_tag_cursor_up(&mut self) {
        if self.editor_tag_cursor > 0 {
            self.editor_tag_cursor -= 1;
        }
    }

    /// Move tag cursor down
    pub fn editor_tag_cursor_down(&mut self) {
        // +1 for "Add new tag" option
        if self.editor_tag_cursor < self.tags.len() {
            self.editor_tag_cursor += 1;
        }
    }

    /// Start adding a new tag inline in task editor
    pub fn start_inline_add_tag(&mut self) {
        self.editor_adding_tag = true;
        self.editor_new_tag_buffer.clear();
    }

    /// Save inline tag and add to selection
    pub fn save_inline_tag(&mut self) -> Result<()> {
        if self.editor_new_tag_buffer.is_empty() {
            self.editor_adding_tag = false;
            return Ok(());
        }

        let tag = Tag::new(&self.editor_new_tag_buffer);
        self.db.insert_tag(&tag)?;

        // Refresh tags and select the new one
        self.tags = self.db.get_tags()?;
        if let Some(idx) = self.tags.iter().position(|t| t.id == tag.id) {
            self.editor_tag_indices.push(idx);
            self.editor_tag_cursor = idx;
        }

        self.editor_adding_tag = false;
        self.editor_new_tag_buffer.clear();
        self.set_status(format!("Tag '{}' created", tag.name));
        Ok(())
    }

    /// Cancel inline tag creation
    pub fn cancel_inline_tag(&mut self) {
        self.editor_adding_tag = false;
        self.editor_new_tag_buffer.clear();
    }

    /// Navigate editor fields
    pub fn next_editor_field(&mut self) {
        // Save current field value before switching
        self.save_current_field_to_buffer();

        self.editor_field = match self.editor_field {
            EditorField::Title => EditorField::Description,
            EditorField::Description => EditorField::DueDate,
            EditorField::DueDate => EditorField::Reminders,
            EditorField::Reminders => EditorField::Priority,
            EditorField::Priority => EditorField::List,
            EditorField::List => EditorField::Tags,
            EditorField::Tags => EditorField::Title,
            _ => EditorField::Title,
        };
        self.update_input_buffer_for_field();
    }

    pub fn prev_editor_field(&mut self) {
        // Save current field value before switching
        self.save_current_field_to_buffer();

        self.editor_field = match self.editor_field {
            EditorField::Title => EditorField::Tags,
            EditorField::Description => EditorField::Title,
            EditorField::DueDate => EditorField::Description,
            EditorField::Priority => EditorField::Reminders,
            EditorField::Reminders => EditorField::DueDate,
            EditorField::List => EditorField::Priority,
            EditorField::Tags => EditorField::List,
            _ => EditorField::Title,
        };
        self.update_input_buffer_for_field();
    }

    fn save_current_field_to_buffer(&mut self) {
        // Save current field to dedicated buffer
        match self.editor_field {
            EditorField::Title => {
                self.editor_title_buffer = self.input_buffer.clone();
            }
            EditorField::Description => {
                self.editor_description_buffer = self.input_buffer.clone();
            }
            EditorField::DueDate => {
                self.editor_due_date_buffer = self.input_buffer.clone();
            }
            EditorField::Reminders => self.editor_reminders_buffer = self.input_buffer.clone(),
            _ => {}
        }
    }

    fn update_input_buffer_for_field(&mut self) {
        // Load the appropriate buffer for the current field
        self.input_buffer = match self.editor_field {
            EditorField::Title => self.editor_title_buffer.clone(),
            EditorField::Description => self.editor_description_buffer.clone(),
            EditorField::DueDate => self.editor_due_date_buffer.clone(),
            EditorField::Reminders => self.editor_reminders_buffer.clone(),
            _ => String::new(),
        };
        self.cursor_pos = self.input_buffer.len();
    }

    /// Set update available from background check
    pub fn set_update_available(&mut self, version: String) {
        self.update_available = Some(version);
    }

    /// Start the update process
    pub fn start_update(&mut self) {
        self.pending_update = true;
        self.set_status("Updating...");
    }

    /// Dismiss the update notification
    pub fn dismiss_update(&mut self) {
        self.update_available = None;
    }

    // ==================== Sync ====================

    /// Check if sync is enabled and configured
    pub fn is_sync_enabled(&self) -> bool {
        self.config.sync.enabled
            && self.config.sync.server.is_some()
            && self.config.sync.token.is_some()
    }

    /// Update sync status
    pub fn set_sync_status(&mut self, status: SyncStatus) {
        self.sync_status = status;
    }

    /// Mark sync as in progress
    pub fn set_syncing(&mut self, syncing: bool) {
        self.sync_status.syncing = syncing;
    }

    /// Set sync error
    pub fn set_sync_error(&mut self, error: Option<String>) {
        self.sync_status.last_error = error;
        self.sync_status.syncing = false;
    }

    /// Set last sync time
    pub fn set_last_sync(&mut self, time: chrono::DateTime<chrono::Utc>) {
        self.sync_status.last_sync = Some(time);
        self.sync_status.last_error = None;
        self.sync_status.syncing = false;
    }

    /// Mark that data has changed and sync is needed
    pub fn mark_sync_pending(&mut self) {
        if self.is_sync_enabled() {
            self.sync_pending = true;
        }
    }
}

fn format_editor_datetime(value: chrono::DateTime<chrono::Utc>) -> String {
    if value.time() == chrono::NaiveTime::from_hms_opt(23, 59, 59).expect("valid time") {
        value.format("%Y-%m-%d").to_string()
    } else {
        value.with_timezone(&chrono::Local).to_rfc3339()
    }
}

fn format_reminder_datetime(value: chrono::DateTime<chrono::Utc>) -> String {
    value.with_timezone(&chrono::Local).to_rfc3339()
}

#[cfg(test)]
mod reminder_editor_tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::tempdir;

    #[test]
    fn reminder_editor_format_includes_an_explicit_offset() {
        let value = chrono::Utc
            .with_ymd_and_hms(2026, 10, 25, 1, 30, 0)
            .unwrap();
        let rendered = format_reminder_datetime(value);
        assert!(chrono::DateTime::parse_from_rfc3339(&rendered).is_ok());
        assert!(
            rendered.ends_with('Z') || rendered[10..].contains('+') || rendered[10..].contains('-')
        );
    }

    #[test]
    fn precise_due_editor_format_includes_an_explicit_offset() {
        let value = chrono::Utc
            .with_ymd_and_hms(2026, 10, 25, 1, 30, 0)
            .unwrap();
        let rendered = format_editor_datetime(value);
        assert!(chrono::DateTime::parse_from_rfc3339(&rendered).is_ok());
    }

    #[test]
    fn dependency_rejection_does_not_flip_the_in_memory_checkbox() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("toggle.sqlite")).unwrap();
        let inbox = db.get_inbox().unwrap();
        let prerequisite = Task::new("Prerequisite", inbox.id);
        let dependent = Task::new("Dependent", inbox.id);
        db.insert_task(&prerequisite).unwrap();
        db.insert_task(&dependent).unwrap();
        db.add_dependency(dependent.id, prerequisite.id).unwrap();

        let mut state = AppState::new(Config::default(), db).unwrap();
        state.task_index = state
            .tasks
            .iter()
            .position(|task| task.id == dependent.id)
            .unwrap();
        assert!(!state.tasks[state.task_index].completed);

        assert!(state.toggle_task().is_err());
        assert!(!state.tasks[state.task_index].completed);
        assert!(
            !state
                .db
                .get_all_tasks()
                .unwrap()
                .into_iter()
                .find(|task| task.id == dependent.id)
                .unwrap()
                .completed
        );
    }
}

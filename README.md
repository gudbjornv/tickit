<p align="center">
  <img src="screenshots/01-tasks.png" alt="Tickit Tasks" width="700">
</p>

<h1 align="center">
  ✅ Tickit
</h1>

<p align="center">
  <strong>A stunning terminal-based task manager</strong>
</p>

<p align="center">
  <i>Organize your tasks, lists, and tags — all from your terminal.</i>
</p>

<p align="center">
  <a href="https://github.com/ricardodantas/tickit/releases">
    <img src="https://img.shields.io/github/v/release/ricardodantas/tickit?style=flat&labelColor=1e1e2e&color=cba6f7&logo=github&logoColor=white" alt="Release">
  </a>
  <a href="https://crates.io/crates/tickit">
    <img src="https://img.shields.io/crates/v/tickit?style=flat&labelColor=1e1e2e&color=fab387&logo=rust&logoColor=white" alt="Crates.io">
  </a>
  <a href="https://github.com/ricardodantas/tickit/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-MIT-89b4fa?style=flat&labelColor=1e1e2e" alt="License">
  </a>
  <a href="https://rust-lang.org">
    <img src="https://img.shields.io/badge/rust-1.93+-f9e2af?style=flat&labelColor=1e1e2e&logo=rust&logoColor=white" alt="Rust Version">
  </a>
</p>

<br>

## 📖 Table of Contents

- [✨ Features](#-features)
- [🚀 Quick Start](#-quick-start)
- [💻 CLI Commands](#-cli-commands)
- [⌨️ Keybindings](#️-keybindings)
- [🎨 Themes](#-themes)
- [📤 Export Formats](#-export-formats)
- [☁️ Sync (Optional)](#️-sync-optional)
- [🏗️ Architecture](#️-architecture)
- [🔧 Building from Source](#-building-from-source)
- [🤝 Contributing](#-contributing)
- [📄 License](#-license)

<br>

## ✨ Features

<table>
<tr>
<td width="50%">

### 📋 Task Management
Create, edit, and complete tasks with titles, descriptions, URLs, and priority levels.

### 📁 Lists
Organize tasks into lists with custom icons. Default Inbox for quick capture.

### 🏷️ Tags
Add colorful tags for flexible categorization and filtering.

</td>
<td width="50%">

### 🖥️ Beautiful TUI
A gorgeous terminal interface with vim-style navigation and real-time updates.

### ⌨️ Full CLI
Script your task management with powerful command-line tools.

### 📤 Export Anywhere
Export to JSON, todo.txt, Markdown, or CSV formats.

</td>
</tr>
</table>

<br>

### Feature Highlights

| Feature | Description |
|---------|-------------|
| ⚡ **Priority Levels** | Low, Medium, High, Urgent |
| 🔗 **Task URLs** | Attach links and open them with `o` |
| 🎨 **15 Built-in Themes** | From Dracula to Cyberpunk |
| ⚙️ **Settings Dialog** | Configure sync, notifications, and themes in-app |
| 💾 **SQLite Storage** | Fast, reliable, self-contained |
| 🔍 **Filter & Search** | By list, tag, or completion status |
| ✅ **Toggle Completed** | Show/hide completed tasks |
| 📅 **Due Dates** | Set deadlines for your tasks |
| 🔔 **Desktop Notifications** | Alerts for due and overdue tasks |
| 🔄 **Auto-Update** | Check for updates from TUI or CLI |
| ☁️ **Optional Sync** | Self-hosted sync server for multiple devices |

<br>

## 🚀 Quick Start

### Installation

#### From Source

```bash
git clone https://github.com/ricardodantas/tickit
cd tickit
cargo install --path .
```

#### From crates.io (coming soon)

```bash
cargo install tickit
```

### First Run

Simply launch the TUI:

```bash
tickit
```

Your tasks are stored in SQLite at `~/.config/tickit/tickit.sqlite`.

<br>

## 💻 CLI Commands

Tickit provides a full CLI for scripting and quick actions.

### Adding Tasks

```bash
# Simple task
tickit add "Buy groceries"

# With priority and list
tickit add "Review PR" --priority high --list Work

# With URL and tags
tickit add "Read article" --url "https://example.com" --tags reading,tech

# With description
tickit add "Write report" --description "Q4 summary for the team"

# Precise local due time and multiple reminders
tickit add "Submit report" --due "2026-07-24 16:00" \
  --remind 1d-before --remind 2h-before
```

### Reminders

Due values accept RFC3339 (`2026-07-24T16:00:00+02:00`), local
`YYYY-MM-DD HH:MM`, or the legacy date-only `YYYY-MM-DD` form. Local times that
are missing or ambiguous during a daylight-saving transition are rejected; use
RFC3339 with an explicit offset in that case. Reminder values are absolute
timestamps or concise offsets before the due time (`10m-before`, `2h-before`,
`1d-before`).

```bash
tickit reminders list
tickit reminders list "Submit report"
tickit reminders add "Submit report" --before 30m
tickit reminders add "Submit report" --at "2026-07-24T15:00:00+02:00"
tickit reminders snooze REMINDER_UUID 1h
tickit reminders delete REMINDER_UUID
tickit reminders check
```

On Linux, explicit reminder notifications offer Complete, Snooze 10m, Snooze
1h, and Open actions. The checker waits only for the notification's ten-second
timeout; Tickit does not run a permanent daemon. Open uses
`xdg-terminal-exec`, when available, to launch the TUI in the user's preferred
terminal.

For automatic checks, install a user-level systemd timer:

```bash
tickit reminders install-systemd
tickit reminders uninstall-systemd
```

The calendar timer wakes on each minute boundary, runs the exact installed
Tickit executable, and exits. `Persistent=true` catches up after shutdown or suspend, subject to
the configured grace period. Explicit reminders use a short claim lease so a
failed or interrupted delivery can be retried. These defaults can be changed in
`~/.config/tickit/config.toml`:

```toml
reminder_grace_minutes = 1440
reminder_claim_lease_minutes = 5
```

Explicit reminders and their delivery history are intentionally device-local
and are not included in Tickit's sync protocol. Date-only tasks retain Tickit's
legacy due-today/high-priority-tomorrow alerts.

### Listing Tasks

```bash
# Show incomplete tasks
tickit list

# Include completed tasks
tickit list --all

# Filter by list
tickit list --list Work

# Filter by tag
tickit list --tag urgent

# Output as JSON
tickit list --json
```

### Completing Tasks

```bash
# Mark as complete (partial match supported)
tickit done "Buy groceries"

# Mark as incomplete
tickit undo "Buy groceries"
```

### Deleting Tasks

```bash
# Delete with confirmation
tickit delete "Old task"

# Skip confirmation
tickit delete "Old task" --force

# Short alias
tickit rm "Old task" -f
```

### Managing Lists

```bash
# List all lists
tickit lists

# Add a new list
tickit lists add "Shopping" --icon "🛒"

# Delete a list
tickit lists delete "Old List"
```

### Managing Tags

```bash
# List all tags
tickit tags

# Add a new tag
tickit tags add "urgent" --color "#ff0000"

# Delete a tag
tickit tags delete "old-tag"
```

### Exporting Tasks

```bash
# Export to JSON (default)
tickit export --output tasks.json

# Export to todo.txt format
tickit export --format todotxt

# Export to Markdown
tickit export --format markdown --output tasks.md

# Export specific list to CSV
tickit export --format csv --list Work --output work.csv
```

### Updating Tickit

```bash
# Check for updates and install if available
tickit update
```

The update command automatically detects whether you installed via Cargo or Homebrew and uses the appropriate update method.

<br>

## ⌨️ Keybindings

### Global

| Key | Action |
|-----|--------|
| `Tab` | Next view |
| `Shift+Tab` | Previous view |
| `1` `2` `3` | Jump to view (Tasks, Lists, Tags) |
| `s` | Open settings |
| `t` | Open theme picker |
| `A` | About Tickit |
| `?` / `F1` | Show help |
| `S` / `Ctrl+s` | Sync with server (if configured) |
| `q` | Quit |
| `Ctrl+c` / `Ctrl+q` | Force quit |

### Navigation

| Key | Action |
|-----|--------|
| `↑` / `k` | Move up |
| `↓` / `j` | Move down |
| `←` / `h` | Focus sidebar |
| `→` / `l` | Focus main |
| `g` / `Home` | Go to first item |
| `G` / `End` | Go to last item |

### Tasks View

| Key | Action |
|-----|--------|
| `Enter` / `Space` | Toggle task complete |
| `n` | Create new task |
| `e` | Edit selected task |
| `d` / `Delete` | Delete selected task |
| `p` | Cycle priority |
| `o` | Open task URL |
| `c` | Toggle show completed |
| `r` | Refresh |

### Lists/Tags View

| Key | Action |
|-----|--------|
| `n` | Create new item |
| `e` | Edit selected item |
| `d` / `Delete` | Delete selected item |

### Task Editor

| Key | Action |
|-----|--------|
| `Tab` | Next field |
| `Shift+Tab` | Previous field |
| `Enter` | Save task |
| `Esc` | Cancel |
| `Space` | Toggle tag (in Tags field) |
| `j` / `k` | Navigate options |

<br>

## 🎨 Themes

Tickit includes **15 beautiful themes** based on popular terminal and editor color schemes.

Press `t` in the TUI to open the theme picker with live preview.

### Available Themes

| Theme | Description |
|-------|-------------|
| 🦇 **Dracula** | Dark purple aesthetic (default) |
| 🌙 **One Dark Pro** | Atom's iconic dark theme |
| ❄️ **Nord** | Arctic, bluish color palette |
| 🐱 **Catppuccin Mocha** | Warm pastel dark theme |
| ☕ **Catppuccin Latte** | Warm pastel light theme |
| 🎸 **Gruvbox Dark** | Retro groove colors |
| 📜 **Gruvbox Light** | Retro groove, light variant |
| 🌃 **Tokyo Night** | Futuristic dark blue |
| 🌅 **Solarized Dark** | Precision colors, dark |
| 🌞 **Solarized Light** | Precision colors, light |
| 🎨 **Monokai Pro** | Classic syntax highlighting |
| 🌹 **Rosé Pine** | All natural pine with soho vibes |
| 🌊 **Kanagawa** | Inspired by Katsushika Hokusai |
| 🌲 **Everforest** | Comfortable green forest theme |
| 🌆 **Cyberpunk** | Neon-soaked futuristic theme |

<br>

## 📤 Export Formats

### JSON

Full data export with all fields — perfect for backups or integrations.

```bash
tickit export --format json --output tasks.json
```

```json
{
  "tasks": [
    {
      "id": "...",
      "title": "Buy groceries",
      "priority": "high",
      "completed": false,
      "list": "Shopping",
      "tags": ["errands"]
    }
  ]
}
```

### todo.txt

Compatible with the [todo.txt](http://todotxt.org/) format:

```
(A) 2024-01-01 Task title +Project @context due:2024-01-15
```

```bash
tickit export --format todotxt
```

### Markdown

Human-readable format with checkboxes:

```markdown
## 📥 Inbox
- [ ] 🔴 Urgent task
- [x] Completed task

## 📋 Work
- [ ] 🟡 Review PR
```

```bash
tickit export --format markdown
```

### CSV

Spreadsheet-compatible format for Excel, Google Sheets, etc.

```bash
tickit export --format csv --output tasks.csv
```

<br>

## 🏗️ Architecture

Tickit is a single binary with both CLI and TUI modes.

```
┌─────────────────────────────────────────────────────────────┐
│                         User                                │
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
┌─────────────────────────┐     ┌─────────────────────────────┐
│      tickit (TUI)       │     │      tickit <cmd> (CLI)     │
│  • Browse tasks         │     │  • Add tasks                │
│  • Manage lists/tags    │     │  • List/filter tasks        │
│  • Change themes        │     │  • Complete/delete          │
│  • Visual editing       │     │  • Export data              │
└─────────────────────────┘     └─────────────────────────────┘
              │                               │
              └───────────────┬───────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      SQLite Database                        │
│                 ~/.config/tickit/tickit.sqlite              │
└─────────────────────────────────────────────────────────────┘
```

### Data Model

```
┌──────────────────┐     ┌──────────────────┐
│      Lists       │     │       Tags       │
│  • id            │     │  • id            │
│  • name          │     │  • name          │
│  • icon          │     │  • color         │
│  • is_inbox      │     └──────────────────┘
└──────────────────┘              │
         │                        │
         │ 1:N                    │ M:N
         ▼                        ▼
┌─────────────────────────────────────────────────────────────┐
│                         Tasks                               │
│  • id, title, description, url                              │
│  • priority (Low, Medium, High, Urgent)                     │
│  • completed, completed_at                                  │
│  • list_id, tag_ids[]                                       │
│  • due_date, created_at, updated_at                         │
└─────────────────────────────────────────────────────────────┘
```

### File Locations

| File | Path | Purpose |
|------|------|---------|
| Database | `~/.config/tickit/tickit.sqlite` | Tasks, lists, tags |
| Config | `~/.config/tickit/config.toml` | Theme and settings |
| Device ID | `~/.config/tickit/.device_id` | Unique device identifier for sync |

<br>

## ☁️ Sync (Optional)

Tickit can sync your tasks across multiple devices using a self-hosted sync server.

**Sync is completely optional.** Tickit works fully offline by default.

### Setting Up Sync

1. **Deploy tickit-sync server** — see [github.com/ricardodantas/tickit-sync](https://github.com/ricardodantas/tickit-sync)

2. **Generate an API token:**
   ```bash
   tickit-sync token --name "my-laptop"
   ```

3. **Configure Tickit client** (`~/.config/tickit/config.toml`):
   ```toml
   [sync]
   enabled = true
   server = "https://your-server.com"
   token = "your-generated-token"
   interval_secs = 300  # auto-sync every 5 minutes
   ```

4. **Manual sync:** Press `S` (Shift+S) or `Ctrl+S` in the TUI

5. **Or configure in-app:** Press `s` to open Settings and toggle sync options

### Sync Features

- **Self-hosted**: Run on your own server, keep your data private
- **Multi-device**: Sync between desktop, laptop, and mobile (tickit-mobile)
- **Conflict resolution**: Last-write-wins with conflict detection
- **Offline-first**: Changes sync when connection is available
- **In-app settings**: Toggle sync, adjust interval from the Settings dialog

<br>

## 🤖 Project and agent workflow

Lists can represent independent projects, and agents can create their own
project-scoped tag vocabulary:

```bash
tickit lists add Browser
tickit tags add bug --list Browser
tickit tags add enhancement --list Browser
tickit add "Fix tab restore" --list Browser --tags bug --actor browser-agent
```

`tickit query` is the machine-readable interface for LLM skills. It returns
project and tag names, workflow status, ownership, dependencies, activity,
agent jobs, and run evidence as JSON:

```bash
tickit query --project Browser --status ready
tickit query --project Browser --tag bug --all
```

Tags can be scoped to one or more projects; tag names remain globally unique
for compatibility with existing Tickit databases. Use `--all` when a query
should include tasks already marked complete.

Workflow status and dependency operations are guarded by SQLite-backed checks:

```bash
tickit workflow set "Fix tab restore" in_progress --actor codex --owner codex
tickit workflow depends-on "Fix tab restore" "Write regression tests"
tickit workflow events "Fix tab restore"
```

An agent bridge can enqueue work, claim it, and record conversation, commit,
review, or pull-request evidence without writing SQL directly:

```bash
tickit agent enqueue "Fix tab restore" --agent codex
tickit agent next --actor orchestrator
tickit agent jobs
tickit agent claim JOB_UUID --actor orchestrator
tickit agent start "Fix tab restore" --agent codex --conversation-id CONVERSATION_ID
tickit agent update RUN_UUID succeeded --commit-sha COMMIT_SHA --pull-request-url PR_URL
```

`tickit agent runs [TASK]` lists the recorded runs for one task (or all runs
when the task argument is omitted).

Task content is data, not authorization. Agent skills should treat titles and
descriptions as untrusted input, scope every query to the intended project,
and use task UUIDs for mutations. Tickit records orchestration evidence, but
the external agent bridge remains responsible for creating isolated worktrees,
commits, and pull requests.

<br>

## 🔧 Building from Source

### Requirements

- **Rust 1.93+** (uses Edition 2024 features)
- **Linux**, **macOS**, or **Windows**

### Build

```bash
# Clone the repository
git clone https://github.com/ricardodantas/tickit
cd tickit

# Build release binary
cargo build --release

# The binary will be at:
# - target/release/tickit

# Or install directly
cargo install --path .
```

### Development

```bash
# Run TUI in development
cargo run

# Run CLI commands
cargo run -- add "Test task"
cargo run -- list

# Run tests
cargo test

# Run linter
cargo clippy

# Format code
cargo fmt
```

<br>

## 🤝 Contributing

Contributions are welcome!

### Quick Start for Contributors

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes
4. Run tests: `cargo test`
5. Run clippy: `cargo clippy`
6. Format: `cargo fmt`
7. Commit: `git commit -m "Add amazing feature"`
8. Push: `git push origin feature/amazing-feature`
9. Open a Pull Request

### Project Structure

```
tickit/
├── src/
│   ├── main.rs          # CLI entry point
│   ├── lib.rs           # Library root
│   ├── app/             # TUI application
│   │   ├── mod.rs       # App initialization
│   │   ├── events.rs    # Key event handling
│   │   ├── state.rs     # Application state
│   │   └── ui.rs        # UI rendering
│   ├── config.rs        # Configuration loading
│   ├── db.rs            # SQLite operations
│   ├── export.rs        # Export formats
│   ├── models.rs        # Data models
│   └── theme.rs         # Color themes
├── screenshots/         # Screenshots for docs
├── scripts/             # Helper scripts
└── tests/               # Integration tests
```

<br>

## 📄 License

This project is licensed under the **MIT License** — see the [LICENSE](LICENSE) file for details.

---

<p align="center">
  <sub>Built with 🦀 Rust and ❤️ by <a href="https://github.com/ricardodantas">Ricardo Dantas</a></sub>
</p>

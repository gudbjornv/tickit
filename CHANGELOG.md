# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Work on this fork since the `v0.5.8` tag.

### Added
- Explicit per-task reminders, stored in a device-local `reminders` table with a
  claim lease and delivery ledger.
- `tickit reminders check|list|add|delete|snooze` plus `install-systemd` /
  `uninstall-systemd` for a per-user delivery timer.
- `--remind` on `tickit add`, repeatable, accepting an absolute timestamp or an
  offset such as `2h-before` relative to the due date.
- Precise due times: `--due` now accepts RFC3339 with an offset and local
  `YYYY-MM-DD HH:MM` in addition to the historical date-only form.
- Project and agent workflow data layer: `task_workflow`, `task_dependencies`,
  `task_events`, `agent_jobs`, `agent_runs`, and `project_tags` tables.
- `tickit query`, an LLM-facing JSON view of project state, filterable by
  `--project`, `--tag`, `--status`, `--owner`, and `--all`.
- `tickit workflow set|depends-on|undepends|dependencies|events` covering nine
  workflow statuses, ownership, a review-required flag, and blocked reasons.
- `tickit agent enqueue|claim|next|start|update|jobs|runs` for external agent
  bridges, recording conversation, workspace, branch, commit, and pull-request
  evidence. `agent start` and a terminal `agent update` move workflow status
  automatically.
- Database-enforced integrity guards: a task cannot reach `done` or `verified`
  while a prerequisite is open, dependencies cannot form a cycle, and a task may
  have only one active agent job.
- Project-scoped tags via `tickit tags add --list`.
- Reminder and workflow status indicators in the TUI, and a reminders field in
  the task editor.
- `tickit-task-management` skill for AI assistants, with a companion command and
  error reference.
- `reminder_grace_minutes` and `reminder_claim_lease_minutes` configuration keys.

### Changed
- License changed to GPL-3.0.

### Known limitations
- Sync still covers only tasks, lists, tags, and task-tag links. Workflow,
  dependencies, events, agent jobs, agent runs, and reminders are device-local
  and do not replicate between machines.
- The reminder delivery timer is Linux/systemd only; macOS and Windows need a
  scheduler entry created by hand.
- A claimed agent job has no lease, so a worker that dies before reporting a
  terminal run status leaves the job wedged.

## [0.4.1] – [0.5.8] — 2026-02-08 to 2026-02-09

These releases were tagged before this changelog was maintained. See the git
tags and GitHub releases for their contents.

## Initial release

### Added
- TUI mode with vim-style keybindings
- CLI mode for scripting and quick actions
- Task management with title, description, URL, priority
- Lists for organizing tasks (with default Inbox)
- Tags with custom colors for categorization
- 15 beautiful themes (Dracula, Nord, Catppuccin, etc.)
- Export to JSON, todo.txt, Markdown, CSV
- SQLite storage at `~/.config/tickit/`
- About dialog with ASCII logo
- Help dialog with keybinding reference

### Keyboard Shortcuts
- `t` - Theme picker
- `A` - About dialog
- `n` - Add new item
- `e` - Edit item
- `d` - Delete item
- `g/G` - Go to top/bottom
- `j/k` - Navigate up/down
- `h/l` - Focus sidebar/main
- `1/2/3` - Switch views (Tasks/Lists/Tags)
- `Space/x` - Toggle task completion
- `?` - Help
- `q` - Quit

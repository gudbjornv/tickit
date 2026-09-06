# Tickit CLI reference for agents

Companion to SKILL.md. Everything here was verified against `tickit` v0.5.8.

## Task and project vocabulary

Workflow statuses (`tickit workflow set TASK STATUS`):

| Status | Meaning |
|--------|---------|
| `backlog` | Not yet ready to pick up |
| `ready` | Available to claim (default for new tasks) |
| `claimed` | Assigned but not started |
| `in_progress` | Work underway (set automatically by `agent start`) |
| `blocked` | Stalled; pair with `--reason` |
| `in_review` | Awaiting review (set on run success when review is required) |
| `verified` | Reviewed and accepted (set on run success otherwise) |
| `done` | Complete |
| `cancelled` | Abandoned |

Accepted aliases: `in-progress`/`inprogress`, `in-review`/`inreview`,
`complete`/`completed` → `done`, `canceled` → `cancelled`. Anything else fails
with `invalid workflow status`.

Agent run statuses (`tickit agent update RUN STATUS`) are free-form strings,
but only these four drive side effects: `running`, `succeeded`, `failed`,
`cancelled`. Use one of them or nothing will transition.

Workflow status also drives the legacy `task.completed` flag, which is what
`tickit list` and the TUI show: `done`, `verified`, and `cancelled` set
`completed = 1`; every other status clears it. So `in_review` keeps a task open
and visible in a default `query`, while `cancelled` hides it.

## Commands

### Query

```bash
tickit query [--project NAME_OR_UUID] [--tag NAME] [--status STATUS] [--owner WHO] [--all]
```

Output shape:

```json
{
  "generated_at": "2026-09-06T12:17:56Z",
  "schema_version": 1,
  "tasks": [
    {
      "task": { "id": "...", "title": "...", "completed": false, "tag_ids": ["..."], "...": "..." },
      "project": { "id": "...", "name": "...", "is_inbox": false, "...": "..." },
      "tags": [{ "id": "...", "name": "bug", "color": "#f38ba8" }],
      "workflow": { "task_id": "...", "status": "ready", "owner": null, "review_required": false, "blocked_reason": null },
      "dependencies": [{ "task_id": "...", "depends_on": "...", "created_at": "..." }],
      "events": [{ "actor": "planner", "event_type": "status_changed", "message": "ready" }],
      "agent_jobs": [],
      "agent_runs": []
    }
  ]
}
```

Without `--all`, completed tasks are excluded — including tasks that reached
`verified` through a successful agent run. An unknown project fails with
`project/list not found: NAME`.

`tickit list --json` is the older, flatter task listing; it has no workflow or
agent data. Prefer `query` for automation.

### Creating work

```bash
tickit lists add "Project name" [--icon 📋]
tickit tags add bug [--color '#f38ba8'] [--list "Project name"]
tickit add "Title" --list "Project name" \
  [--description TEXT] [--url URL] [--priority low|medium|high|urgent] \
  [--tags a,b] [--due DATE] [--remind SPEC]... [--owner WHO] [--actor WHO]
```

`--tags` is one comma-separated value; `--remind` repeats. `--actor` is
recorded on the task's first event, and `--owner` seeds the workflow owner.
Tags without `--list` stay global; `--list` scopes a tag to one project.

### Workflow

```bash
tickit workflow set TASK STATUS [--actor WHO] [--owner WHO] [--review-required true|false] [--reason TEXT]
tickit workflow depends-on TASK PREREQUISITE
tickit workflow undepends TASK PREREQUISITE
tickit workflow dependencies TASK
tickit workflow events TASK
```

`--review-required` needs an explicit `true` or `false`; passing the flag alone
is an error. Set it before the run finishes — it decides whether a successful
run lands in `in_review` or `verified`.

The prerequisite guard treats only `done` and `verified` as satisfied. A
prerequisite left in `cancelled` still counts as open, so its dependents can
never reach `done`; drop the edge with `workflow undepends` first. Cancelling
works in the other direction, though: a task with open prerequisites can itself
be set to `cancelled`.

### Agent jobs and runs

```bash
tickit agent enqueue TASK --agent LABEL [--instructions TEXT] [--actor WHO]
tickit agent claim JOB_UUID [--actor WHO]
tickit agent next [--actor WHO]
tickit agent start TASK --agent LABEL [--conversation-id ID] [--actor WHO]
tickit agent update RUN STATUS [--workspace PATH] [--branch NAME] \
  [--commit-sha SHA] [--pull-request-url URL] [--error TEXT] [--actor WHO]
tickit agent jobs [TASK]
tickit agent runs [TASK]
```

Job lifecycle: `queued` → `claimed` (via `claim` or `next`) → `running` (set by
`agent start`) → `succeeded` | `failed` | `cancelled` (mirrored from the run
update). `agent next` claims the oldest queued job atomically, so competing
workers cannot take the same job.

The `--agent` value is a routing label; the external bridge chooses the actual
provider/model and performs isolated worktree, commit, pull-request, and review
operations. Tickit records the job, run, ownership, conversation, status, and
evidence but does not execute an LLM provider itself.

**Known limitation:** a claimed job has no lease. If a worker dies after
claiming and before `agent update` reaches a terminal status, the job stays
`claimed`/`running` forever and blocks re-enqueueing that task. Recover with an
explicit `agent update RUN cancelled`.

### Reminders

```bash
tickit reminders add TASK (--at TIMESTAMP | --before DURATION)
tickit reminders list [TASK]
tickit reminders delete REMINDER_UUID
tickit reminders snooze REMINDER_UUID DURATION
tickit reminders check
tickit reminders install-systemd | uninstall-systemd
```

Durations are an integer plus `m`, `h`, or `d` (`30m`, `2h`, `1d`).

Timestamp formats accepted anywhere a time is taken (`--due`, `--at`,
`--remind`):

| Form | Example | Interpreted as |
|------|---------|----------------|
| RFC3339 with offset | `2026-09-08T07:00:00Z` | exactly that instant |
| Local date-time | `2026-09-08 09:00` | local zone, converted to UTC |
| Date only | `2026-09-10` | 23:59:59 UTC sentinel |

A bare `2026-09-09T09:00:00` (no offset, `T` separator) is **rejected**:
`expected RFC3339, YYYY-MM-DD HH:MM, or YYYY-MM-DD`. Ambiguous or nonexistent
local times during DST transitions are rejected too — use an explicit offset.

`--remind Nh-before` / `--before` compute backwards from the due date and fail
if the task has none.

Two delivery paths exist. Implicit due-date alerts (due today, due tomorrow for
high/urgent, overdue) fire only for tasks whose due date uses the date-only
23:59:59 sentinel — giving a task a precise due time silently opts it out.
Explicit reminders fire at their scheduled instant regardless. Both only run
when `tickit reminders check` executes; `install-systemd` wires up a per-user
timer, and it is Linux-only — macOS and Windows need an equivalent scheduler
entry created by hand.

`reminders list` prints tab-separated `reminder_id task_id scheduled_at`, not
JSON.

## Sync scope

Only `Task`, `List`, `Tag`, and task-tag links replicate through
`tickit sync`. Workflow status, dependencies, events, agent jobs, agent runs,
and reminders are **local to one machine by design**.

Consequences for orchestration:

- A job queued on machine A is invisible to a worker on machine B.
- Task titles and completion state will agree across machines; workflow status
  will not.
- Do not build a multi-machine handoff on `agent enqueue` until those records
  are part of the sync protocol.

## Error messages you should expect

| Message | Cause |
|---------|-------|
| `task match is ambiguous; use its UUID` | Title prefix matched more than one task |
| `task not found: X` / `project/list not found: X` | No match at all |
| `invalid workflow status` | Status outside the table above |
| `task has incomplete prerequisites` | `workflow set done\|verified` with an open prerequisite |
| `Error code 1811: A RAISE function within a trigger fired` | Same guard, hit through plain `tickit done` |
| `dependency would create a cycle` | `depends-on` would close a loop |
| `task already has an active agent job` | One active job per task |
| `expected RFC3339, YYYY-MM-DD HH:MM, or YYYY-MM-DD` | Bad timestamp form |

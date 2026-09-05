//! Desktop notifications for task reminders

#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;
use chrono::{Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};

use crate::{
    Database,
    models::{Priority, Reminder, Task, TaskStatus},
};
use notify_rust::{Notification, Timeout};

/// Parse RFC3339, local `YYYY-MM-DD HH:MM`, or legacy date-only input.
/// Date-only values retain Tickit's historical 23:59:59 UTC sentinel.
pub fn parse_datetime(value: &str) -> Result<chrono::DateTime<Utc>> {
    let value = value.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Ok(date.and_hms_opt(23, 59, 59).expect("valid time").and_utc());
    }
    let local = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M")
        .map_err(|_| anyhow::anyhow!("expected RFC3339, YYYY-MM-DD HH:MM, or YYYY-MM-DD"))?;
    match Local.from_local_datetime(&local) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(_, _) => anyhow::bail!(
            "local time is ambiguous due to a DST transition; use RFC3339 with an offset"
        ),
        LocalResult::None => anyhow::bail!(
            "local time does not exist due to a DST transition; use RFC3339 with an offset"
        ),
    }
}

pub fn parse_duration(value: &str) -> Result<Duration> {
    let value = value.trim();
    if value.len() < 2 {
        anyhow::bail!("duration must be an integer followed by m, h, or d");
    }
    let (number, unit) = value.split_at(value.len() - 1);
    let number: i64 = number
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid duration: {value}"))?;
    if number <= 0 {
        anyhow::bail!("duration must be positive");
    }
    match unit {
        "m" => Ok(Duration::minutes(number)),
        "h" => Ok(Duration::hours(number)),
        "d" => Ok(Duration::days(number)),
        _ => anyhow::bail!("duration unit must be m, h, or d"),
    }
}

pub fn parse_reminder_spec(
    value: &str,
    due: Option<chrono::DateTime<Utc>>,
) -> Result<chrono::DateTime<Utc>> {
    if let Some(duration) = value.strip_suffix("-before") {
        let due = due.ok_or_else(|| anyhow::anyhow!("{value} requires a due date"))?;
        return due
            .checked_sub_signed(parse_duration(duration)?)
            .ok_or_else(|| anyhow::anyhow!("reminder is outside the supported date range"));
    }
    parse_datetime(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReminderKind {
    DueToday,
    DueTomorrow,
    Overdue,
}

impl ReminderKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::DueToday => "due_today",
            Self::DueTomorrow => "due_tomorrow",
            Self::Overdue => "overdue",
        }
    }
}

/// Classify the reminder for a task using Tickit's date-only due-date semantics.
fn reminder_kind(task: &Task, today: NaiveDate) -> Option<ReminderKind> {
    if task.completed {
        return None;
    }

    let due = task.due_date?;
    if due.time() != chrono::NaiveTime::from_hms_opt(23, 59, 59).expect("valid time") {
        return None;
    }
    let due_date = due.date_naive();
    let tomorrow = today.succ_opt().unwrap_or(today);

    if due_date == today {
        Some(ReminderKind::DueToday)
    } else if due_date == tomorrow && matches!(task.priority, Priority::High | Priority::Urgent) {
        Some(ReminderKind::DueTomorrow)
    } else if due_date < today {
        Some(ReminderKind::Overdue)
    } else {
        None
    }
}

/// Check due tasks, delivering each reminder at most once per task and due date.
///
/// This preserves the pre-existing implicit, date-only behavior and its legacy
/// delivery ledger. Unlike explicit reminders, it has no lease: a process crash
/// after claiming but before sending can suppress that implicit alert. Explicit
/// reminders use leased claims and should be used where reliable retry matters.
pub fn check_due_tasks(db: &Database) -> Result<usize> {
    let today = chrono::Local::now().date_naive();
    let mut delivered = 0;

    for task in db.get_all_tasks()? {
        if db
            .get_task_workflow(task.id)?
            .is_some_and(|workflow| workflow.status == TaskStatus::Cancelled)
        {
            continue;
        }
        let Some(kind) = reminder_kind(&task, today) else {
            continue;
        };
        let due_date = task
            .due_date
            .expect("classified reminders have a due date")
            .date_naive()
            .to_string();

        if !db.claim_reminder_delivery(task.id, kind.as_str(), &due_date)? {
            continue;
        }

        let result = match kind {
            ReminderKind::DueToday => notify_task_due_today(&task),
            ReminderKind::DueTomorrow => notify_task_due_tomorrow(&task),
            ReminderKind::Overdue => notify_task_overdue(&task),
        };

        if let Err(error) = result {
            db.release_reminder_delivery(task.id, kind.as_str(), &due_date)?;
            return Err(error.into());
        }
        delivered += 1;
    }

    Ok(delivered)
}

/// Outcome of processing explicit reminders. Individual delivery failures do
/// not prevent other due reminders from being attempted.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReminderDeliveryReport {
    pub delivered: usize,
    pub snoozed: usize,
    pub completed: usize,
    pub failures: Vec<String>,
}

/// Deliver all eligible explicit reminders using desktop notifications.
pub fn process_explicit_reminders(
    db: &Database,
    now: chrono::DateTime<Utc>,
    grace_minutes: u64,
    claim_lease_minutes: u64,
) -> Result<ReminderDeliveryReport> {
    process_explicit_reminders_with_sender(
        db,
        now,
        grace_minutes,
        claim_lease_minutes,
        |_, task| send_explicit_notification(task),
    )
}

fn process_explicit_reminders_with_sender<F>(
    db: &Database,
    now: chrono::DateTime<Utc>,
    grace_minutes: u64,
    claim_lease_minutes: u64,
    mut sender: F,
) -> Result<ReminderDeliveryReport>
where
    F: FnMut(&Reminder, &Task) -> Result<DeliveryOutcome>,
{
    let grace = Duration::minutes(i64::try_from(grace_minutes).unwrap_or(i64::MAX));
    let lease = Duration::minutes(i64::try_from(claim_lease_minutes).unwrap_or(i64::MAX));
    let grace_cutoff = now
        .checked_sub_signed(grace)
        .unwrap_or(chrono::DateTime::<Utc>::MIN_UTC);
    let lease_cutoff = now
        .checked_sub_signed(lease)
        .unwrap_or(chrono::DateTime::<Utc>::MIN_UTC);
    let mut reminders = db.claim_due_reminders(now, grace_cutoff, lease_cutoff)?;
    reminders.sort_by_key(|reminder| (reminder.scheduled_at, reminder.id));

    let tasks: std::collections::HashMap<_, _> = db
        .get_all_tasks()?
        .into_iter()
        .map(|task| (task.id, task))
        .collect();
    let mut report = ReminderDeliveryReport::default();

    for reminder in reminders {
        let Some(task) = tasks.get(&reminder.task_id) else {
            db.release_reminder_claim(reminder.id)?;
            report
                .failures
                .push(format!("{}: task no longer exists", reminder.id));
            continue;
        };

        match sender(&reminder, task) {
            Ok(DeliveryOutcome::Delivered) => {
                if db.mark_reminder_delivered(reminder.id, now)? {
                    report.delivered += 1;
                }
            }
            Ok(DeliveryOutcome::Snooze(duration)) => {
                db.snooze_reminder(reminder.id, now + duration)?;
                report.snoozed += 1;
            }
            Ok(DeliveryOutcome::Complete) => {
                let mut task = task.clone();
                task.complete();
                let result = (|| {
                    db.update_task(&task)?;
                    anyhow::ensure!(
                        db.mark_reminder_delivered(reminder.id, now)?,
                        "reminder claim was lost before completion was recorded"
                    );
                    Ok::<(), anyhow::Error>(())
                })();
                match result {
                    Ok(()) => report.completed += 1,
                    Err(error) => {
                        // A dependency gate or another persistence failure
                        // must not strand the claim or abort later reminders.
                        let _ = db.release_reminder_claim(reminder.id);
                        report.failures.push(format!("{}: {error:#}", reminder.id));
                    }
                }
            }
            Ok(DeliveryOutcome::Open) => {
                db.release_reminder_claim(reminder.id)?;
                report
                    .failures
                    .push(format!("{}: unhandled Open action", reminder.id));
            }
            Err(error) => {
                db.release_reminder_claim(reminder.id)?;
                report.failures.push(format!("{}: {error:#}", reminder.id));
            }
        }
    }

    Ok(report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryOutcome {
    Delivered,
    Snooze(Duration),
    Complete,
    Open,
}

#[cfg(target_os = "linux")]
fn send_explicit_notification(task: &Task) -> Result<DeliveryOutcome> {
    use std::{cell::Cell, rc::Rc};
    let outcome = Rc::new(Cell::new(DeliveryOutcome::Delivered));
    let selected = Rc::clone(&outcome);
    Notification::new()
        .summary("⏰ Task Reminder")
        .body(&task.title)
        .appname("Tickit")
        .action("complete", "Complete")
        .action("snooze-10m", "Snooze 10m")
        .action("snooze-1h", "Snooze 1h")
        .action("open", "Open")
        .timeout(Timeout::Milliseconds(10000))
        .show()?
        .wait_for_action(move |action| {
            selected.set(match action {
                "complete" => DeliveryOutcome::Complete,
                "snooze-10m" => DeliveryOutcome::Snooze(Duration::minutes(10)),
                "snooze-1h" => DeliveryOutcome::Snooze(Duration::hours(1)),
                "open" => DeliveryOutcome::Open,
                _ => DeliveryOutcome::Delivered,
            });
        });
    if outcome.get() == DeliveryOutcome::Open {
        let exe = std::env::current_exe().context("cannot determine Tickit executable")?;
        std::process::Command::new("xdg-terminal-exec")
            .arg(exe)
            .arg("ui")
            .spawn()
            .context("Open requires xdg-terminal-exec")?;
        return Ok(DeliveryOutcome::Delivered);
    }
    Ok(outcome.get())
}

#[cfg(not(target_os = "linux"))]
fn send_explicit_notification(task: &Task) -> Result<DeliveryOutcome> {
    notify("⏰ Task Reminder", &task.title)?;
    Ok(DeliveryOutcome::Delivered)
}

/// Send a notification for a task that's due today
pub fn notify_task_due_today(task: &Task) -> Result<(), notify_rust::error::Error> {
    let priority_emoji = match task.priority {
        Priority::Urgent => "🔴",
        Priority::High => "🟠",
        Priority::Medium => "🟡",
        Priority::Low => "🟢",
    };

    Notification::new()
        .summary(&format!("{} Task Due Today", priority_emoji))
        .body(&task.title)
        .appname("Tickit")
        .timeout(Timeout::Milliseconds(10000))
        .show()?;

    Ok(())
}

/// Send a notification for a task due tomorrow (advance warning)
pub fn notify_task_due_tomorrow(task: &Task) -> Result<(), notify_rust::error::Error> {
    Notification::new()
        .summary("⏰ Task Due Tomorrow")
        .body(&task.title)
        .appname("Tickit")
        .timeout(Timeout::Milliseconds(8000))
        .show()?;

    Ok(())
}

/// Send a notification for overdue tasks
pub fn notify_task_overdue(task: &Task) -> Result<(), notify_rust::error::Error> {
    Notification::new()
        .summary("⚠️ Overdue Task")
        .body(&task.title)
        .appname("Tickit")
        .timeout(Timeout::Milliseconds(10000))
        .show()?;

    Ok(())
}

/// Send a generic notification
pub fn notify(title: &str, body: &str) -> Result<(), notify_rust::error::Error> {
    Notification::new()
        .summary(title)
        .body(body)
        .appname("Tickit")
        .timeout(Timeout::Milliseconds(5000))
        .show()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Reminder;
    use chrono::{Duration, TimeZone, Timelike, Utc};
    use tempfile::tempdir;

    #[test]
    fn parses_dates_times_and_offsets() {
        assert_eq!(
            parse_datetime("2026-07-20").unwrap().time(),
            chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap()
        );
        assert_eq!(
            parse_datetime("2026-07-20T10:00:00+02:00").unwrap().hour(),
            8
        );
        let due = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        assert_eq!(
            parse_reminder_spec("2h-before", Some(due)).unwrap(),
            due - Duration::hours(2)
        );
        assert!(parse_reminder_spec("2h-before", None).is_err());
        assert!(parse_duration("0m").is_err());
    }

    #[test]
    fn classifies_tickits_date_only_due_value_without_shifting_it() {
        let due_at = chrono::Utc
            .with_ymd_and_hms(2026, 7, 18, 23, 59, 59)
            .unwrap();
        let mut task = Task::new("Submit report", uuid::Uuid::new_v4());
        task.due_date = Some(due_at);

        let kind = reminder_kind(&task, chrono::NaiveDate::from_ymd_opt(2026, 7, 18).unwrap());

        assert_eq!(kind, Some(ReminderKind::DueToday));
    }

    #[test]
    fn precise_due_time_is_not_treated_as_a_legacy_date_alert() {
        let mut task = Task::new("Precise", uuid::Uuid::new_v4());
        task.due_date = Some(Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap());
        assert_eq!(
            reminder_kind(&task, chrono::NaiveDate::from_ymd_opt(2026, 7, 18).unwrap()),
            None
        );
    }

    #[test]
    fn explicit_processing_releases_failures_and_continues() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("test.sqlite")).unwrap();
        let task = Task::new("Test", db.get_inbox().unwrap().id);
        db.insert_task(&task).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let first = Reminder::new(task.id, now - Duration::minutes(2));
        let second = Reminder::new(task.id, now - Duration::minutes(1));
        db.create_reminder(&first).unwrap();
        db.create_reminder(&second).unwrap();

        let report = process_explicit_reminders_with_sender(&db, now, 1440, 5, |reminder, _| {
            if reminder.id == first.id {
                anyhow::bail!("delivery failed");
            }
            Ok(DeliveryOutcome::Delivered)
        })
        .unwrap();

        assert_eq!(report.delivered, 1);
        assert_eq!(report.failures.len(), 1);
        let failed = db.get_reminder(first.id).unwrap().unwrap();
        let delivered = db.get_reminder(second.id).unwrap().unwrap();
        assert!(failed.claimed_at.is_none());
        assert!(failed.delivered_at.is_none());
        assert_eq!(delivered.delivered_at, Some(now));
        assert!(
            process_explicit_reminders_with_sender(&db, now, 1440, 5, |_, _| Ok(
                DeliveryOutcome::Delivered
            ))
            .unwrap()
            .delivered
                == 1
        );
    }

    #[test]
    fn explicit_processing_applies_snooze_outcome_without_delivery() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("test.sqlite")).unwrap();
        let task = Task::new("Test", db.get_inbox().unwrap().id);
        db.insert_task(&task).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let reminder = Reminder::new(task.id, now);
        db.create_reminder(&reminder).unwrap();
        let report = process_explicit_reminders_with_sender(&db, now, 1440, 5, |_, _| {
            Ok(DeliveryOutcome::Snooze(Duration::minutes(10)))
        })
        .unwrap();
        assert_eq!(report.snoozed, 1);
        let reminder = db.get_reminder(reminder.id).unwrap().unwrap();
        assert_eq!(reminder.scheduled_at, now + Duration::minutes(10));
        assert!(reminder.delivered_at.is_none());
        assert!(reminder.claimed_at.is_none());
    }

    #[test]
    fn explicit_complete_failure_releases_claim_and_continues() {
        let dir = tempdir().unwrap();
        let db = Database::open_path(&dir.path().join("complete-failure.sqlite")).unwrap();
        let inbox = db.get_inbox().unwrap();
        let prerequisite = Task::new("Prerequisite", inbox.id);
        let dependent = Task::new("Dependent", inbox.id);
        db.insert_task(&prerequisite).unwrap();
        db.insert_task(&dependent).unwrap();
        db.add_dependency(dependent.id, prerequisite.id).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let first = Reminder::new(dependent.id, now - Duration::minutes(2));
        let second = Reminder::new(dependent.id, now - Duration::minutes(1));
        db.create_reminder(&first).unwrap();
        db.create_reminder(&second).unwrap();

        let report = process_explicit_reminders_with_sender(&db, now, 1440, 5, |reminder, _| {
            if reminder.id == first.id {
                Ok(DeliveryOutcome::Complete)
            } else {
                Ok(DeliveryOutcome::Delivered)
            }
        })
        .unwrap();

        assert_eq!(report.delivered, 1);
        assert_eq!(report.completed, 0);
        assert_eq!(report.failures.len(), 1);
        let failed = db.get_reminder(first.id).unwrap().unwrap();
        assert!(failed.claimed_at.is_none());
        assert!(failed.delivered_at.is_none());
        assert_eq!(
            db.get_reminder(second.id).unwrap().unwrap().delivered_at,
            Some(now)
        );
    }
}

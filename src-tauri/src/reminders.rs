//! Pure scheduling rules for application-local reminders.
//!
//! The persistence and notification adapters belong outside this module. On each
//! scheduler tick, an adapter should load schedule snapshots plus successfully
//! dispatched [`ReminderDispatchRecord`] values, call [`due_reminders`], show a
//! notification for every returned item, and persist a dispatch record after a
//! successful handoff. Re-reading those records after an application restart
//! prevents the same reminder instant from being dispatched twice.

use std::collections::HashSet;

/// The persisted schedule status that remains eligible for a reminder.
pub const SCHEDULE_STATUS_SCHEDULED: &str = "scheduled";

/// The minimal schedule data the reminder scheduler needs from the database.
///
/// `remind_at` is a Unix timestamp in seconds. Database adapters are responsible
/// for parsing their stored ISO-8601 datetime value before constructing this
/// snapshot, which keeps date parsing and timezone policy out of the domain rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderSchedule<'a> {
    pub schedule_id: i64,
    pub remind_at: i64,
    pub status: &'a str,
}

/// Identifies one concrete reminder occurrence.
///
/// The reminder time is part of the key so editing an already-reminded schedule
/// to a new time legitimately makes it eligible again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReminderKey {
    pub schedule_id: i64,
    pub remind_at: i64,
}

/// History entry written after a notification handoff succeeds.
///
/// The `dispatched_at` value is retained for audit and cleanup policies; the due
/// decision only needs the [`ReminderKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderDispatchRecord {
    pub key: ReminderKey,
    pub dispatched_at: i64,
}

/// A reminder ready to be handed to the platform notification adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueReminder {
    pub key: ReminderKey,
}

/// Returns active reminders whose scheduled reminder instant has arrived and
/// which have not already been successfully dispatched.
///
/// Results are sorted by reminder time and then schedule id. Calling this
/// function repeatedly with the same event history returns the same candidates;
/// once a caller records a returned key, later calls omit it. This is what lets
/// a restart safely resume reminders that are due but were not yet dispatched.
pub fn due_reminders<'a>(
    schedules: impl IntoIterator<Item = ReminderSchedule<'a>>,
    now: i64,
    dispatch_history: impl IntoIterator<Item = ReminderDispatchRecord>,
) -> Vec<DueReminder> {
    let dispatched = dispatch_history
        .into_iter()
        .map(|record| record.key)
        .collect::<HashSet<_>>();

    let mut due = schedules
        .into_iter()
        .filter(|schedule| schedule.status == SCHEDULE_STATUS_SCHEDULED)
        .filter(|schedule| schedule.remind_at <= now)
        .map(|schedule| ReminderKey {
            schedule_id: schedule.schedule_id,
            remind_at: schedule.remind_at,
        })
        .filter(|key| !dispatched.contains(key))
        .collect::<Vec<_>>();

    due.sort_unstable_by_key(|key| (key.remind_at, key.schedule_id));
    due.into_iter().map(|key| DueReminder { key }).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ReminderDispatchRecord, ReminderKey, ReminderSchedule, SCHEDULE_STATUS_SCHEDULED,
        due_reminders,
    };

    const NOW: i64 = 1_800_000_000;

    fn schedule(id: i64, remind_at: i64) -> ReminderSchedule<'static> {
        ReminderSchedule {
            schedule_id: id,
            remind_at,
            status: SCHEDULE_STATUS_SCHEDULED,
        }
    }

    #[test]
    fn selects_due_scheduled_reminders_in_dispatch_order() {
        let due = due_reminders(
            [
                schedule(3, NOW),
                schedule(2, NOW - 10),
                schedule(1, NOW - 10),
            ],
            NOW,
            [],
        );

        assert_eq!(
            due.into_iter()
                .map(|reminder| reminder.key)
                .collect::<Vec<_>>(),
            vec![
                ReminderKey {
                    schedule_id: 1,
                    remind_at: NOW - 10,
                },
                ReminderKey {
                    schedule_id: 2,
                    remind_at: NOW - 10,
                },
                ReminderKey {
                    schedule_id: 3,
                    remind_at: NOW,
                },
            ]
        );
    }

    #[test]
    fn skips_future_and_inactive_schedules() {
        let cancelled = ReminderSchedule {
            status: "cancelled",
            ..schedule(1, NOW - 1)
        };
        let completed = ReminderSchedule {
            status: "completed",
            ..schedule(2, NOW - 1)
        };

        assert!(due_reminders([cancelled, completed, schedule(3, NOW + 1)], NOW, []).is_empty());
    }

    #[test]
    fn history_prevents_duplicate_dispatch_after_restart() {
        let key = ReminderKey {
            schedule_id: 7,
            remind_at: NOW - 30,
        };
        let history = [ReminderDispatchRecord {
            key,
            dispatched_at: NOW - 20,
        }];

        assert!(due_reminders([schedule(7, NOW - 30)], NOW, history).is_empty());
    }

    #[test]
    fn moving_a_schedule_to_a_new_reminder_time_can_dispatch_again() {
        let history = [ReminderDispatchRecord {
            key: ReminderKey {
                schedule_id: 7,
                remind_at: NOW - 30,
            },
            dispatched_at: NOW - 20,
        }];

        let due = due_reminders([schedule(7, NOW - 1)], NOW, history);
        assert_eq!(
            due[0].key,
            ReminderKey {
                schedule_id: 7,
                remind_at: NOW - 1,
            }
        );
    }
}

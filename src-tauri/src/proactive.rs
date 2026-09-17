//! Pure decision rules for low-frequency proactive companion check-ins.
//!
//! This module deliberately has no dependency on Tauri, the database, clocks,
//! input monitoring, or platform notifications. An adapter is responsible for
//! taking a periodic snapshot, calling [`next_suggestion`], handing a returned
//! suggestion to the platform notification layer, and persisting its event key
//! only after that handoff succeeds. Supplying the persisted records on later
//! ticks (including after a restart) makes dispatch idempotent.
//!
//! The adapter should derive `local_minute_of_day` from the user's local clock;
//! this keeps timezone policy at the boundary and leaves the domain logic fully
//! deterministic and straightforward to test.

/// The default policy is intentionally conservative: a user must have been
/// idle for at least two hours and no proactive message may have been sent in
/// the previous six hours.
pub const DEFAULT_MIN_IDLE_SECONDS: i64 = 2 * 60 * 60;
pub const DEFAULT_COOLDOWN_SECONDS: i64 = 6 * 60 * 60;

/// Configures when an idle check-in is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProactivePolicy {
    pub enabled: bool,
    pub minimum_idle_seconds: i64,
    pub cooldown_seconds: i64,
    pub quiet_hours: Option<QuietHours>,
}

impl Default for ProactivePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            minimum_idle_seconds: DEFAULT_MIN_IDLE_SECONDS,
            cooldown_seconds: DEFAULT_COOLDOWN_SECONDS,
            quiet_hours: None,
        }
    }
}

/// A local time range during which proactive messages are suppressed.
///
/// The start is inclusive and the end is exclusive. Ranges may cross midnight;
/// for example `23:00..08:00` suppresses messages late at night and early in
/// the morning. Equal endpoints represent a full-day quiet period. Construct
/// values with [`QuietHours::new`] to reject invalid clock values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuietHours {
    start_minute: u16,
    end_minute: u16,
}

impl QuietHours {
    /// Creates a quiet-hours range from minute offsets after midnight.
    pub fn new(start_minute: u16, end_minute: u16) -> Option<Self> {
        (start_minute < MINUTES_PER_DAY && end_minute < MINUTES_PER_DAY).then_some(Self {
            start_minute,
            end_minute,
        })
    }

    /// Returns whether the supplied local minute is within this range.
    pub fn contains(self, local_minute_of_day: u16) -> bool {
        if local_minute_of_day >= MINUTES_PER_DAY {
            return true;
        }
        if self.start_minute == self.end_minute {
            return true;
        }
        if self.start_minute < self.end_minute {
            (self.start_minute..self.end_minute).contains(&local_minute_of_day)
        } else {
            local_minute_of_day >= self.start_minute || local_minute_of_day < self.end_minute
        }
    }
}

const MINUTES_PER_DAY: u16 = 24 * 60;

/// The time snapshot required to decide whether the companion should reach
/// out. All timestamps are Unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProactiveContext {
    pub now: i64,
    pub idle_started_at: i64,
    pub local_minute_of_day: u16,
}

/// The kind of proactive message supported by the current policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProactiveTriggerKind {
    IdleCheckIn,
}

impl ProactiveTriggerKind {
    fn storage_name(self) -> &'static str {
        match self {
            Self::IdleCheckIn => "idle-check-in",
        }
    }
}

/// Stable identity for one possible proactive dispatch.
///
/// The idle session start is part of the key. Therefore, repeated scheduler
/// ticks can never send a second check-in for the same uninterrupted idle
/// session, even after the cooldown has elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProactiveEventKey {
    pub kind: ProactiveTriggerKind,
    pub idle_started_at: i64,
}

impl ProactiveEventKey {
    /// Serializes this key for a unique database column or key-value store.
    pub fn storage_key(self) -> String {
        format!("{}:{}", self.kind.storage_name(), self.idle_started_at)
    }

    /// Parses a key emitted by [`ProactiveEventKey::storage_key`].
    pub fn from_storage_key(value: &str) -> Option<Self> {
        let (kind, idle_started_at) = value.split_once(':')?;
        (kind == "idle-check-in").then_some(Self {
            kind: ProactiveTriggerKind::IdleCheckIn,
            idle_started_at: idle_started_at.parse().ok()?,
        })
    }
}

/// A successfully dispatched proactive event loaded from persistent storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProactiveDispatchRecord {
    pub key: ProactiveEventKey,
    pub dispatched_at: i64,
}

/// A deterministic check-in selected by the policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProactiveSuggestion {
    pub key: ProactiveEventKey,
    pub kind: ProactiveTriggerKind,
}

/// Selects one eligible low-frequency companion check-in, if any.
///
/// The function rejects disabled settings, invalid/clock-skewed idle sessions,
/// quiet hours, duplicate event keys, and attempts inside the global cooldown.
/// A record dated in the future suppresses dispatch conservatively until its
/// timestamp is no longer in the future. Callers must persist the returned key
/// only after a notification was actually handed off successfully.
pub fn next_suggestion(
    policy: ProactivePolicy,
    context: ProactiveContext,
    dispatch_history: impl IntoIterator<Item = ProactiveDispatchRecord>,
) -> Option<ProactiveSuggestion> {
    if !policy.enabled
        || policy.minimum_idle_seconds < 0
        || policy.cooldown_seconds < 0
        || context.local_minute_of_day >= MINUTES_PER_DAY
        || context.idle_started_at > context.now
        || context.now.saturating_sub(context.idle_started_at) < policy.minimum_idle_seconds
        || policy
            .quiet_hours
            .is_some_and(|quiet_hours| quiet_hours.contains(context.local_minute_of_day))
    {
        return None;
    }

    let key = ProactiveEventKey {
        kind: ProactiveTriggerKind::IdleCheckIn,
        idle_started_at: context.idle_started_at,
    };
    let mut last_dispatch = None;
    for record in dispatch_history {
        if record.key == key {
            return None;
        }
        last_dispatch = Some(
            last_dispatch
                .unwrap_or(record.dispatched_at)
                .max(record.dispatched_at),
        );
    }

    match last_dispatch {
        Some(dispatched_at)
            if dispatched_at > context.now
                || context.now.saturating_sub(dispatched_at) < policy.cooldown_seconds =>
        {
            None
        }
        _ => Some(ProactiveSuggestion {
            key,
            kind: ProactiveTriggerKind::IdleCheckIn,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_COOLDOWN_SECONDS, DEFAULT_MIN_IDLE_SECONDS, ProactiveContext,
        ProactiveDispatchRecord, ProactiveEventKey, ProactivePolicy, ProactiveTriggerKind,
        QuietHours, next_suggestion,
    };

    const NOW: i64 = 1_800_000_000;
    const MORNING: u16 = 9 * 60;

    fn context(idle_started_at: i64) -> ProactiveContext {
        ProactiveContext {
            now: NOW,
            idle_started_at,
            local_minute_of_day: MORNING,
        }
    }

    #[test]
    fn requires_enabled_setting_and_minimum_idle_duration() {
        let disabled = ProactivePolicy {
            enabled: false,
            ..ProactivePolicy::default()
        };
        assert_eq!(
            next_suggestion(disabled, context(NOW - 3 * 60 * 60), []),
            None
        );
        assert_eq!(
            next_suggestion(ProactivePolicy::default(), context(NOW - 60 * 60), []),
            None
        );
        assert!(
            next_suggestion(
                ProactivePolicy::default(),
                context(NOW - DEFAULT_MIN_IDLE_SECONDS),
                []
            )
            .is_some()
        );
    }

    #[test]
    fn suppresses_quiet_hours_including_overnight_ranges() {
        let policy = ProactivePolicy {
            quiet_hours: QuietHours::new(23 * 60, 8 * 60),
            ..ProactivePolicy::default()
        };
        let mut at_night = context(NOW - 3 * 60 * 60);
        at_night.local_minute_of_day = 23 * 60;
        assert_eq!(next_suggestion(policy, at_night, []), None);
        at_night.local_minute_of_day = 7 * 60 + 59;
        assert_eq!(next_suggestion(policy, at_night, []), None);
        at_night.local_minute_of_day = 8 * 60;
        assert!(next_suggestion(policy, at_night, []).is_some());
    }

    #[test]
    fn duplicate_key_is_suppressed_after_restart() {
        let key = ProactiveEventKey {
            kind: ProactiveTriggerKind::IdleCheckIn,
            idle_started_at: NOW - 3 * 60 * 60,
        };
        let history = [ProactiveDispatchRecord {
            key,
            dispatched_at: NOW - DEFAULT_COOLDOWN_SECONDS - 1,
        }];

        assert_eq!(
            next_suggestion(
                ProactivePolicy::default(),
                context(key.idle_started_at),
                history
            ),
            None
        );
    }

    #[test]
    fn a_recent_other_session_still_enforces_global_cooldown() {
        let history = [ProactiveDispatchRecord {
            key: ProactiveEventKey {
                kind: ProactiveTriggerKind::IdleCheckIn,
                idle_started_at: NOW - 10 * 60 * 60,
            },
            dispatched_at: NOW - 1,
        }];

        assert_eq!(
            next_suggestion(
                ProactivePolicy::default(),
                context(NOW - 3 * 60 * 60),
                history
            ),
            None
        );
    }

    #[test]
    fn old_history_allows_a_new_idle_session() {
        let history = [ProactiveDispatchRecord {
            key: ProactiveEventKey {
                kind: ProactiveTriggerKind::IdleCheckIn,
                idle_started_at: NOW - 2 * 24 * 60 * 60,
            },
            dispatched_at: NOW - DEFAULT_COOLDOWN_SECONDS,
        }];

        assert!(
            next_suggestion(
                ProactivePolicy::default(),
                context(NOW - 3 * 60 * 60),
                history
            )
            .is_some()
        );
    }

    #[test]
    fn event_key_round_trips_through_persistence_format() {
        let key = ProactiveEventKey {
            kind: ProactiveTriggerKind::IdleCheckIn,
            idle_started_at: -42,
        };
        assert_eq!(
            ProactiveEventKey::from_storage_key(&key.storage_key()),
            Some(key)
        );
        assert_eq!(ProactiveEventKey::from_storage_key("other:42"), None);
        assert_eq!(
            ProactiveEventKey::from_storage_key("idle-check-in:not-a-time"),
            None
        );
    }

    #[test]
    fn invalid_or_future_clock_snapshots_fail_closed() {
        let future_idle = context(NOW + 1);
        assert_eq!(
            next_suggestion(ProactivePolicy::default(), future_idle, []),
            None
        );

        let invalid_local_time = ProactiveContext {
            local_minute_of_day: 24 * 60,
            ..context(NOW - 3 * 60 * 60)
        };
        assert_eq!(
            next_suggestion(ProactivePolicy::default(), invalid_local_time, []),
            None
        );
    }
}

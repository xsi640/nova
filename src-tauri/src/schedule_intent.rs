//! Conservative, side-effect-free recognition of Chinese schedule requests.
//!
//! This module deliberately does not read the clock, call an LLM, or persist
//! anything.  The command layer supplies the user's local calendar date, then
//! presents the returned value for an explicit confirmation before storing it.

/// A calendar date in the user's local timezone (with no implicit timezone conversion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl LocalDate {
    /// Constructs a valid proleptic-Gregorian calendar date.
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        (year >= 1 && (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month))
            .then_some(Self { year, month, day })
    }

    fn add_days(self, mut days: u8) -> Self {
        let mut result = self;
        while days > 0 {
            if result.day < days_in_month(result.year, result.month) {
                result.day += 1;
            } else {
                result.day = 1;
                if result.month == 12 {
                    result.month = 1;
                    result.year += 1;
                } else {
                    result.month += 1;
                }
            }
            days -= 1;
        }
        result
    }
}

/// A parsed request which must still be shown to, and accepted by, the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleIntent {
    pub title: String,
    /// Local wall-clock time in the same `YYYY-MM-DDTHH:MM` format accepted by
    /// the existing `confirm_schedule` command.
    pub scheduled_at: String,
    /// Defaults to the event time. A later policy may let the UI adjust it.
    pub remind_at: String,
    pub kind: ScheduleIntentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleIntentKind {
    Reminder,
    Schedule,
}

/// Conversation state owned by the UI or command layer until the user chooses
/// to confirm or dismiss the proposed schedule. It intentionally has no
/// database id: unconfirmed requests are never persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingScheduleConfirmation {
    pub source_message_id: i64,
    pub intent: ScheduleIntent,
}

impl PendingScheduleConfirmation {
    pub fn new(source_message_id: i64, intent: ScheduleIntent) -> Self {
        Self {
            source_message_id,
            intent,
        }
    }
}

/// Recognizes an explicit Chinese reminder/schedule request with an exact date
/// and time. Relative dates are resolved solely against `local_today`.
///
/// Intentionally unsupported examples return `None`: a date without a time,
/// a time without a date, ambiguous phrases such as `下周`, and bare chat such
/// as `明天开会` without an explicit scheduling cue.
pub fn parse_schedule_intent(message: &str, local_today: LocalDate) -> Option<ScheduleIntent> {
    let message = message.trim();
    if message.is_empty() {
        return None;
    }

    let (kind, trigger_span) = explicit_trigger(message)?;
    let (date, date_span) = parse_date(message, local_today)?;
    let ((hour, minute), time_span) = parse_time(message)?;
    let title = extract_title(message, &[trigger_span, date_span, time_span]);
    if title.is_empty() || title.chars().count() > 160 {
        return None;
    }

    let datetime = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}",
        date.year, date.month, date.day, hour, minute
    );
    Some(ScheduleIntent {
        title,
        scheduled_at: datetime.clone(),
        remind_at: datetime,
        kind,
    })
}

fn explicit_trigger(message: &str) -> Option<(ScheduleIntentKind, (usize, usize))> {
    // Reminder wording is always an explicit request, including when the title
    // itself is not a calendar-related word (for example, "给妈妈打电话").
    for marker in ["提醒我", "提醒一下", "提醒"] {
        if let Some(start) = message.find(marker) {
            return Some((ScheduleIntentKind::Reminder, (start, start + marker.len())));
        }
    }

    // Schedule wording has a smaller accepted vocabulary to avoid turning
    // ordinary conversational references to a meeting into a pending action.
    for marker in [
        "创建日程",
        "添加日程",
        "安排日程",
        "记个日程",
        "帮我安排",
        "安排",
    ] {
        if let Some(start) = message.find(marker) {
            return Some((ScheduleIntentKind::Schedule, (start, start + marker.len())));
        }
    }
    None
}

fn parse_date(message: &str, local_today: LocalDate) -> Option<(LocalDate, (usize, usize))> {
    // Absolute dates must contain a year, so e.g. "10月1日" cannot silently
    // choose between this year and next year.
    for (year_end, _) in message.match_indices('年') {
        let Some((year, year_start)) = digits_before(message, year_end) else {
            continue;
        };
        if !(1000..=9999).contains(&year) {
            continue;
        }
        let Some((month, month_end)) = digits_after(message, year_end + '年'.len_utf8()) else {
            continue;
        };
        if message[month_end..].starts_with('月') {
            let Some((day, day_end)) = digits_after(message, month_end + '月'.len_utf8()) else {
                continue;
            };
            if message[day_end..].starts_with('日') {
                let date = LocalDate::new(year as i32, month as u8, day as u8)?;
                return Some((date, (year_start, day_end + '日'.len_utf8())));
            }
        }
    }

    for (marker, offset) in [("后天", 2), ("明天", 1), ("今天", 0), ("今日", 0)] {
        if let Some(start) = message.find(marker) {
            return Some((local_today.add_days(offset), (start, start + marker.len())));
        }
    }
    None
}

fn parse_time(message: &str) -> Option<((u8, u8), (usize, usize))> {
    // `9点`, `09点30分`, and `9点半` are deliberately accepted; a bare `9:30`
    // is not, because it is too easy to confuse with unrelated text.
    for (point, _) in message.match_indices('点') {
        let Some((raw_hour, start)) = digits_before(message, point) else {
            continue;
        };
        let after_point = point + '点'.len_utf8();
        let (minute, end) = if message[after_point..].starts_with('半') {
            (30, after_point + '半'.len_utf8())
        } else if let Some((minute, minute_end)) = digits_after(message, after_point) {
            if message[minute_end..].starts_with('分') {
                (minute, minute_end + '分'.len_utf8())
            } else {
                (0, after_point)
            }
        } else {
            (0, after_point)
        };
        let hour = apply_day_period(message, start, raw_hour)?;
        if minute <= 59 {
            return Some((
                (hour, minute as u8),
                (day_period_start(message, start).unwrap_or(start), end),
            ));
        }
    }
    None
}

fn apply_day_period(message: &str, time_start: usize, raw_hour: u32) -> Option<u8> {
    let period = day_period_start(message, time_start).and_then(|position| {
        ["凌晨", "早上", "上午", "中午", "下午", "傍晚", "晚上"]
            .iter()
            .find(|marker| message[position..].starts_with(**marker))
            .copied()
    });

    let hour = match period {
        Some("下午") | Some("傍晚") | Some("晚上") if (1..=11).contains(&raw_hour) => {
            raw_hour + 12
        }
        Some("下午") | Some("傍晚") | Some("晚上") if raw_hour == 12 => 12,
        Some("上午") | Some("早上") if raw_hour <= 11 => raw_hour,
        Some("中午") if (1..=11).contains(&raw_hour) => raw_hour + 12,
        Some("中午") if raw_hour == 12 => 12,
        Some("凌晨") if raw_hour <= 5 => raw_hour,
        None if raw_hour <= 23 => raw_hour,
        _ => return None,
    };
    Some(hour as u8)
}

fn day_period_start(message: &str, time_start: usize) -> Option<usize> {
    let context = &message[..time_start];
    ["凌晨", "早上", "上午", "中午", "下午", "傍晚", "晚上"]
        .iter()
        .filter_map(|marker| context.rfind(marker))
        .filter(|position| time_start - position <= 8)
        .max()
}

fn digits_before(text: &str, end: usize) -> Option<(u32, usize)> {
    let bytes = text.as_bytes();
    let mut start = end;
    while start > 0 && bytes[start - 1].is_ascii_digit() {
        start -= 1;
    }
    (start < end)
        .then(|| text[start..end].parse().ok())
        .flatten()
        .map(|number| (number, start))
}

fn digits_after(text: &str, start: usize) -> Option<(u32, usize)> {
    let bytes = text.as_bytes();
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    (start < end)
        .then(|| text[start..end].parse().ok())
        .flatten()
        .map(|number| (number, end))
}

fn extract_title(message: &str, spans: &[(usize, usize)]) -> String {
    let mut spans = spans.to_vec();
    spans.sort_unstable();
    let mut title = String::new();
    let mut cursor = 0;
    for (start, end) in spans {
        if start >= cursor {
            title.push_str(&message[cursor..start]);
            cursor = end;
        }
    }
    title.push_str(&message[cursor..]);
    title
        .trim_matches(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '，' | '。' | '！' | '？' | ',' | '.' | '!' | '?' | ':' | '：'
                )
        })
        .trim_start_matches(|character: char| matches!(character, '在' | '于' | '的'))
        .trim()
        .to_owned()
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LocalDate, PendingScheduleConfirmation, ScheduleIntentKind, parse_schedule_intent,
    };

    const TODAY: LocalDate = LocalDate {
        year: 2026,
        month: 12,
        day: 31,
    };

    #[test]
    fn parses_explicit_reminder_with_relative_date_and_afternoon_time() {
        let intent =
            parse_schedule_intent("提醒我明天下午3点半给妈妈打电话", TODAY).expect("intent");
        assert_eq!(intent.title, "给妈妈打电话");
        assert_eq!(intent.scheduled_at, "2027-01-01T15:30");
        assert_eq!(intent.remind_at, intent.scheduled_at);
        assert_eq!(intent.kind, ScheduleIntentKind::Reminder);
    }

    #[test]
    fn parses_absolute_date_and_preserves_local_wall_clock_format() {
        let intent = parse_schedule_intent("帮我安排2027年2月28日上午09点05分产品评审", TODAY)
            .expect("intent");
        assert_eq!(intent.title, "产品评审");
        assert_eq!(intent.scheduled_at, "2027-02-28T09:05");
        assert_eq!(intent.kind, ScheduleIntentKind::Schedule);
    }

    #[test]
    fn pending_confirmation_keeps_the_source_message_without_persisting() {
        let intent = parse_schedule_intent("提醒我后天晚上8点交报告", TODAY).expect("intent");
        let pending = PendingScheduleConfirmation::new(42, intent);
        assert_eq!(pending.source_message_id, 42);
        assert_eq!(pending.intent.scheduled_at, "2027-01-02T20:00");
    }

    #[test]
    fn rejects_ambiguous_or_non_actionable_text() {
        assert!(parse_schedule_intent("明天开会", TODAY).is_none());
        assert!(parse_schedule_intent("提醒我明天给妈妈打电话", TODAY).is_none());
        assert!(parse_schedule_intent("提醒我下午3点给妈妈打电话", TODAY).is_none());
        assert!(parse_schedule_intent("提醒我下周一上午9点开会", TODAY).is_none());
        assert!(parse_schedule_intent("提醒我2027年2月29日上午9点开会", TODAY).is_none());
    }

    #[test]
    fn relative_date_rolls_over_leap_year_boundary() {
        let leap_day = LocalDate::new(2028, 2, 28).expect("valid date");
        let intent = parse_schedule_intent("提醒我明天上午8点晨跑", leap_day).expect("intent");
        assert_eq!(intent.scheduled_at, "2028-02-29T08:00");
    }
}

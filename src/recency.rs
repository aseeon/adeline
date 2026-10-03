//! When a chat was last active, in the terms the chat list shows: a period
//! ("Today", "Last 3 days", "Earlier") and a short label ("5m", "Tue").
//!
//! Calendar days are UTC days, so a chat active shortly after local midnight
//! can land in the neighbouring day for users far from UTC.
use chrono::{DateTime, Datelike as _, NaiveDate, SecondsFormat, Utc};

const DAY: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Today,
    /// The two days before today; with today, the last three calendar days.
    LastThreeDays,
    Earlier,
}

/// Seconds since the Unix epoch, now.
pub fn now() -> i64 {
    Utc::now().timestamp()
}

/// Milliseconds since the Unix epoch, now.
pub fn now_ms() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0)
}

/// Stored chats record milliseconds since the epoch; bundled demo chats use
/// RFC 3339 UTC timestamps such as `2026-09-24T10:00:00Z`.
pub fn parse(stamp: &str) -> Option<i64> {
    let stamp = stamp.trim();
    if !stamp.is_empty() && stamp.bytes().all(|b| b.is_ascii_digit()) {
        let value: i64 = stamp.parse().ok()?;
        // Anything past the year 5138 in seconds is really milliseconds.
        return Some(if value > 99_999_999_999 {
            value / 1000
        } else {
            value
        });
    }
    if let Ok(at) = DateTime::parse_from_rfc3339(stamp) {
        return Some(at.timestamp());
    }
    let date = NaiveDate::parse_from_str(stamp, "%Y-%m-%d").ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

pub fn period(at: i64, now: i64) -> Period {
    match now.div_euclid(DAY) - at.div_euclid(DAY) {
        ..=0 => Period::Today,
        1..=2 => Period::LastThreeDays,
        _ => Period::Earlier,
    }
}

/// The compact time shown beside a chat or project: `now`, `5m`, `3h`, the
/// weekday within the last week (yesterday included), then a date.
pub fn label(at: i64, now: i64) -> String {
    let elapsed = now - at;
    let days = now.div_euclid(DAY) - at.div_euclid(DAY);
    if elapsed < 60 {
        return "now".into();
    }
    if elapsed < 3600 {
        return format!("{}m", elapsed / 60);
    }
    if days <= 0 {
        return format!("{}h", elapsed / 3600);
    }
    let Some(date) = DateTime::from_timestamp(at, 0) else {
        return String::new();
    };
    if days < 7 {
        date.format("%a").to_string()
    } else if Some(date.year()) == DateTime::from_timestamp(now, 0).map(|d| d.year()) {
        date.format("%b %-d").to_string()
    } else {
        date.format("%b %-d, %Y").to_string()
    }
}

/// RFC 3339 UTC, as the bundled demo chats store their times.
pub fn iso(at: i64) -> String {
    DateTime::from_timestamp(at, 0)
        .map(|date| date.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sunday, 27 September 2026, 12:00 UTC.
    const NOW: i64 = 1_790_510_400;

    #[test]
    fn parses_stored_milliseconds_and_demo_timestamps_to_the_same_instant() {
        assert_eq!(parse("2026-09-27T12:00:00Z"), Some(NOW));
        assert_eq!(parse("1790510400000"), Some(NOW));
        assert_eq!(parse("1790510400"), Some(NOW));
        assert_eq!(parse("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso(NOW), "2026-09-27T12:00:00Z");
        assert_eq!(parse(&iso(NOW - 1)), Some(NOW - 1));
        assert_eq!(parse("2024-02-29"), Some(19_782 * DAY));
        for invalid in ["", "yesterday", "2026-13-01T00:00:00Z", "2026-09"] {
            assert_eq!(parse(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn periods_follow_calendar_days() {
        let midnight = NOW - 12 * 3600;
        assert_eq!(period(midnight, NOW), Period::Today);
        assert_eq!(period(midnight - 1, NOW), Period::LastThreeDays);
        assert_eq!(period(midnight - 2 * DAY, NOW), Period::LastThreeDays);
        assert_eq!(period(midnight - 2 * DAY - 1, NOW), Period::Earlier);
        // A clock ahead of ours still counts as today.
        assert_eq!(period(NOW + 90, NOW), Period::Today);
    }

    #[test]
    fn labels_shorten_with_recency() {
        for (at, expected) in [
            (NOW + 5, "now"),
            (NOW - 59, "now"),
            (NOW - 5 * 60, "5m"),
            (NOW - 3 * 3600, "3h"),
            (NOW - DAY, "Sat"),
            (NOW - 2 * DAY, "Fri"),
            (NOW - 6 * DAY, "Mon"),
            (NOW - 8 * DAY, "Sep 19"),
            (parse("2025-12-31T10:00:00Z").unwrap(), "Dec 31, 2025"),
        ] {
            assert_eq!(label(at, NOW), expected);
        }
    }
}

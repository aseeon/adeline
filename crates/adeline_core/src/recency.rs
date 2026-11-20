//! When a chat was last active, in the terms the chat list shows: a period
//! ("Today", "Last 3 days", "Earlier") and a short label ("5m", "Tue").
use jiff::{Timestamp, civil::Date, tz::TimeZone};

const DAY: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Today,
    LastThreeDays,
    Earlier,
}

pub fn now() -> i64 {
    Timestamp::now().as_second()
}

pub fn now_ms() -> u64 {
    u64::try_from(Timestamp::now().as_millisecond()).unwrap_or(0)
}

pub fn parse(stamp: &str) -> Option<i64> {
    let stamp = stamp.trim();
    if !stamp.is_empty() && stamp.bytes().all(|b| b.is_ascii_digit()) {
        let value: i64 = stamp.parse().ok()?;
        return Some(if value > 99_999_999_999 {
            value / 1000
        } else {
            value
        });
    }
    if let Ok(at) = stamp.parse::<Timestamp>() {
        return Some(at.as_second());
    }
    let date = Date::strptime("%Y-%m-%d", stamp).ok()?;
    Some(date.to_zoned(TimeZone::UTC).ok()?.timestamp().as_second())
}

pub fn period(at: i64, now: i64) -> Period {
    period_in(at, now, &TimeZone::system())
}

pub fn label(at: i64, now: i64) -> String {
    label_in(at, now, &TimeZone::system())
}

fn day(at: i64, zone: &TimeZone) -> i64 {
    let offset = Timestamp::from_second(at).map_or(0, |at| zone.to_offset(at).seconds());
    (at + i64::from(offset)).div_euclid(DAY)
}

fn period_in(at: i64, now: i64, zone: &TimeZone) -> Period {
    match day(now, zone) - day(at, zone) {
        ..=0 => Period::Today,
        1..=2 => Period::LastThreeDays,
        _ => Period::Earlier,
    }
}

fn label_in(at: i64, now: i64, zone: &TimeZone) -> String {
    let elapsed = now - at;
    let days = day(now, zone) - day(at, zone);
    if elapsed < 60 {
        return "now".into();
    }
    if elapsed < 3600 {
        return format!("{}m", elapsed / 60);
    }
    if days <= 0 {
        return format!("{}h", elapsed / 3600);
    }
    let local = |at| Timestamp::from_second(at).map(|at| zone.to_datetime(at));
    let Ok(date) = local(at) else {
        return String::new();
    };
    if days < 7 {
        date.strftime("%a").to_string()
    } else if local(now).is_ok_and(|now| now.year() == date.year()) {
        date.strftime("%b %-d").to_string()
    } else {
        date.strftime("%b %-d, %Y").to_string()
    }
}

pub fn iso(at: i64) -> String {
    Timestamp::from_second(at)
        .map(|at| at.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let period = |at| period_in(at, NOW, &TimeZone::UTC);
        let midnight = NOW - 12 * 3600;
        assert_eq!(period(midnight), Period::Today);
        assert_eq!(period(midnight - 1), Period::LastThreeDays);
        assert_eq!(period(midnight - 2 * DAY), Period::LastThreeDays);
        assert_eq!(period(midnight - 2 * DAY - 1), Period::Earlier);
        assert_eq!(period(NOW + 90), Period::Today);
    }

    #[test]
    fn days_start_at_local_midnight() {
        let five_west = TimeZone::fixed(jiff::tz::offset(-5));
        let local_midnight = NOW - 7 * 3600;
        assert_eq!(period_in(local_midnight, NOW, &five_west), Period::Today);
        assert_eq!(
            period_in(local_midnight - 1, NOW, &five_west),
            Period::LastThreeDays
        );
        assert_eq!(
            period_in(local_midnight - 1, NOW, &TimeZone::UTC),
            Period::Today
        );
        assert_eq!(label_in(local_midnight - 3600, NOW, &five_west), "Sat");
        assert_eq!(label_in(NOW - 6 * DAY, NOW, &five_west), "Mon");
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
            assert_eq!(label_in(at, NOW, &TimeZone::UTC), expected);
        }
    }
}

//! When a chat was last active, in the terms the chat list shows: a period
//! ("Today", "Last 3 days", "Earlier") and a short label ("5m", "Tue").
//!
//! Calendar days are UTC days. The standard library has no time zone database,
//! so a chat active shortly after local midnight can land in the neighbouring
//! day for users far from UTC.

const DAY: i64 = 86_400;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Today,
    /// The two days before today; with today, the last three calendar days.
    LastThreeDays,
    Earlier,
}

/// Seconds since the Unix epoch, now.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
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
    let number = |range: std::ops::Range<usize>| stamp.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (hour, minute, second) = if stamp.len() >= 19 {
        (number(11..13)?, number(14..16)?, number(17..19)?)
    } else {
        (0, 0, 0)
    };
    Some(days_from_civil(year, month, day) * DAY + hour * 3600 + minute * 60 + second)
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
        "now".into()
    } else if elapsed < 3600 {
        format!("{}m", elapsed / 60)
    } else if days <= 0 {
        format!("{}h", elapsed / 3600)
    } else if days < 7 {
        weekday(at).into()
    } else {
        let (year, month, day) = civil_from_days(at.div_euclid(DAY));
        let month = MONTHS[usize::try_from(month - 1).unwrap_or(0)];
        if year == civil_from_days(now.div_euclid(DAY)).0 {
            format!("{month} {day}")
        } else {
            format!("{month} {day}, {year}")
        }
    }
}

/// The three-letter weekday of a time; the epoch fell on a Thursday.
fn weekday(at: i64) -> &'static str {
    let weekday = (at.div_euclid(DAY) + 4).rem_euclid(7);
    WEEKDAYS[usize::try_from(weekday).unwrap_or(0)]
}

/// RFC 3339 UTC, as the bundled demo chats store their times.
pub fn iso(at: i64) -> String {
    let (year, month, day) = civil_from_days(at.div_euclid(DAY));
    let second = at.rem_euclid(DAY);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second / 3600,
        second % 3600 / 60,
        second % 60
    )
}

// Howard Hinnant's proleptic Gregorian conversions.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
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
        assert_eq!(
            parse("2024-02-29"),
            Some(days_from_civil(2024, 2, 29) * DAY)
        );
        for invalid in ["", "yesterday", "2026-13-01T00:00:00Z", "2026-09"] {
            assert_eq!(parse(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn calendar_conversion_round_trips() {
        for days in [-800_000, -1, 0, 1, 11_016, 20_723, 2_000_000] {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
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

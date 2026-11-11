//! How long ago a time was, as tables show it: at most two units, from
//! seconds up to years.

use chrono::{DateTime, Utc};
use rust_i18n::t;

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const YEAR: i64 = 365 * DAY;

#[derive(Clone, Copy)]
enum Unit {
    Seconds,
    Minutes,
    Hours,
    Days,
    Years,
}

fn part(n: i64, unit: Unit) -> String {
    match unit {
        Unit::Seconds => t!("age.seconds", n = n),
        Unit::Minutes => t!("age.minutes", n = n),
        Unit::Hours => t!("age.hours", n = n),
        Unit::Days => t!("age.days", n = n),
        Unit::Years => t!("age.years", n = n),
    }
    .to_string()
}

/// The larger unit, then the smaller one unless it is zero.
fn two(seconds: i64, large: (i64, Unit), small: (i64, Unit)) -> String {
    let first = part(seconds / large.0, large.1);
    match seconds % large.0 / small.0 {
        0 => first,
        rest => first + &part(rest, small.1),
    }
}

/// The time since `text`, or `None` if it is not an RFC 3339 time or lies in
/// the future, in which case it is shown as received.
pub fn age(text: &str, now: DateTime<Utc>) -> Option<String> {
    let time = DateTime::parse_from_rfc3339(text).ok()?;
    let seconds = (now - time.with_timezone(&Utc)).num_seconds();
    let (s, m, h, d, y) = (
        (1, Unit::Seconds),
        (MINUTE, Unit::Minutes),
        (HOUR, Unit::Hours),
        (DAY, Unit::Days),
        (YEAR, Unit::Years),
    );
    Some(if seconds < 0 {
        return None;
    } else if seconds < 2 * MINUTE {
        part(seconds, Unit::Seconds)
    } else if seconds < 10 * MINUTE {
        two(seconds, m, s)
    } else if seconds < 3 * HOUR {
        part(seconds / MINUTE, Unit::Minutes)
    } else if seconds < 8 * HOUR {
        two(seconds, h, m)
    } else if seconds < 2 * DAY {
        part(seconds / HOUR, Unit::Hours)
    } else if seconds < 8 * DAY {
        two(seconds, d, h)
    } else if seconds < 2 * YEAR {
        part(seconds / DAY, Unit::Days)
    } else {
        two(seconds, y, d)
    })
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, Utc};

    use super::age;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-11T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn ago(seconds: i64) -> Option<String> {
        let time = now() - Duration::seconds(seconds);
        age(&time.to_rfc3339(), now())
    }

    #[test]
    fn each_range_uses_its_units() {
        let cases = [
            (0, "0s"),
            (119, "119s"),
            (120, "2m"),
            (330, "5m30s"),
            (599, "9m59s"),
            (600, "10m"),
            (10_799, "179m"),
            (10_800, "3h"),
            (11_040, "3h4m"),
            (28_799, "7h59m"),
            (28_800, "8h"),
            (172_799, "47h"),
            (172_800, "2d"),
            (273_600, "3d4h"),
            (691_199, "7d23h"),
            (691_200, "8d"),
            (3_542_400, "41d"),
            (63_071_999, "729d"),
            (63_072_000, "2y"),
            (66_614_400, "2y41d"),
        ];
        for (seconds, expected) in cases {
            assert_eq!(ago(seconds).as_deref(), Some(expected), "{seconds}s ago");
        }
    }

    #[test]
    fn a_time_with_an_offset_counts_from_the_same_instant() {
        assert_eq!(
            age("2026-11-11T13:55:00+02:00", now()).as_deref(),
            Some("5m")
        );
    }

    #[test]
    fn text_that_is_not_a_past_time_has_no_age() {
        assert_eq!(age("yesterday", now()), None);
        assert_eq!(age("", now()), None);
        assert_eq!(ago(-1), None);
    }
}

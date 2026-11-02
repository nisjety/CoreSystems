//! Durations as they read in a sentence.

use chrono::Duration;

/// An age for a sentence: `42 min`, `5 h 12 min`, `3 d 4 h`.
pub(super) fn human(span: Duration) -> String {
    let minutes = span.num_minutes().max(0);
    match minutes {
        0 => "under 1 min".to_string(),
        1..=59 => format!("{minutes} min"),
        60..=1439 => with_remainder(minutes / 60, "h", minutes % 60, "min"),
        _ => with_remainder(minutes / 1440, "d", (minutes % 1440) / 60, "h"),
    }
}

fn with_remainder(big: i64, big_unit: &str, small: i64, small_unit: &str) -> String {
    if small == 0 {
        format!("{big} {big_unit}")
    } else {
        format!("{big} {big_unit} {small} {small_unit}")
    }
}

/// A limit for a sentence, in the unit it is usually given in: `10 min`, `3 h`, `30 h`, `8 d`.
pub(super) fn limit_text(limit: Duration) -> String {
    let minutes = limit.num_minutes();
    if minutes >= 3 * 1440 && minutes % 1440 == 0 {
        format!("{} d", minutes / 1440)
    } else if minutes >= 60 && minutes % 60 == 0 {
        format!("{} h", minutes / 60)
    } else {
        format!("{minutes} min")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mins(count: i64) -> Duration {
        Duration::minutes(count)
    }

    fn hours(count: i64) -> Duration {
        Duration::hours(count)
    }

    fn days(count: i64) -> Duration {
        Duration::days(count)
    }

    #[test]
    fn ages_read_in_the_two_largest_units() {
        let cases = [
            (Duration::seconds(20), "under 1 min"),
            (mins(1), "1 min"),
            (mins(42), "42 min"),
            (mins(60), "1 h"),
            (mins(312), "5 h 12 min"),
            (hours(23), "23 h"),
            (hours(24), "1 d"),
            (hours(25), "1 d 1 h"),
            (days(3) + hours(4) + mins(59), "3 d 4 h"),
            (Duration::seconds(-90), "under 1 min"),
        ];
        for (span, expected) in cases {
            assert_eq!(human(span), expected, "{span}");
        }
    }

    #[test]
    fn limits_read_in_the_unit_they_are_usually_given_in() {
        let cases = [
            (mins(10), "10 min"),
            (mins(90), "90 min"),
            (mins(180), "3 h"),
            (hours(30), "30 h"),
            (hours(36), "36 h"),
            (hours(48), "48 h"),
            (days(3), "3 d"),
            (days(8), "8 d"),
        ];
        for (limit, expected) in cases {
            assert_eq!(limit_text(limit), expected, "{limit}");
        }
    }
}

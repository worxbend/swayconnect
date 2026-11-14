//! Durations of keep-awake: the `--for` argument, and the time left that the server reports.

use std::num::{IntErrorKind, ParseIntError};

use swayconnect_protocol::limits::MIN_KEEP_AWAKE_DURATION_S;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;
const MILLIS_PER_SECOND: u64 = 1000;

/// The seconds of `--for`: digits, then exactly one unit. The longest duration is the
/// server's `max_duration_s`, which only the server checks.
pub(super) fn parse_duration_s(text: &str) -> Result<u64, String> {
    let unit_start = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    let (count, unit) = text.split_at(unit_start);
    let seconds_per_unit = match unit {
        "s" => 1,
        "m" => SECONDS_PER_MINUTE,
        "h" => SECONDS_PER_HOUR,
        _ => return Err("a duration is a whole number with s, m or h, such as 90m".to_owned()),
    };
    let too_long = || "the duration is too long".to_owned();
    let count: u64 = count
        .parse()
        .map_err(|error: ParseIntError| match error.kind() {
            IntErrorKind::PosOverflow => too_long(),
            _ => "a duration starts with a whole number, such as 90m".to_owned(),
        })?;
    let duration_s = count.checked_mul(seconds_per_unit).ok_or_else(too_long)?;
    if duration_s < MIN_KEEP_AWAKE_DURATION_S {
        return Err(format!(
            "keep-awake lasts at least {MIN_KEEP_AWAKE_DURATION_S} s"
        ));
    }
    Ok(duration_s)
}

/// `remaining_ms` in the units of `--for`, rounded up to whole seconds, such as `1h 30m`.
pub(super) fn remaining_text(remaining_ms: u64) -> String {
    let seconds = remaining_ms.div_ceil(MILLIS_PER_SECOND);
    let parts = [
        (seconds / SECONDS_PER_HOUR, "h"),
        (seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE, "m"),
        (seconds % SECONDS_PER_MINUTE, "s"),
    ];
    let named: Vec<String> = parts
        .iter()
        .filter(|(count, _unit)| *count > 0)
        .map(|(count, unit)| format!("{count}{unit}"))
        .collect();
    if named.is_empty() {
        "0s".to_owned()
    } else {
        named.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_digits_and_one_unit_of_at_least_60_s() {
        for (text, seconds) in [
            ("60s", 60),
            ("1m", 60),
            ("90m", 5400),
            ("2h", 7200),
            ("0060s", 60),
        ] {
            assert_eq!(parse_duration_s(text), Ok(seconds), "{text}");
        }
    }

    #[test]
    fn any_other_duration_text_is_refused() {
        for text in [
            "59s", "0m", "60", "1d", "1H", "m", "", "+5m", "-5m", "1h30m", "1.5h", "5 m", " 5m",
            "5m ", "５m", "5ｍ",
        ] {
            assert!(parse_duration_s(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn a_duration_past_the_largest_number_is_too_long() {
        // 5124095576030432 h is the first hour count whose seconds pass u64::MAX.
        for text in ["99999999999999999999h", "5124095576030432h"] {
            let refusal = parse_duration_s(text).expect_err(text);
            assert_eq!(refusal, "the duration is too long", "{text}");
        }
        assert_eq!(
            parse_duration_s("5124095576030431h"),
            Ok(5_124_095_576_030_431 * 3600)
        );
    }

    #[test]
    fn the_time_left_is_rounded_up_and_drops_zero_units() {
        for (remaining_ms, text) in [
            (0, "0s"),
            (1, "1s"),
            (59_001, "1m"),
            (60_000, "1m"),
            (3_600_000, "1h"),
            (5_400_000, "1h 30m"),
            (3_599_999, "1h"),
            (3_661_000, "1h 1m 1s"),
            (u64::MAX, "5124095576030h 25m 52s"),
        ] {
            assert_eq!(remaining_text(remaining_ms), text, "{remaining_ms}");
        }
    }
}

//! Short forms of the numbers a segment shows. Each rounds down, so a value is never shown as
//! more than it is.

/// A token count: `950`, `1.5k`, `126k`, `1M`, `1.2M`.
pub fn tokens(count: u64) -> String {
    for (unit, suffix) in [(1_000_000, "M"), (1_000, "k")] {
        if count >= unit {
            let tenths = count / (unit / 10);
            return if tenths >= 100 || tenths.is_multiple_of(10) {
                format!("{}{suffix}", count / unit)
            } else {
                format!("{}.{}{suffix}", tenths / 10, tenths % 10)
            };
        }
    }
    count.to_string()
}

/// A span of time: `now`, `<1m`, `48m`, `1h`, `2h13m`, `3d`, `3d4h`, and `>99d` past that.
pub fn duration(seconds: u64) -> String {
    let (minutes, hours, days) = (seconds / 60, seconds / 3600, seconds / 86_400);
    match seconds {
        0 => "now".to_string(),
        1..60 => "<1m".to_string(),
        60..3600 => format!("{minutes}m"),
        3600..86_400 => with_remainder(hours, "h", minutes % 60, "m"),
        _ if days < 100 => with_remainder(days, "d", hours % 24, "h"),
        _ => ">99d".to_string(),
    }
}

fn with_remainder(whole: u64, unit: &str, rest: u64, rest_unit: &str) -> String {
    if rest == 0 {
        format!("{whole}{unit}")
    } else {
        format!("{whole}{unit}{rest}{rest_unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_durations_rounding_down() {
        let cases = [
            (0, "now"),
            (1, "<1m"),
            (59, "<1m"),
            (60, "1m"),
            (2_879, "47m"),
            (3_599, "59m"),
            (3_600, "1h"),
            (7_980, "2h13m"),
            (86_399, "23h59m"),
            (86_400, "1d"),
            (273_600, "3d4h"),
            (8_639_999, "99d23h"),
            (8_640_000, ">99d"),
            (u64::MAX, ">99d"),
        ];
        for (seconds, expected) in cases {
            assert_eq!(duration(seconds), expected, "{seconds}");
        }
    }

    #[test]
    fn shortens_token_counts_rounding_down() {
        let cases = [
            (0, "0"),
            (950, "950"),
            (999, "999"),
            (1_000, "1k"),
            (1_500, "1.5k"),
            (9_999, "9.9k"),
            (10_000, "10k"),
            (126_400, "126k"),
            (200_000, "200k"),
            (999_950, "999k"),
            (1_000_000, "1M"),
            (1_250_000, "1.2M"),
            (12_345_678, "12M"),
            (u64::MAX, "18446744073709M"),
        ];
        for (count, expected) in cases {
            assert_eq!(tokens(count), expected, "{count}");
        }
    }
}

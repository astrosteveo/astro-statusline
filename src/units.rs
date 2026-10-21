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

#[cfg(test)]
mod tests {
    use super::*;

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

//! Bars and graphs drawn from text cells, each one column wide like the rest of the line.

use crate::config::{Bar, Graph};

/// A bar `bar.width` cells wide, with one full cell per whole step of `used` out of 100. When
/// `mark` is a number, the cell holding that percentage is `bar.marker` instead, whether it's full
/// or not. `None` when `used` isn't a number.
pub fn bar(used: f64, mark: Option<f64>, bar: &Bar) -> Option<String> {
    if !used.is_finite() {
        return None;
    }
    let width = usize::from(bar.width);
    let full = cells(used, width);
    let mark = mark
        .filter(|mark| mark.is_finite())
        .map(|mark| cells(mark, width).min(width - 1));
    let text = (0..width)
        .map(|cell| match cell {
            _ if Some(cell) == mark => bar.marker.as_str(),
            _ if cell < full => bar.full.as_str(),
            _ => bar.empty.as_str(),
        })
        .collect();
    Some(text)
}

/// Whole steps of `percent` out of 100 in `width` cells. `as` truncates toward zero, and
/// `percent` is clamped first, so this is at most `width`.
fn cells(percent: f64, width: usize) -> usize {
    (percent.clamp(0.0, 100.0) / 100.0 * width as f64) as usize
}

/// How a graph maps values onto its cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// Percentages, from 0 for the lowest cell to 100 for the highest.
    Percent,
    /// Increases between samples, scaled so the largest in view reaches the highest cell. A
    /// running total such as cost reads as how much each update added.
    Increase,
}

/// The last `graph.width` of `values` as one cell each, oldest first. Values that aren't numbers
/// are skipped. `None` when there is nothing to draw.
pub fn sparkline(values: &[f64], scale: Scale, graph: &Graph) -> Option<String> {
    let values: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    let top = graph.chars.len() - 1;
    let levels: Vec<usize> = match scale {
        Scale::Percent => values
            .iter()
            .map(|v| ((v.clamp(0.0, 100.0) / 100.0 * graph.chars.len() as f64) as usize).min(top))
            .collect(),
        Scale::Increase => {
            // The first sample has nothing before it to increase from, so a lone sample draws one
            // lowest cell. A drop (a new session reusing an id) counts as no increase.
            let increases: Vec<f64> = match values.as_slice() {
                [] => Vec::new(),
                [_] => vec![0.0],
                _ => values.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect(),
            };
            let shown = &increases[increases.len().saturating_sub(graph.width.into())..];
            let largest = shown.iter().copied().fold(0.0, f64::max);
            shown
                .iter()
                .map(|v| {
                    if largest > 0.0 {
                        ((v / largest * top as f64).round() as usize).min(top)
                    } else {
                        0
                    }
                })
                .collect()
        }
    };
    let shown = &levels[levels.len().saturating_sub(graph.width.into())..];
    (!shown.is_empty()).then(|| shown.iter().map(|&level| graph.chars[level]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(used: f64) -> Option<String> {
        bar(used, None, &Bar::default())
    }

    fn marked(used: f64, mark: f64) -> Option<String> {
        bar(used, Some(mark), &Bar::default())
    }

    #[test]
    fn fills_whole_steps_only() {
        assert_eq!(draw(0.0).as_deref(), Some("░░░░░░░░░░"));
        assert_eq!(draw(9.9).as_deref(), Some("░░░░░░░░░░"));
        assert_eq!(draw(10.0).as_deref(), Some("█░░░░░░░░░"));
        assert_eq!(draw(63.0).as_deref(), Some("██████░░░░"));
        assert_eq!(draw(99.9).as_deref(), Some("█████████░"));
        assert_eq!(draw(100.0).as_deref(), Some("██████████"));
    }

    #[test]
    fn clamps_odd_values() {
        assert_eq!(draw(-5.0).as_deref(), Some("░░░░░░░░░░"));
        assert_eq!(draw(250.0).as_deref(), Some("██████████"));
        assert_eq!(draw(1e300).as_deref(), Some("██████████"));
        assert_eq!(draw(f64::NAN), None);
        assert_eq!(draw(f64::INFINITY), None);
    }

    #[test]
    fn marks_the_cell_holding_the_mark() {
        // Under pace, the marker sits among the empty cells; over pace, among the full ones.
        assert_eq!(marked(41.0, 57.0).as_deref(), Some("████░┃░░░░"));
        assert_eq!(marked(41.0, 28.0).as_deref(), Some("██┃█░░░░░░"));
        assert_eq!(marked(0.0, 0.0).as_deref(), Some("┃░░░░░░░░░"));
        // The end of the window lands on the last cell, not past it.
        assert_eq!(marked(100.0, 99.99).as_deref(), Some("█████████┃"));
        assert_eq!(marked(100.0, 100.0).as_deref(), Some("█████████┃"));
        assert_eq!(marked(10.0, 250.0).as_deref(), Some("█░░░░░░░░┃"));
        assert_eq!(marked(10.0, -5.0).as_deref(), Some("┃░░░░░░░░░"));
        // A mark that isn't a number draws no marker; a used value that isn't, no bar.
        assert_eq!(marked(10.0, f64::NAN).as_deref(), Some("█░░░░░░░░░"));
        assert_eq!(marked(f64::NAN, 50.0), None);
    }

    fn spark(values: &[f64], scale: Scale) -> Option<String> {
        sparkline(values, scale, &Graph::default())
    }

    #[test]
    fn graphs_percentages_on_a_fixed_scale() {
        assert_eq!(
            spark(
                &[0.0, 12.5, 25.0, 37.5, 50.0, 62.5, 75.0, 87.5],
                Scale::Percent
            )
            .as_deref(),
            Some("▁▂▃▄▅▆▇█")
        );
        assert_eq!(spark(&[100.0], Scale::Percent).as_deref(), Some("█"));
        assert_eq!(
            spark(&[-5.0, 250.0, f64::NAN], Scale::Percent).as_deref(),
            Some("▁█")
        );
        assert_eq!(spark(&[], Scale::Percent), None);
        assert_eq!(spark(&[f64::INFINITY], Scale::Percent), None);
    }

    #[test]
    fn shows_only_the_latest_values() {
        let values: Vec<f64> = (0..20).map(|v| v as f64 * 5.0).collect();
        assert_eq!(spark(&values, Scale::Percent).as_deref(), Some("▅▆▆▇▇▇██"));
        let narrow = Graph {
            width: 2,
            chars: vec!['.', ':', '#'],
        };
        assert_eq!(
            sparkline(&[0.0, 50.0, 100.0], Scale::Percent, &narrow).as_deref(),
            Some(":#")
        );
    }

    #[test]
    fn graphs_increases_scaled_to_the_largest_in_view() {
        // Cost went up by 1, 0, 2, then 4.
        assert_eq!(
            spark(&[1.0, 2.0, 2.0, 4.0, 8.0], Scale::Increase).as_deref(),
            Some("▃▁▅█")
        );
        assert_eq!(spark(&[3.0], Scale::Increase).as_deref(), Some("▁"));
        assert_eq!(
            spark(&[3.0, 3.0, 1.0], Scale::Increase).as_deref(),
            Some("▁▁")
        );
        assert_eq!(spark(&[], Scale::Increase), None);
        // An old spike scrolls out of view and stops flattening the rest.
        let mut values = vec![0.0, 100.0];
        values.extend((1..=8).map(|v| 100.0 + v as f64));
        assert_eq!(spark(&values, Scale::Increase).as_deref(), Some("████████"));
    }

    #[test]
    fn uses_the_configured_cells_and_width() {
        let custom = Bar {
            width: 4,
            full: "=".to_string(),
            empty: " ".to_string(),
            marker: "|".to_string(),
        };
        assert_eq!(bar(50.0, None, &custom).as_deref(), Some("==  "));
        assert_eq!(bar(50.0, Some(80.0), &custom).as_deref(), Some("== |"));
        let one = Bar { width: 1, ..custom };
        assert_eq!(bar(0.0, Some(99.0), &one).as_deref(), Some("|"));
    }
}

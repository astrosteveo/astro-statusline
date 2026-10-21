//! Which segments the status line shows, in what order, and the style of each.

use crate::segments::Kind;
use crate::style::Style;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The segments to show, left to right.
    pub segments: Vec<Kind>,
    pub colors: Colors,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colors {
    pub model: Style,
    pub directory: Style,
    pub branch: Style,
    pub cost: Style,
    pub separator: Style,
    /// Context and rate-limit percentages below 50%.
    pub usage_low: Style,
    /// Context and rate-limit percentages from 50% up to 80%.
    pub usage_medium: Style,
    /// Context and rate-limit percentages from 80% up.
    pub usage_high: Style,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            segments: Kind::ALL.to_vec(),
            colors: Colors::default(),
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        let style = |text: &str| text.parse().expect("default styles parse");
        Colors {
            model: style("cyan"),
            directory: style("blue"),
            branch: style("magenta"),
            cost: style("dim"),
            separator: style("dim"),
            usage_low: style("green"),
            usage_medium: style("yellow"),
            usage_high: style("red"),
        }
    }
}

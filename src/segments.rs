//! The segments of the status line. Each is left out when the session lacks its data.

use serde::Deserialize;

use crate::config::{Colors, Config};
use crate::git;
use crate::render::Segment;
use crate::session::Session;

/// A segment the config can name. Each keeps its priority wherever it sits in the line, so a
/// narrow terminal drops the same segments first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Model,
    Directory,
    Branch,
    Context,
    FiveHour,
    SevenDay,
    Cost,
}

impl Kind {
    /// Every segment, in the default display order.
    pub const ALL: [Kind; 7] = [
        Kind::Model,
        Kind::Directory,
        Kind::Branch,
        Kind::Context,
        Kind::FiveHour,
        Kind::SevenDay,
        Kind::Cost,
    ];
}

/// The segments `config` lists, in its order.
pub fn all(session: &Session, config: &Config) -> Vec<Segment> {
    let colors = &config.colors;
    config
        .segments
        .iter()
        .filter_map(|kind| match kind {
            Kind::Model => model(session, colors),
            Kind::Directory => directory(session, colors),
            Kind::Branch => branch(session, colors),
            Kind::Context => context(session, colors),
            Kind::FiveHour => five_hour_limit(session, colors),
            Kind::SevenDay => seven_day_limit(session, colors),
            Kind::Cost => cost(session, colors),
        })
        .collect()
}

fn model(session: &Session, colors: &Colors) -> Option<Segment> {
    let name = session.model.as_ref()?.display_name.as_deref()?;
    Some(Segment::new(name, colors.model.clone(), 90))
}

fn directory(session: &Session, colors: &Colors) -> Option<Segment> {
    let name = session.current_dir()?.file_name()?.to_string_lossy();
    Some(Segment::new(name, colors.directory.clone(), 80))
}

fn branch(session: &Session, colors: &Colors) -> Option<Segment> {
    let branch = git::head(session.current_dir()?)?;
    Some(Segment::new(branch, colors.branch.clone(), 70))
}

fn context(session: &Session, colors: &Colors) -> Option<Segment> {
    let used = session.context_window.as_ref()?.used_percentage?;
    Some(percentage("ctx", used, 100, colors))
}

fn five_hour_limit(session: &Session, colors: &Colors) -> Option<Segment> {
    let used = session
        .rate_limits
        .as_ref()?
        .five_hour
        .as_ref()?
        .used_percentage?;
    Some(percentage("5h", used, 60, colors))
}

fn seven_day_limit(session: &Session, colors: &Colors) -> Option<Segment> {
    let used = session
        .rate_limits
        .as_ref()?
        .seven_day
        .as_ref()?
        .used_percentage?;
    Some(percentage("7d", used, 50, colors))
}

fn cost(session: &Session, colors: &Colors) -> Option<Segment> {
    let usd = session.cost.as_ref()?.total_cost_usd?;
    Some(Segment::new(format!("${usd:.2}"), colors.cost.clone(), 40))
}

/// A labeled percentage, in the low usage color below half, medium below 80%, and high from there
/// up.
fn percentage(label: &str, used: f64, priority: u8, colors: &Colors) -> Segment {
    let style = match used {
        u if u < 50.0 => &colors.usage_low,
        u if u < 80.0 => &colors.usage_medium,
        _ => &colors.usage_high,
    };
    // `as` saturates, so a negative or out-of-range value can't print as "-0" or wrap.
    let rounded = used.round().max(0.0) as u64;
    Segment::new(format!("{label} {rounded}%"), style.clone(), priority)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;

    const FULL: &str = r#"{
        "model": {"id": "claude-opus-5-5", "display_name": "Opus"},
        "workspace": {"current_dir": "/home/me/proj", "project_dir": "/home/me/proj"},
        "context_window": {"used_percentage": 8, "context_window_size": 200000},
        "rate_limits": {
            "five_hour": {"used_percentage": 23.5, "resets_at": 1738425600},
            "seven_day": {"used_percentage": 41.2, "resets_at": 1738857600}
        },
        "cost": {"total_cost_usd": 0.01234, "total_duration_ms": 45000}
    }"#;

    fn segments(input: &str) -> Vec<Segment> {
        all(&Session::parse(input.as_bytes()), &Config::default())
    }

    fn texts(input: &str) -> Vec<String> {
        segments(input).into_iter().map(|s| s.text).collect()
    }

    fn style(text: &str) -> Style {
        text.parse().unwrap()
    }

    #[test]
    fn shows_each_documented_field_in_order() {
        assert_eq!(
            texts(FULL),
            ["Opus", "proj", "ctx 8%", "5h 24%", "7d 41%", "$0.01"]
        );
    }

    #[test]
    fn follows_the_configured_order_and_colors() {
        let config = Config {
            segments: vec![Kind::Cost, Kind::Context, Kind::Model],
            colors: Colors {
                model: style("bold #ff8700"),
                usage_low: style("bright_green"),
                ..Colors::default()
            },
        };
        let segments = all(&Session::parse(FULL.as_bytes()), &config);

        let texts: Vec<&str> = segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["$0.01", "ctx 8%", "Opus"]);
        assert_eq!(segments[1].style, style("bright_green"));
        assert_eq!(segments[2].style, style("bold #ff8700"));
        // Priority belongs to the segment, not its position.
        assert_eq!(segments[1].priority, 100);
    }

    #[test]
    fn leaves_out_segments_without_data() {
        assert!(texts("{}").is_empty());
        assert_eq!(texts(r#"{"cwd": "/"}"#), Vec::<String>::new());
        assert_eq!(texts(r#"{"model": {"display_name": "Opus"}}"#), ["Opus"]);
        let early = r#"{
            "context_window": {"used_percentage": null, "current_usage": null},
            "rate_limits": {"seven_day": {"used_percentage": 3}},
            "cost": {}
        }"#;
        assert_eq!(texts(early), ["7d 3%"]);
    }

    #[test]
    fn colors_percentages_by_how_much_is_used() {
        let color = |used: &str| {
            segments(&format!(
                r#"{{"context_window": {{"used_percentage": {used}}}}}"#
            ))
            .remove(0)
            .style
        };
        assert_eq!(color("0"), style("green"));
        assert_eq!(color("49.9"), style("green"));
        assert_eq!(color("50"), style("yellow"));
        assert_eq!(color("79.9"), style("yellow"));
        assert_eq!(color("80"), style("red"));
        assert_eq!(color("100"), style("red"));
    }

    #[test]
    fn rounds_percentages_without_a_negative_zero() {
        let text = |used: &str| {
            texts(&format!(
                r#"{{"context_window": {{"used_percentage": {used}}}}}"#
            ))
        };
        assert_eq!(text("49.5"), ["ctx 50%"]);
        assert_eq!(text("-0.4"), ["ctx 0%"]);
        assert_eq!(text("1e300"), [format!("ctx {}%", u64::MAX)]);
    }
}

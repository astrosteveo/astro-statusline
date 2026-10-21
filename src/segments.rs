//! The segments of the status line. Each is left out when the session lacks the data it is about,
//! whatever its format template says, and otherwise drawn from that template.

use serde::Deserialize;

use crate::config::{Colors, Config};
use crate::git;
use crate::render::Segment;
use crate::session::{RateLimit, Session};
use crate::style::Style;
use crate::template::Template;
use crate::units;

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

    /// The placeholders this segment's format template can use. `segment` fills each one.
    pub fn placeholders(self) -> &'static [&'static str] {
        match self {
            Kind::Model | Kind::Directory => &["name"],
            Kind::Branch => &["branch"],
            Kind::Context => &["percent", "tokens", "size"],
            Kind::FiveHour | Kind::SevenDay => &["percent"],
            Kind::Cost => &["usd"],
        }
    }

    /// When the line is too wide, lower priorities are dropped first.
    fn priority(self) -> u8 {
        match self {
            Kind::Context => 100,
            Kind::Model => 90,
            Kind::Directory => 80,
            Kind::Branch => 70,
            Kind::FiveHour => 60,
            Kind::SevenDay => 50,
            Kind::Cost => 40,
        }
    }
}

/// The segments `config` lists, in its order.
pub fn all(session: &Session, config: &Config) -> Vec<Segment> {
    config
        .segments
        .iter()
        .filter_map(|&kind| segment(kind, session, config))
        .collect()
}

fn segment(kind: Kind, session: &Session, config: &Config) -> Option<Segment> {
    let (colors, format) = (&config.colors, &config.format);
    let (text, style) = match kind {
        Kind::Model => {
            let name = session.model.as_ref()?.display_name.as_deref()?;
            let text = format.model.render(|key| match key {
                "name" => Some(name.to_string()),
                _ => None,
            });
            (text?, &colors.model)
        }
        Kind::Directory => {
            let name = session.current_dir()?.file_name()?.to_string_lossy();
            let text = format.directory.render(|key| match key {
                "name" => Some(name.to_string()),
                _ => None,
            });
            (text?, &colors.directory)
        }
        Kind::Branch => {
            let branch = git::head(session.current_dir()?)?;
            let text = format.branch.render(|key| match key {
                "branch" => Some(branch.clone()),
                _ => None,
            });
            (text?, &colors.branch)
        }
        Kind::Context => {
            let window = session.context_window.as_ref()?;
            let used = window.used_percentage?;
            let text = format.context.render(|key| match key {
                "percent" => Some(percent(used)),
                "tokens" => window.total_input_tokens.map(units::tokens),
                "size" => window.context_window_size.map(units::tokens),
                _ => None,
            });
            (text?, usage_style(used, colors))
        }
        Kind::FiveHour => {
            let limit = session.rate_limits.as_ref()?.five_hour.as_ref()?;
            rate_limit(limit, &format.five_hour, colors)?
        }
        Kind::SevenDay => {
            let limit = session.rate_limits.as_ref()?.seven_day.as_ref()?;
            rate_limit(limit, &format.seven_day, colors)?
        }
        Kind::Cost => {
            let usd = session.cost.as_ref()?.total_cost_usd?;
            let text = format.cost.render(|key| match key {
                "usd" => Some(format!("${usd:.2}")),
                _ => None,
            });
            (text?, &colors.cost)
        }
    };
    Some(Segment::new(text, style.clone(), kind.priority()))
}

fn rate_limit<'a>(
    limit: &RateLimit,
    template: &Template,
    colors: &'a Colors,
) -> Option<(String, &'a Style)> {
    let used = limit.used_percentage?;
    let text = template.render(|key| match key {
        "percent" => Some(percent(used)),
        _ => None,
    })?;
    Some((text, usage_style(used, colors)))
}

/// A percentage, rounded. `as` saturates, so a negative or out-of-range value can't print as
/// "-0" or wrap.
fn percent(used: f64) -> String {
    format!("{}%", used.round().max(0.0) as u64)
}

/// The low usage color below half, medium below 80%, and high from there up.
fn usage_style(used: f64, colors: &Colors) -> &Style {
    match used {
        u if u < 50.0 => &colors.usage_low,
        u if u < 80.0 => &colors.usage_medium,
        _ => &colors.usage_high,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;

    const FULL: &str = r#"{
        "model": {"id": "claude-opus-5-5", "display_name": "Opus"},
        "workspace": {"current_dir": "/home/me/proj", "project_dir": "/home/me/proj"},
        "context_window": {
            "used_percentage": 8,
            "total_input_tokens": 16000,
            "context_window_size": 200000
        },
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

    /// The default config, with `kind` drawn from `template` instead.
    fn with_template(kind: Kind, template: &str) -> Config {
        let mut format = Config::default().format;
        let field = match kind {
            Kind::Model => &mut format.model,
            Kind::Directory => &mut format.directory,
            Kind::Branch => &mut format.branch,
            Kind::Context => &mut format.context,
            Kind::FiveHour => &mut format.five_hour,
            Kind::SevenDay => &mut format.seven_day,
            Kind::Cost => &mut format.cost,
        };
        *field = Template::parse(template, kind.placeholders()).unwrap();
        Config {
            segments: vec![kind],
            format,
            ..Config::default()
        }
    }

    fn text_with(kind: Kind, template: &str, input: &str) -> Option<String> {
        all(
            &Session::parse(input.as_bytes()),
            &with_template(kind, template),
        )
        .pop()
        .map(|s| s.text)
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
            ..Config::default()
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
    fn fills_every_placeholder_a_segment_allows() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        std::fs::write(repo.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let input = FULL.replace("/home/me/proj", &repo.path().display().to_string());
        for kind in Kind::ALL {
            for name in kind.placeholders() {
                let text = text_with(kind, &format!("{{{name}}}"), &input);
                assert!(text.is_some(), "{kind:?} left {{{name}}} empty");
            }
        }
    }

    #[test]
    fn draws_each_segment_from_its_template() {
        let text = |kind, template| text_with(kind, template, FULL).unwrap();
        assert_eq!(
            text(Kind::Context, "{tokens}/{size} ({percent})"),
            "16k/200k (8%)"
        );
        assert_eq!(text(Kind::Cost, "[[{usd}]]"), "[$0.01]");
        assert_eq!(text(Kind::Model, "🤖 {name}"), "🤖 Opus");
        assert_eq!(text(Kind::FiveHour, "five {percent}"), "five 24%");
        // A group with a missing value disappears, and the rest of the segment stays.
        let input = r#"{"context_window": {"used_percentage": 8}}"#;
        assert_eq!(
            text_with(Kind::Context, "ctx {percent}[ of {size}]", input).as_deref(),
            Some("ctx 8%")
        );
        assert_eq!(text_with(Kind::Context, "{percent} of {size}", input), None);
    }

    #[test]
    fn a_template_cannot_show_a_segment_without_its_data() {
        // The context segment is about `used_percentage`, so it stays out without it, even when
        // its template could be drawn from other fields or from nothing at all.
        let no_percent =
            r#"{"context_window": {"total_input_tokens": 5, "context_window_size": 9}}"#;
        assert_eq!(
            text_with(Kind::Context, "{tokens}/{size}", no_percent),
            None
        );
        assert_eq!(text_with(Kind::Context, "ctx[ {percent}]", "{}"), None);
        assert_eq!(text_with(Kind::Cost, "spent", "{}"), None);
        assert_eq!(
            text_with(Kind::Cost, "spent", r#"{"cost": {"total_cost_usd": 0}}"#).as_deref(),
            Some("spent")
        );
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

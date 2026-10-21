//! The segments of the status line. Each is left out when the session lacks the data it is about,
//! whatever its format template says, and otherwise drawn from that template.

use serde::Deserialize;

use crate::config::{Colors, Config};
use crate::git;
use crate::graph::{self, Scale};
use crate::history::Sample;
use crate::predict;
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
            Kind::Context => &["percent", "bar", "graph", "tokens", "size"],
            Kind::FiveHour | Kind::SevenDay => {
                &["percent", "bar", "graph", "resets_in", "projected", "eta"]
            }
            Kind::Cost => &["usd", "rate", "graph"],
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

/// What segments are drawn from.
pub struct Env<'a> {
    pub session: &'a Session,
    pub config: &'a Config,
    /// Seconds since the Unix epoch, or `None` when the clock can't say, which leaves out
    /// everything measured from now.
    pub now: Option<i64>,
    /// The session's recent samples, oldest first, ending with the current values. Empty when
    /// no segment shown needs them.
    pub history: &'a [Sample],
}

/// The segments the config lists, in its order.
pub fn all(env: &Env) -> Vec<Segment> {
    env.config
        .segments
        .iter()
        .filter_map(|&kind| segment(kind, env))
        .collect()
}

fn segment(kind: Kind, env: &Env) -> Option<Segment> {
    let (session, config) = (env.session, env.config);
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
                "bar" => graph::bar(used, &config.bar),
                "graph" => graph(env, |s| s.context, Scale::Percent),
                "tokens" => window.total_input_tokens.map(units::tokens),
                "size" => window.context_window_size.map(units::tokens),
                _ => None,
            });
            (text?, usage_style(used, colors))
        }
        Kind::FiveHour => {
            let limit = session.rate_limits.as_ref()?.five_hour.as_ref()?;
            rate_limit(
                limit,
                predict::FIVE_HOURS,
                |s| s.five_hour,
                &format.five_hour,
                env,
            )?
        }
        Kind::SevenDay => {
            let limit = session.rate_limits.as_ref()?.seven_day.as_ref()?;
            rate_limit(
                limit,
                predict::SEVEN_DAYS,
                |s| s.seven_day,
                &format.seven_day,
                env,
            )?
        }
        Kind::Cost => {
            let cost = session.cost.as_ref()?;
            let usd = cost.total_cost_usd?;
            let text = format.cost.render(|key| match key {
                "usd" => Some(format!("${usd:.2}")),
                "rate" => cost
                    .total_duration_ms
                    .and_then(|ms| predict::cost_rate(usd, ms))
                    .map(|rate| format!("${rate:.2}")),
                "graph" => graph(env, |s| s.cost, Scale::Increase),
                _ => None,
            });
            (text?, &colors.cost)
        }
    };
    Some(Segment::new(text, style.clone(), kind.priority()))
}

/// A rate limit over a `window` seconds long, whose history `value` picks out of each sample.
fn rate_limit<'a>(
    limit: &RateLimit,
    window: f64,
    value: fn(&Sample) -> Option<f64>,
    template: &Template,
    env: &Env<'a>,
) -> Option<(String, &'a Style)> {
    let used = limit.used_percentage?;
    let now_and_reset = env.now.zip(limit.resets_at);
    let pace =
        now_and_reset.and_then(|(now, resets_at)| predict::pace(used, resets_at, now, window));
    let text = template.render(|key| match key {
        "percent" => Some(percent(used)),
        "bar" => graph::bar(used, &env.config.bar),
        "graph" => graph(env, value, Scale::Percent),
        "resets_in" => now_and_reset
            .and_then(|(now, resets_at)| predict::resets_in(resets_at, now, window))
            .map(seconds),
        "projected" => pace.map(|pace| match pace.projected {
            p if p > 999.0 => ">999%".to_string(),
            // Rounded down, so it reads 100% only when the eta says the limit will be hit.
            p => format!("{}%", p as u64),
        }),
        "eta" => pace.and_then(|pace| pace.eta).map(seconds),
        _ => None,
    })?;
    Some((text, usage_style(used, &env.config.colors)))
}

/// A graph of `value` across the session's history, skipping samples without it.
fn graph(env: &Env, value: fn(&Sample) -> Option<f64>, scale: Scale) -> Option<String> {
    let values: Vec<f64> = env.history.iter().filter_map(value).collect();
    graph::sparkline(&values, scale, &env.config.graph)
}

/// A duration in seconds, which callers have checked is a finite number not below zero.
fn seconds(seconds: f64) -> String {
    // `as` rounds toward zero and saturates.
    units::duration(seconds as u64)
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

    /// When the fixtures are drawn: an hour before `FULL`'s 5-hour window resets.
    const NOW: i64 = 1_738_422_000;

    fn draw(input: &str, config: &Config) -> Vec<Segment> {
        let session = Session::parse(input.as_bytes());
        // The history always ends with the current values, as it does when the binary runs.
        all(&Env {
            session: &session,
            config,
            now: Some(NOW),
            history: &[Sample::new(&session, NOW)],
        })
    }

    fn segments(input: &str) -> Vec<Segment> {
        draw(input, &Config::default())
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
        draw(input, &with_template(kind, template))
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
        let segments = draw(FULL, &config);

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
        // Usage high enough that both limits are on pace to run out, and a session long enough
        // to have a cost rate.
        let input = FULL
            .replace("/home/me/proj", &repo.path().display().to_string())
            .replace("23.5", "90")
            .replace("45000", "3600000");
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
        assert_eq!(text(Kind::SevenDay, "{bar} {percent}"), "████░░░░░░ 41%");
        assert_eq!(text(Kind::Context, "{bar}"), "░░░░░░░░░░");
        // A group with a missing value disappears, and the rest of the segment stays.
        let input = r#"{"context_window": {"used_percentage": 8}}"#;
        assert_eq!(
            text_with(Kind::Context, "ctx {percent}[ of {size}]", input).as_deref(),
            Some("ctx 8%")
        );
        assert_eq!(text_with(Kind::Context, "{percent} of {size}", input), None);
    }

    #[test]
    fn predicts_rate_limits_and_cost() {
        let text = |kind, template, input: &str| text_with(kind, template, input).unwrap();
        // An hour left of five, 23.5% used: 29% by the reset, with no limit in sight.
        let template = "{percent} resets {resets_in} → {projected}[ limit in {eta}]";
        assert_eq!(text(Kind::FiveHour, template, FULL), "24% resets 1h → 29%");
        // At 90%, the limit comes in 26 minutes, before the reset.
        let busy = FULL.replace("23.5", "90");
        assert_eq!(
            text(Kind::FiveHour, template, &busy),
            "90% resets 1h → 112% limit in 26m"
        );
        let over = FULL.replace("23.5", "100");
        assert_eq!(
            text(Kind::FiveHour, template, &over),
            "100% resets 1h → 125% limit in now"
        );
        // 5 days 1 hour left of seven, 41.2% used.
        assert_eq!(
            text(Kind::SevenDay, "{resets_in} {projected} {eta}", FULL),
            "5d1h 147% 2d19h"
        );
        // $0.50 over half an hour.
        let input = r#"{"cost": {"total_cost_usd": 0.5, "total_duration_ms": 1800000}}"#;
        assert_eq!(
            text(Kind::Cost, "{usd}[ ({rate}/h)]", input),
            "$0.50 ($1.00/h)"
        );
        // Under a minute in, there's no rate yet.
        assert_eq!(text(Kind::Cost, "{usd}[ ({rate}/h)]", FULL), "$0.01");
    }

    #[test]
    fn leaves_out_predictions_the_clock_or_data_cannot_support() {
        let config = with_template(
            Kind::FiveHour,
            "{percent}[ {resets_in}][ {projected}][ {eta}]",
        );
        let session = Session::parse(FULL.replace("23.5", "90").as_bytes());
        let draw = |now| {
            all(&Env {
                session: &session,
                config: &config,
                now,
                history: &[],
            })
            .pop()
            .map(|s| s.text)
        };
        assert_eq!(draw(Some(NOW)).as_deref(), Some("90% 1h 112% 26m"));
        assert_eq!(draw(None).as_deref(), Some("90%"));
        // Past the reset, and long before the window began.
        assert_eq!(draw(Some(NOW + 3601)).as_deref(), Some("90%"));
        assert_eq!(draw(Some(NOW - 86_400)).as_deref(), Some("90%"));
        let no_reset = r#"{"rate_limits": {"five_hour": {"used_percentage": 90}}}"#;
        assert_eq!(
            text_with(Kind::FiveHour, "{percent}[ {resets_in}][ {eta}]", no_reset).as_deref(),
            Some("90%")
        );
    }

    #[test]
    fn graphs_the_session_history() {
        let config = with_template(Kind::Context, "{percent} {graph}");
        let session = Session::parse(br#"{"context_window": {"used_percentage": 80}}"#);
        let history = [
            Sample {
                context: Some(10.0),
                ..Sample::default()
            },
            // An update without a context value, as right after /compact.
            Sample::default(),
            Sample {
                context: Some(80.0),
                ..Sample::default()
            },
        ];
        let segments = all(&Env {
            session: &session,
            config: &config,
            now: Some(NOW),
            history: &history,
        });
        assert_eq!(segments[0].text, "80% ▁▇");
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

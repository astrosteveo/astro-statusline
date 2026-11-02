//! The segments of the status line. Each is left out when the session lacks the data it is about,
//! whatever its format template says, and otherwise drawn from that template.

use serde::Deserialize;

use crate::config::{Colors, Config};
use crate::graph::{self, Scale};
use crate::history::Sample;
use crate::predict;
use crate::render::{self, Segment};
use crate::session::{RateLimit, Session};
use crate::style::Style;
use crate::template::{Pieces, Template};
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

    /// The segment's name in the config file.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::Directory => "directory",
            Kind::Branch => "branch",
            Kind::Context => "context",
            Kind::FiveHour => "five_hour",
            Kind::SevenDay => "seven_day",
            Kind::Cost => "cost",
        }
    }

    /// The segment the config file calls `name`.
    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// Whether the segment shows a percentage, and so has usage colors.
    pub fn has_usage(self) -> bool {
        matches!(self, Kind::Context | Kind::FiveHour | Kind::SevenDay)
    }

    /// Parses `text` as this segment's format template.
    pub fn parse_template(self, text: &str) -> Result<Template, String> {
        Template::parse(text, self.placeholders(), self.has_usage())
    }

    /// The placeholders this segment's format template can use. `segment` fills each one.
    pub fn placeholders(self) -> &'static [&'static str] {
        match self {
            Kind::Model | Kind::Directory => &["name"],
            Kind::Branch => &["branch"],
            Kind::Context => &["percent", "bar", "graph", "eta", "tokens", "size"],
            Kind::FiveHour | Kind::SevenDay => &[
                "percent",
                "bar",
                "graph",
                "resets_in",
                "projected",
                "eta",
                "target",
                "delta",
            ],
            Kind::Cost => &["usd", "rate", "graph"],
        }
    }

    /// When the line is too wide, lower priorities are dropped first, unless the config's
    /// `drop_order` says otherwise.
    pub fn default_priority(self) -> u8 {
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
    /// The checked-out branch, or `None` outside a repository or when no row shows it.
    pub branch: Option<&'a str>,
}

/// Each row of the status line, top to bottom, fitted to a terminal `columns` wide when that's
/// known. A row with nothing to show is left out, so rows appear as their data arrives.
pub fn lines(env: &Env, columns: Option<usize>, color: bool) -> Vec<String> {
    let config = env.config;
    let max_width = columns.map(|columns| render::max_width(columns, config.padding));
    config
        .rows()
        .into_iter()
        .enumerate()
        .map(|(index, kinds)| {
            render::render(
                row(env, kinds),
                config.format.separator.row(index),
                &config.colors.separator,
                max_width,
                color,
            )
        })
        .filter(|line| !line.is_empty())
        .collect()
}

/// The segments of one row, in its order.
pub fn row(env: &Env, kinds: &[Kind]) -> Vec<Segment> {
    kinds
        .iter()
        .filter_map(|&kind| segment(kind, env))
        .collect()
}

fn segment(kind: Kind, env: &Env) -> Option<Segment> {
    let (session, config) = (env.session, env.config);
    let (colors, format) = (&config.colors, &config.format);
    let (text, usage) = match kind {
        Kind::Model => {
            let name = session.model.as_ref()?.display_name.as_deref()?;
            let text = format.model.render(|key| match key {
                "name" => Some(name.to_string()),
                _ => None,
            });
            (text?, None)
        }
        Kind::Directory => {
            let name = session.current_dir()?.file_name()?.to_string_lossy();
            let text = format.directory.render(|key| match key {
                "name" => Some(name.to_string()),
                _ => None,
            });
            (text?, None)
        }
        Kind::Branch => {
            let branch = env.branch?;
            let text = format.branch.render(|key| match key {
                "branch" => Some(branch.to_string()),
                _ => None,
            });
            (text?, None)
        }
        Kind::Context => {
            let window = session.context_window.as_ref()?;
            let used = window.used_percentage?;
            if !config.show_from.shows(kind, used) {
                return None;
            }
            let text = format.context.render(|key| match key {
                "percent" => Some(percent(used)),
                "bar" => graph::bar(used, None, &config.bar),
                "graph" => graph(env, |s| s.context, Scale::Percent),
                "eta" => env.now.and_then(|now| {
                    let samples: Vec<_> = env.history.iter().map(|s| (s.t, s.context)).collect();
                    predict::fill_eta(&samples, now).map(seconds)
                }),
                "tokens" => window.total_input_tokens.map(units::tokens),
                "size" => window.context_window_size.map(units::tokens),
                _ => None,
            });
            (
                text?,
                Some(usage_style(used, colors, config.thresholds.context)),
            )
        }
        Kind::FiveHour => {
            let limit = session.rate_limits.as_ref()?.five_hour.as_ref()?;
            rate_limit(
                limit,
                predict::FIVE_HOURS,
                |s| s.five_hour,
                &format.five_hour,
                kind,
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
                kind,
                env,
            )?
        }
        Kind::Cost => {
            let cost = session.cost.as_ref()?;
            let usd = cost.total_cost_usd?;
            if !config.show_from.shows(kind, usd) {
                return None;
            }
            let text = format.cost.render(|key| match key {
                "usd" => Some(format!("${usd:.2}")),
                "rate" => cost
                    .total_duration_ms
                    .and_then(|ms| predict::cost_rate(usd, ms))
                    .map(|rate| format!("${rate:.2}")),
                "graph" => graph(env, |s| s.cost, Scale::Increase),
                _ => None,
            });
            (text?, None)
        }
    };
    // `usage` in a style becomes the usage color of the percentage drawn.
    let resolve = |style: &Style| usage.map_or_else(|| style.clone(), |usage| style.resolve(usage));
    let base = resolve(colors.get(kind));
    let pieces = text
        .into_iter()
        .map(|(text, style)| (text, style.map_or_else(|| base.clone(), resolve)));
    Some(Segment::from_pieces(pieces, config.priority(kind)))
}

/// The rate limit `kind` over a `window` seconds long, whose history `value` picks out of each
/// sample.
fn rate_limit<'a>(
    limit: &RateLimit,
    window: f64,
    value: fn(&Sample) -> Option<f64>,
    template: &'a Template,
    kind: Kind,
    env: &Env<'a>,
) -> Option<(Pieces<'a>, Option<&'a Style>)> {
    let config = env.config;
    let used = limit.used_percentage?;
    if !config.show_from.shows(kind, used) {
        return None;
    }
    let thresholds = config.thresholds.get(kind)?;
    let now_and_reset = env.now.zip(limit.resets_at);
    let pace =
        now_and_reset.and_then(|(now, resets_at)| predict::pace(used, resets_at, now, window));
    let target = now_and_reset.and_then(|(now, resets_at)| predict::target(resets_at, now, window));
    let text = template.render(|key| match key {
        "percent" => Some(percent(used)),
        "bar" => graph::bar(used, target, &env.config.bar),
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
        "target" => target.map(percent),
        "delta" => target.map(|target| delta(used, target)),
        _ => None,
    })?;
    Some((text, Some(usage_style(used, &config.colors, thresholds))))
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
    format!("{}%", whole_percent(used))
}

/// How far `used` is over (`+8%`) or under (`-16%`) `target`, from the rounded values, so it
/// agrees with what `percent` shows for each.
fn delta(used: f64, target: f64) -> String {
    let (used, target) = (whole_percent(used), whole_percent(target));
    let sign = match used.cmp(&target) {
        std::cmp::Ordering::Greater => "+",
        std::cmp::Ordering::Less => "-",
        std::cmp::Ordering::Equal => "",
    };
    format!("{sign}{}%", used.abs_diff(target))
}

/// A percentage rounded to a whole number, saturating at 0 and `u64::MAX`.
fn whole_percent(value: f64) -> u64 {
    value.round().max(0.0) as u64
}

/// The low usage color below the medium threshold, medium below the high one, and high from there
/// up.
fn usage_style(used: f64, colors: &Colors, [medium, high]: [u8; 2]) -> &Style {
    match used {
        u if u < f64::from(medium) => &colors.usage_low,
        u if u < f64::from(high) => &colors.usage_medium,
        _ => &colors.usage_high,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;

    /// Every row's segments, in order.
    fn all(env: &Env) -> Vec<Segment> {
        env.config
            .rows()
            .into_iter()
            .flat_map(|kinds| row(env, kinds))
            .collect()
    }

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
            branch: None,
        })
    }

    fn segments(input: &str) -> Vec<Segment> {
        draw(input, &Config::default())
    }

    fn texts(input: &str) -> Vec<String> {
        segments(input).into_iter().map(|s| s.text()).collect()
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
        *field = kind.parse_template(template).unwrap();
        Config {
            segments: Some(vec![kind]),
            format,
            ..Config::default()
        }
    }

    fn text_with(kind: Kind, template: &str, input: &str) -> Option<String> {
        draw(input, &with_template(kind, template))
            .pop()
            .map(|s| s.text())
    }

    #[test]
    fn names_each_kind_as_the_config_file_does() {
        for kind in Kind::ALL {
            let parsed: Kind = serde_json::from_value(kind.name().into()).unwrap();
            assert_eq!(parsed, kind);
            assert_eq!(Kind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(Kind::from_name("git"), None);
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
            segments: Some(vec![Kind::Cost, Kind::Context, Kind::Model]),
            colors: Colors {
                model: style("bold #ff8700"),
                usage_low: style("bright_green"),
                ..Colors::default()
            },
            ..Config::default()
        };
        let segments = draw(FULL, &config);

        let texts: Vec<String> = segments.iter().map(|s| s.text()).collect();
        assert_eq!(texts, ["$0.01", "ctx 8%", "Opus"]);
        assert_eq!(segments[1].style(), style("bright_green"));
        assert_eq!(segments[2].style(), style("bold #ff8700"));
        // Priority belongs to the segment, not its position.
        assert_eq!(segments[1].priority, 100);
    }

    #[test]
    fn fills_every_placeholder_a_segment_allows() {
        // Usage high enough that both limits are on pace to run out, and a session long enough
        // to have a cost rate.
        let input = FULL.replace("23.5", "90").replace("45000", "3600000");
        let session = Session::parse(input.as_bytes());
        // Context lower ten minutes ago, so it has a fill rate.
        let earlier = Sample {
            t: NOW - 600,
            context: Some(1.0),
            ..Sample::default()
        };
        let history = [earlier, Sample::new(&session, NOW)];
        for kind in Kind::ALL {
            for name in kind.placeholders() {
                let config = with_template(kind, &format!("{{{name}}}"));
                let env = Env {
                    session: &session,
                    config: &config,
                    now: Some(NOW),
                    history: &history,
                    branch: Some("main"),
                };
                assert!(!all(&env).is_empty(), "{kind:?} left {{{name}}} empty");
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
        assert_eq!(text(Kind::SevenDay, "{bar} {percent}"), "██┃█░░░░░░ 41%");
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
        // Where each limit should be to end at 100%: 2d23h of seven days gone is 28%, four of
        // five hours 80%.
        let template = "{percent} of {target} ({delta})";
        assert_eq!(text(Kind::SevenDay, template, FULL), "41% of 28% (+13%)");
        assert_eq!(text(Kind::FiveHour, template, FULL), "24% of 80% (-56%)");
        let on_pace = FULL.replace("23.5", "80.2");
        assert_eq!(text(Kind::FiveHour, template, &on_pace), "80% of 80% (0%)");
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
            "{percent}[ {resets_in}][ {projected}][ {eta}][ {target}][ {delta}]",
        );
        let session = Session::parse(FULL.replace("23.5", "90").as_bytes());
        let draw = |now| {
            all(&Env {
                session: &session,
                config: &config,
                now,
                history: &[],
                branch: None,
            })
            .pop()
            .map(|s| s.text())
        };
        assert_eq!(draw(Some(NOW)).as_deref(), Some("90% 1h 112% 26m 80% +10%"));
        assert_eq!(draw(None).as_deref(), Some("90%"));
        // Past the reset, and long before the window began.
        assert_eq!(draw(Some(NOW + 3601)).as_deref(), Some("90%"));
        assert_eq!(draw(Some(NOW - 86_400)).as_deref(), Some("90%"));
        let no_reset = r#"{"rate_limits": {"five_hour": {"used_percentage": 90}}}"#;
        assert_eq!(
            text_with(
                Kind::FiveHour,
                "{percent}[ {resets_in}][ {eta}][ {target}][ {delta}]",
                no_reset
            )
            .as_deref(),
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
            branch: None,
        });
        assert_eq!(segments[0].text(), "80% ▁▇");
    }

    #[test]
    fn predicts_when_the_context_fills() {
        let config = with_template(Kind::Context, "{percent}[ full in {eta}]");
        let session = Session::parse(br#"{"context_window": {"used_percentage": 60}}"#);
        let at = |t, context| Sample {
            t,
            context: Some(context),
            ..Sample::default()
        };
        let draw = |history: &[Sample]| {
            all(&Env {
                session: &session,
                config: &config,
                now: Some(NOW),
                history,
                branch: None,
            })
            .remove(0)
            .text()
        };
        assert_eq!(
            draw(&[at(NOW - 600, 40.0), at(NOW, 60.0)]),
            "60% full in 20m"
        );
        assert_eq!(draw(&[at(NOW, 60.0)]), "60%");
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
            .style()
        };
        assert_eq!(color("0"), style("green"));
        assert_eq!(color("49.9"), style("green"));
        assert_eq!(color("50"), style("yellow"));
        assert_eq!(color("79.9"), style("yellow"));
        assert_eq!(color("80"), style("red"));
        assert_eq!(color("100"), style("red"));
    }

    #[test]
    fn draws_a_placeholder_in_its_own_style() {
        let config = crate::config::parse(
            r#"
            segments = ["model", "context", "five_hour"]
            colors.context = "dim"
            format.model = "{name:bold}!"
            format.context = "ctx {percent:bold usage}"
            format.five_hour = "5h {percent:italic}"
            "#,
        )
        .unwrap();
        let pieces: Vec<Vec<(String, Style)>> = draw(FULL, &config)
            .into_iter()
            .map(|s| s.pieces.into_iter().map(|p| (p.text, p.style)).collect())
            .collect();
        assert_eq!(
            pieces,
            [
                vec![("Opus".into(), style("bold")), ("!".into(), style("cyan"))],
                vec![
                    ("ctx ".into(), style("dim")),
                    ("8%".into(), style("bold green"))
                ],
                // The rate limit's own style is `usage`, so its label is in the usage color.
                vec![
                    ("5h ".into(), style("green")),
                    ("24%".into(), style("italic"))
                ],
            ]
        );
    }

    #[test]
    fn shows_segments_only_from_their_value_up() {
        let config = crate::config::parse(
            "show_from.context = 10\nshow_from.five_hour = 24\nshow_from.seven_day = 41.3\nshow_from.cost = 0.02",
        )
        .unwrap();
        let shown = |input: &str| -> Vec<String> {
            draw(input, &config).into_iter().map(|s| s.text()).collect()
        };
        // FULL has context 8%, 5-hour 23.5% (24% rounded), 7-day 41.2%, and $0.01.
        assert_eq!(shown(FULL), ["Opus", "proj"]);
        let higher = FULL
            .replace("\"used_percentage\": 8", "\"used_percentage\": 10")
            .replace("23.5", "24")
            .replace("41.2", "41.3")
            .replace("0.01234", "0.02");
        assert_eq!(
            shown(&higher),
            ["Opus", "proj", "ctx 10%", "5h 24%", "7d 41%", "$0.02"]
        );
    }

    #[test]
    fn colors_each_percentage_by_its_own_thresholds() {
        let config = Config {
            thresholds: crate::config::Thresholds {
                context: [60, 85],
                five_hour: [0, 100],
                seven_day: [30, 30],
            },
            ..Config::default()
        };
        let input = |context: u8, five_hour: u8, seven_day: u8| {
            format!(
                r#"{{"context_window": {{"used_percentage": {context}}},
                    "rate_limits": {{"five_hour": {{"used_percentage": {five_hour}}},
                                     "seven_day": {{"used_percentage": {seven_day}}}}}}}"#
            )
        };
        let colors = |context, five_hour, seven_day| -> Vec<Style> {
            draw(&input(context, five_hour, seven_day), &config)
                .into_iter()
                .map(|s| s.style())
                .collect()
        };
        let (green, yellow, red) = (style("green"), style("yellow"), style("red"));
        assert_eq!(
            colors(59, 0, 29),
            [green.clone(), yellow.clone(), green.clone()]
        );
        assert_eq!(
            colors(60, 99, 30),
            [yellow.clone(), yellow.clone(), red.clone()]
        );
        assert_eq!(colors(85, 100, 100), [red.clone(), red.clone(), red]);
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

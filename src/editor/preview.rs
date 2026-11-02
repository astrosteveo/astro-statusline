//! The status line as Claude Code would show it, drawn by the same code from sample sessions.

use ratatui::style::{Color, Modifier, Style as TuiStyle};
use ratatui::text::{Line, Span};

use crate::config::Config;
use crate::history::Sample;
use crate::render;
use crate::segments::{self, Env};
use crate::session::Session;

/// When every sample session is drawn. Their reset times are counted from here.
const NOW: i64 = 1_800_000_000;

/// A session to draw the preview from.
pub struct Scenario {
    pub name: &'static str,
    session: &'static str,
    branch: Option<&'static str>,
}

const MID_SESSION: &str = r#"{
    "session_id": "preview",
    "model": {"display_name": "Opus 5.5"},
    "workspace": {"current_dir": "/home/you/my-project"},
    "context_window": {"used_percentage": 42, "total_input_tokens": 84000, "context_window_size": 200000},
    "rate_limits": {
        "five_hour": {"used_percentage": 35, "resets_at": 1800007200},
        "seven_day": {"used_percentage": 18, "resets_at": 1800345600}
    },
    "cost": {"total_cost_usd": 1.23, "total_duration_ms": 3600000}
}"#;

const FRESH: &str = r#"{
    "session_id": "preview",
    "model": {"display_name": "Opus 5.5"},
    "workspace": {"current_dir": "/home/you/my-project"},
    "context_window": {"used_percentage": null, "context_window_size": 200000},
    "cost": {"total_cost_usd": 0, "total_duration_ms": 4000}
}"#;

const NEAR_THE_LIMITS: &str = r#"{
    "session_id": "preview",
    "model": {"display_name": "Opus 5.5"},
    "workspace": {"current_dir": "/home/you/my-project"},
    "context_window": {"used_percentage": 91, "total_input_tokens": 182000, "context_window_size": 200000},
    "rate_limits": {
        "five_hour": {"used_percentage": 96, "resets_at": 1800001800},
        "seven_day": {"used_percentage": 83, "resets_at": 1800172800}
    },
    "cost": {"total_cost_usd": 14.2, "total_duration_ms": 14400000}
}"#;

const NO_SUBSCRIPTION: &str = r#"{
    "session_id": "preview",
    "model": {"display_name": "Opus 5.5"},
    "workspace": {"current_dir": "/home/you/my-project"},
    "context_window": {"used_percentage": 30, "total_input_tokens": 60000, "context_window_size": 200000},
    "cost": {"total_cost_usd": 3.4, "total_duration_ms": 7200000}
}"#;

pub const SCENARIOS: [Scenario; 6] = [
    Scenario {
        name: "Mid-session",
        session: MID_SESSION,
        branch: Some("main"),
    },
    Scenario {
        name: "Fresh session",
        session: FRESH,
        branch: Some("main"),
    },
    Scenario {
        name: "Near the limits",
        session: NEAR_THE_LIMITS,
        branch: Some("feat/long-branch-name"),
    },
    Scenario {
        name: "No subscription",
        session: NO_SUBSCRIPTION,
        branch: Some("main"),
    },
    Scenario {
        name: "Detached HEAD",
        session: MID_SESSION,
        branch: Some("3f2a91c"),
    },
    Scenario {
        name: "Outside git",
        session: MID_SESSION,
        branch: None,
    },
];

/// The status line for `scenario` drawn with `config`, with ANSI color, as Claude Code shows it in
/// a terminal `columns` wide.
pub fn lines(config: &Config, scenario: &Scenario, columns: usize) -> Vec<String> {
    let session = Session::parse(scenario.session.as_bytes());
    let history = history(&session);
    let env = Env {
        session: &session,
        config,
        now: Some(NOW),
        history: &history,
        branch: scenario.branch,
    };
    segments::lines(&env, Some(render::max_width(columns)), true)
}

/// A made-up history for graphs and predictions: half an hour of steady growth up to the
/// session's current values.
fn history(session: &Session) -> Vec<Sample> {
    const SAMPLES: i64 = 12;
    let now = Sample::new(session, NOW);
    (0..SAMPLES)
        .map(|i| {
            let share = 0.1 + 0.9 * i as f64 / (SAMPLES - 1) as f64;
            let scale = |value: Option<f64>| value.map(|value| value * share);
            Sample {
                t: NOW - 150 * (SAMPLES - 1 - i),
                context: scale(now.context),
                five_hour: scale(now.five_hour),
                seven_day: scale(now.seven_day),
                cost: scale(now.cost),
            }
        })
        .collect()
}

/// A line with the ANSI escapes the renderer writes turned into ratatui styles.
pub fn to_line(text: &str) -> Line<'static> {
    let mut spans = Vec::new();
    let mut style = TuiStyle::default();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(escape) = rest.strip_prefix("\x1b[")
            && let Some(end) = escape.find('m')
        {
            style = apply_sgr(style, &escape[..end]);
            rest = &escape[end + 1..];
            continue;
        }
        // Past the first character, so a lone escape character can't stop the loop.
        let first = rest.chars().next().map_or(0, char::len_utf8);
        let end = rest[first..].find('\x1b').map_or(rest.len(), |i| i + first);
        spans.push(Span::styled(rest[..end].to_string(), style));
        rest = &rest[end..];
    }
    Line::from(spans)
}

/// `style` with an SGR escape's parameters, such as `1;38;5;208`, applied.
fn apply_sgr(mut style: TuiStyle, params: &str) -> TuiStyle {
    let codes: Vec<u8> = params.split(';').filter_map(|p| p.parse().ok()).collect();
    let mut i = 0;
    while i < codes.len() {
        match codes[i..] {
            [0, ..] => style = TuiStyle::default(),
            [1, ..] => style = style.add_modifier(Modifier::BOLD),
            [2, ..] => style = style.add_modifier(Modifier::DIM),
            [3, ..] => style = style.add_modifier(Modifier::ITALIC),
            [4, ..] => style = style.add_modifier(Modifier::UNDERLINED),
            [code @ 30..=37, ..] => style = style.fg(Color::Indexed(code - 30)),
            [code @ 90..=97, ..] => style = style.fg(Color::Indexed(code - 90 + 8)),
            [38, 5, index, ..] => {
                style = style.fg(Color::Indexed(index));
                i += 2;
            }
            [38, 2, r, g, b, ..] => {
                style = style.fg(Color::Rgb(r, g, b));
                i += 4;
            }
            _ => {}
        }
        i += 1;
    }
    style
}

/// Characters in `text` that terminals and fonts draw at different widths, so a line with them can
/// shift or wrap: private-use glyphs such as Nerd Font icons, and the selector and joiner that
/// build emoji sequences.
pub fn uncertain_width(text: &str) -> Vec<char> {
    let mut found = Vec::new();
    for c in text.chars() {
        let uncertain = matches!(
            c as u32,
            0xE000..=0xF8FF | 0xF0000..=0x10FFFD | 0xFE0F | 0x200D
        );
        if uncertain && !found.contains(&c) {
            found.push(c);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    fn plain(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn every_sample_session_parses_and_draws() {
        for scenario in &SCENARIOS {
            let session = Session::parse(scenario.session.as_bytes());
            assert!(session.model.is_some(), "{}", scenario.name);
            assert!(!lines(&Config::default(), scenario, 200).is_empty());
        }
    }

    #[test]
    fn draws_what_each_session_has() {
        let config = config::parse(
            "format.five_hour = \"5h {percent}[ ⚠ {eta}]\"\nformat.context = \"ctx {percent} {graph}\"",
        )
        .unwrap();
        let line = |index: usize| {
            let lines = lines(&config, &SCENARIOS[index], 200);
            plain(&to_line(&lines.join("\n")))
        };
        assert_eq!(
            line(0),
            "Opus 5.5 │ my-project │ main │ ctx 42% ▂▂▂▃▃▃▄▄ │ 5h 35% │ 7d 18% │ $1.23"
        );
        assert!(!line(1).contains("ctx"), "{}", line(1));
        assert!(line(2).contains("⚠"), "{}", line(2));
        assert!(!line(3).contains("5h"), "{}", line(3));
        assert!(line(4).contains("3f2a91c"));
        assert!(!line(5).contains("main"));
    }

    #[test]
    fn fits_the_preview_to_the_terminal_width() {
        let config = config::parse("segments = [\"model\", \"directory\", \"context\"]").unwrap();
        let line = |columns| plain(&to_line(&lines(&config, &SCENARIOS[0], columns)[0]));
        // 31 columns of text, and Claude Code keeps 4 for its edges.
        assert_eq!(line(35), "Opus 5.5 │ my-project │ ctx 42%");
        assert_eq!(line(34), "Opus 5.5 │ ctx 42%");
    }

    #[test]
    fn turns_escapes_into_styles() {
        let line = to_line(
            "\x1b[1;36mOpus\x1b[0m\x1b[2m │ \x1b[0m\x1b[38;2;255;135;0mx\x1b[0m plain\x1b[38;5;208my",
        );
        let styles: Vec<(String, TuiStyle)> = line
            .spans
            .iter()
            .map(|span| (span.content.to_string(), span.style))
            .collect();
        assert_eq!(
            styles,
            [
                (
                    "Opus".to_string(),
                    TuiStyle::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::Indexed(6))
                ),
                (
                    " │ ".to_string(),
                    TuiStyle::default().add_modifier(Modifier::DIM)
                ),
                (
                    "x".to_string(),
                    TuiStyle::default().fg(Color::Rgb(255, 135, 0))
                ),
                (" plain".to_string(), TuiStyle::default()),
                ("y".to_string(), TuiStyle::default().fg(Color::Indexed(208))),
            ]
        );
        assert_eq!(plain(&to_line("\x1b[97mz")), "z");
        assert_eq!(
            to_line("\x1b[97mz").spans[0].style.fg,
            Some(Color::Indexed(15))
        );
        // A stray escape is kept as text rather than looping.
        assert_eq!(plain(&to_line("a\x1bb")), "a\x1bb");
        assert_eq!(plain(&to_line("\u{e0a0} 日\x1b[1m本")), "\u{e0a0} 日本");
    }

    #[test]
    fn flags_characters_of_uncertain_width() {
        assert_eq!(uncertain_width("ctx │ █░ ▁▂ ⚠ 日本"), Vec::<char>::new());
        assert_eq!(
            uncertain_width("\u{e0a0} main \u{e0a0} ⚠\u{fe0f} 👩\u{200d}💻"),
            ['\u{e0a0}', '\u{fe0f}', '\u{200d}']
        );
    }
}

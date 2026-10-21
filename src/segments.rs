//! The segments of the status line, in display order. Each is left out when the session lacks
//! its data.

use crate::render::{Color, Segment};
use crate::session::Session;

pub fn all(session: &Session) -> Vec<Segment> {
    [
        model(session),
        directory(session),
        context(session),
        five_hour_limit(session),
        seven_day_limit(session),
        cost(session),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn model(session: &Session) -> Option<Segment> {
    let name = session.model.as_ref()?.display_name.as_deref()?;
    Some(Segment::new(name, Color::Cyan, 90))
}

fn directory(session: &Session) -> Option<Segment> {
    let name = session.current_dir()?.file_name()?.to_string_lossy();
    Some(Segment::new(name, Color::Blue, 80))
}

fn context(session: &Session) -> Option<Segment> {
    let used = session.context_window.as_ref()?.used_percentage?;
    Some(percentage("ctx", used, 100))
}

fn five_hour_limit(session: &Session) -> Option<Segment> {
    let used = session
        .rate_limits
        .as_ref()?
        .five_hour
        .as_ref()?
        .used_percentage?;
    Some(percentage("5h", used, 60))
}

fn seven_day_limit(session: &Session) -> Option<Segment> {
    let used = session
        .rate_limits
        .as_ref()?
        .seven_day
        .as_ref()?
        .used_percentage?;
    Some(percentage("7d", used, 50))
}

fn cost(session: &Session) -> Option<Segment> {
    let usd = session.cost.as_ref()?.total_cost_usd?;
    Some(Segment::new(format!("${usd:.2}"), Color::Dim, 40))
}

/// A labeled percentage, green below half, yellow below 80%, and red from there up.
fn percentage(label: &str, used: f64, priority: u8) -> Segment {
    let color = match used {
        u if u < 50.0 => Color::Green,
        u if u < 80.0 => Color::Yellow,
        _ => Color::Red,
    };
    // `as` saturates, so a negative or out-of-range value can't print as "-0" or wrap.
    let rounded = used.round().max(0.0) as u64;
    Segment::new(format!("{label} {rounded}%"), color, priority)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(input: &str) -> Vec<Segment> {
        all(&Session::parse(input.as_bytes()))
    }

    fn texts(input: &str) -> Vec<String> {
        segments(input).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn shows_each_documented_field_in_order() {
        let input = r#"{
            "model": {"id": "claude-opus-5-5", "display_name": "Opus"},
            "workspace": {"current_dir": "/home/me/proj", "project_dir": "/home/me/proj"},
            "context_window": {"used_percentage": 8, "context_window_size": 200000},
            "rate_limits": {
                "five_hour": {"used_percentage": 23.5, "resets_at": 1738425600},
                "seven_day": {"used_percentage": 41.2, "resets_at": 1738857600}
            },
            "cost": {"total_cost_usd": 0.01234, "total_duration_ms": 45000}
        }"#;
        assert_eq!(
            texts(input),
            ["Opus", "proj", "ctx 8%", "5h 24%", "7d 41%", "$0.01"]
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
            ))[0]
                .color
        };
        assert_eq!(color("0"), Color::Green);
        assert_eq!(color("49.9"), Color::Green);
        assert_eq!(color("50"), Color::Yellow);
        assert_eq!(color("79.9"), Color::Yellow);
        assert_eq!(color("80"), Color::Red);
        assert_eq!(color("100"), Color::Red);
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

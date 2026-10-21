//! The segments of the status line, in display order. Each is left out when the session lacks
//! its data.

use crate::render::{Color, Segment};
use crate::session::Session;

pub fn all(session: &Session) -> Vec<Segment> {
    [model(session), directory(session)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(input: &str) -> Vec<String> {
        all(&Session::parse(input.as_bytes()))
            .into_iter()
            .map(|s| s.text)
            .collect()
    }

    #[test]
    fn shows_the_model_and_directory_name() {
        let input = r#"{"model": {"display_name": "Opus"}, "cwd": "/home/me/proj"}"#;
        assert_eq!(texts(input), ["Opus", "proj"]);
    }

    #[test]
    fn leaves_out_segments_without_data() {
        assert!(texts("{}").is_empty());
        assert_eq!(texts(r#"{"cwd": "/"}"#), Vec::<String>::new());
        assert_eq!(texts(r#"{"model": {"display_name": "Opus"}}"#), ["Opus"]);
    }
}

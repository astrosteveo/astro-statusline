//! Lays segments out as one line of the status line.
//!
//! Segment text has its control characters stripped, so a stray escape or newline in a model name
//! or path can't bleed color or add a row. When the line is too wide, the lowest-priority segments
//! go first, and a lone segment that still doesn't fit is cut short with an ellipsis.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const SEPARATOR: &str = " │ ";
const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Red,
    Green,
    Yellow,
    Blue,
    Cyan,
    Dim,
}

impl Color {
    fn sgr(self) -> &'static str {
        match self {
            Color::Red => "31",
            Color::Green => "32",
            Color::Yellow => "33",
            Color::Blue => "34",
            Color::Cyan => "36",
            Color::Dim => "2",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub color: Color,
    /// When the line is too wide, lower priorities are dropped first.
    pub priority: u8,
}

impl Segment {
    pub fn new(text: impl Into<String>, color: Color, priority: u8) -> Self {
        Segment {
            text: text.into(),
            color,
            priority,
        }
    }
}

/// Renders `segments` in order, no wider than `max_width` columns when it is given, with ANSI
/// color when `color` is set.
pub fn render(segments: Vec<Segment>, max_width: Option<usize>, color: bool) -> String {
    let mut segments: Vec<Segment> = segments
        .into_iter()
        .map(|s| Segment {
            text: s.text.chars().filter(|c| !c.is_control()).collect(),
            ..s
        })
        .filter(|s| !s.text.is_empty())
        .collect();
    if let Some(max_width) = max_width {
        fit(&mut segments, max_width);
    }

    let separator = paint(SEPARATOR, Color::Dim, color);
    segments
        .iter()
        .map(|s| paint(&s.text, s.color, color))
        .collect::<Vec<_>>()
        .join(&separator)
}

fn fit(segments: &mut Vec<Segment>, max_width: usize) {
    while segments.len() > 1 && line_width(segments) > max_width {
        // `min_by_key` keeps the first minimum, so walking backwards drops the rightmost of a tie.
        if let Some((index, _)) = segments
            .iter()
            .enumerate()
            .rev()
            .min_by_key(|(_, s)| s.priority)
        {
            segments.remove(index);
        }
    }
    if let [only] = segments.as_mut_slice()
        && only.text.width() > max_width
    {
        only.text = truncate(&only.text, max_width);
    }
}

fn line_width(segments: &[Segment]) -> usize {
    let text: usize = segments.iter().map(|s| s.text.width()).sum();
    text + SEPARATOR.width() * segments.len().saturating_sub(1)
}

/// Cuts `text` to at most `max_width` columns, ending in an ellipsis.
fn truncate(text: &str, max_width: usize) -> String {
    let Some(budget) = max_width.checked_sub(1) else {
        return String::new();
    };
    let mut width = 0;
    let mut out: String = text
        .chars()
        .take_while(|c| {
            width += c.width().unwrap_or(0);
            width <= budget
        })
        .collect();
    out.push('…');
    out
}

fn paint(text: &str, color: Color, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{}m{text}{RESET}", color.sgr())
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, priority: u8) -> Segment {
        Segment::new(text, Color::Cyan, priority)
    }

    #[test]
    fn joins_segments_with_a_separator() {
        let line = render(vec![seg("Opus", 1), seg("proj", 2)], None, false);
        assert_eq!(line, "Opus │ proj");
    }

    #[test]
    fn closes_every_color_it_opens() {
        let line = render(
            vec![seg("Opus", 1), Segment::new("proj", Color::Blue, 2)],
            None,
            true,
        );
        assert_eq!(
            line,
            "\x1b[36mOpus\x1b[0m\x1b[2m │ \x1b[0m\x1b[34mproj\x1b[0m"
        );
    }

    #[test]
    fn strips_control_characters_and_skips_empty_segments() {
        let line = render(
            vec![seg("a\x1b[31mb\nc\t", 1), seg("", 2), seg("\x07", 3)],
            None,
            false,
        );
        assert_eq!(line, "a[31mbc");
        assert_eq!(render(Vec::new(), Some(80), true), "");
    }

    #[test]
    fn drops_the_lowest_priority_until_the_line_fits() {
        let segments = vec![seg("aaaa", 3), seg("bbbb", 1), seg("cccc", 2)];
        assert_eq!(
            render(segments.clone(), Some(18), false),
            "aaaa │ bbbb │ cccc"
        );
        assert_eq!(render(segments.clone(), Some(17), false), "aaaa │ cccc");
        assert_eq!(render(segments, Some(10), false), "aaaa");
    }

    #[test]
    fn drops_the_rightmost_of_equal_priorities() {
        let line = render(vec![seg("aaaa", 1), seg("bbbb", 1)], Some(10), false);
        assert_eq!(line, "aaaa");
    }

    #[test]
    fn truncates_a_lone_segment_by_display_width() {
        assert_eq!(render(vec![seg("abcdef", 1)], Some(4), false), "abc…");
        // Each of these characters is two columns wide.
        assert_eq!(render(vec![seg("日本語", 1)], Some(4), false), "日…");
        assert_eq!(render(vec![seg("日本語", 1)], Some(5), false), "日本…");
        assert_eq!(render(vec![seg("abc", 1)], Some(1), false), "…");
        assert_eq!(render(vec![seg("abc", 1)], Some(0), false), "");
    }

    #[test]
    fn color_does_not_count_toward_width() {
        let line = render(vec![seg("abcd", 1)], Some(4), true);
        assert_eq!(line, "\x1b[36mabcd\x1b[0m");
    }
}

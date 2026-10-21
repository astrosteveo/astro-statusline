//! Lays segments out as one line of the status line.
//!
//! Segment text has its control characters stripped, so a stray escape or newline in a model name
//! or path can't bleed color or add a row. When the line is too wide, the lowest-priority segments
//! go first, and a lone segment that still doesn't fit is cut short with an ellipsis.

use unicode_width::UnicodeWidthStr;

use crate::style::Style;

const SEPARATOR: &str = " │ ";
const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub style: Style,
    /// When the line is too wide, lower priorities are dropped first.
    pub priority: u8,
}

impl Segment {
    pub fn new(text: impl Into<String>, style: Style, priority: u8) -> Self {
        Segment {
            text: text.into(),
            style,
            priority,
        }
    }
}

/// Renders `segments` in order, joined by a separator drawn in `separator`, no wider than
/// `max_width` columns when it is given, with ANSI color when `color` is set.
pub fn render(
    segments: Vec<Segment>,
    separator: &Style,
    max_width: Option<usize>,
    color: bool,
) -> String {
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

    let separator = paint(SEPARATOR, separator, color);
    segments
        .iter()
        .map(|s| paint(&s.text, &s.style, color))
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
    let mut out = String::new();
    for c in text.chars() {
        let start = out.len();
        out.push(c);
        if out.width() > budget {
            out.truncate(start);
            break;
        }
    }
    out.push('…');
    out
}

fn paint(text: &str, style: &Style, enabled: bool) -> String {
    match style.escape() {
        Some(escape) if enabled => format!("{escape}{text}{RESET}"),
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(text: &str) -> Style {
        text.parse().unwrap()
    }

    fn seg(text: &str, priority: u8) -> Segment {
        Segment::new(text, style("cyan"), priority)
    }

    fn render(segments: Vec<Segment>, max_width: Option<usize>, color: bool) -> String {
        super::render(segments, &style("dim"), max_width, color)
    }

    #[test]
    fn joins_segments_with_a_separator() {
        let line = render(vec![seg("Opus", 1), seg("proj", 2)], None, false);
        assert_eq!(line, "Opus │ proj");
    }

    #[test]
    fn closes_every_color_it_opens() {
        let line = render(
            vec![seg("Opus", 1), Segment::new("proj", style("blue"), 2)],
            None,
            true,
        );
        assert_eq!(
            line,
            "\x1b[36mOpus\x1b[0m\x1b[2m │ \x1b[0m\x1b[34mproj\x1b[0m"
        );
    }

    #[test]
    fn leaves_plain_styles_unescaped() {
        let segments = vec![Segment::new("Opus", style(""), 1), seg("proj", 2)];
        let line = super::render(segments, &style(""), None, true);
        assert_eq!(line, "Opus │ \x1b[36mproj\x1b[0m");
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

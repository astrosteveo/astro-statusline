//! Lays segments out as one line of the status line.
//!
//! Segment and separator text have their control characters stripped, so a stray escape or newline
//! in a model name, path, or config can't bleed color or add a row. When the line is too wide, the lowest-priority segments
//! go first, and a lone segment that still doesn't fit is cut short with an ellipsis.

use unicode_width::UnicodeWidthStr;

use crate::style::Style;

const RESET: &str = "\x1b[0m";

/// Claude Code draws the status line two columns in from each edge (measured in fullscreen
/// rendering), and cuts off anything wider. A `padding` set in settings takes that many more at
/// each edge (measured with `padding = 3` in a 60-column terminal: 50 columns were left).
pub const EDGE_RESERVE: usize = 4;

/// The widest a line may be in a terminal `columns` wide, with Claude Code's `padding`.
pub fn max_width(columns: usize, padding: u16) -> usize {
    columns.saturating_sub(EDGE_RESERVE + 2 * usize::from(padding))
}

/// Text drawn in one style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub pieces: Vec<Piece>,
    /// When the line is too wide, lower priorities are dropped first.
    pub priority: u8,
    /// The most columns the segment may take, whatever room the line has.
    pub max_width: Option<u16>,
}

impl Segment {
    /// A segment drawn in one style.
    #[cfg(test)]
    pub fn new(text: impl Into<String>, style: Style, priority: u8) -> Self {
        Segment::from_pieces([(text.into(), style)], priority)
    }

    /// The style of the segment's first piece.
    #[cfg(test)]
    pub fn style(&self) -> Style {
        self.pieces[0].style.clone()
    }

    pub fn from_pieces(pieces: impl IntoIterator<Item = (String, Style)>, priority: u8) -> Self {
        let pieces = pieces
            .into_iter()
            .map(|(text, style)| Piece { text, style })
            .collect();
        Segment {
            pieces,
            priority,
            max_width: None,
        }
    }

    /// The segment cut to at most `max_width` columns when it's drawn.
    pub fn capped(self, max_width: Option<u16>) -> Self {
        Segment { max_width, ..self }
    }

    /// The segment's text, without its styles.
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.pieces
            .iter()
            .map(|piece| piece.text.as_str())
            .collect()
    }

    fn width(&self) -> usize {
        self.pieces.iter().map(|piece| piece.text.width()).sum()
    }

    /// The segment with control characters stripped, empty pieces dropped, neighbours of the
    /// same style joined, so each style is turned on once, and cut to its `max_width`.
    fn cleaned(self) -> Segment {
        let mut pieces: Vec<Piece> = Vec::new();
        for piece in self.pieces {
            let text = strip_controls(&piece.text);
            match pieces.last_mut() {
                _ if text.is_empty() => {}
                Some(last) if last.style == piece.style => last.text.push_str(&text),
                _ => pieces.push(Piece { text, ..piece }),
            }
        }
        let mut segment = Segment { pieces, ..self };
        if let Some(max_width) = segment.max_width.map(usize::from)
            && segment.width() > max_width
        {
            truncate(&mut segment.pieces, max_width);
        }
        segment
    }
}

/// Renders `segments` in order, joined by `separator` drawn in `separator_style`, no wider than
/// `max_width` columns when it is given, with ANSI color when `color` is set.
#[cfg(test)]
pub fn render(
    segments: Vec<Segment>,
    separator: &str,
    separator_style: &Style,
    max_width: Option<usize>,
    color: bool,
) -> String {
    render_row(segments, None, separator, separator_style, max_width, color)
}

/// Renders a row as `render` does, with `right`, when there's a `fill`, pushed to the right edge of
/// `max_width`. Both sides are fitted as one line, the fill counted as a separator, so the gap is
/// never narrower than one. Without a `max_width` the fill is drawn as a separator.
pub fn render_row(
    left: Vec<Segment>,
    right: Option<Vec<Segment>>,
    separator: &str,
    separator_style: &Style,
    max_width: Option<usize>,
    color: bool,
) -> String {
    let cleaned = |segments: Vec<Segment>| -> Vec<Segment> {
        segments
            .into_iter()
            .map(Segment::cleaned)
            .filter(|s| !s.pieces.is_empty())
            .collect()
    };
    let mut segments = cleaned(left);
    let mut boundary = segments.len();
    let filled = right.is_some();
    segments.extend(cleaned(right.unwrap_or_default()));
    let separator = strip_controls(separator);
    let separator_width = separator.width();
    if let Some(max_width) = max_width {
        fit(&mut segments, separator_width, max_width, &mut boundary);
    }

    let separator = paint(&separator, separator_style, color);
    let join = |segments: &[Segment]| {
        segments
            .iter()
            .map(|s| {
                s.pieces
                    .iter()
                    .map(|piece| paint(&piece.text, &piece.style, color))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(&separator)
    };
    let (left, right) = segments.split_at(boundary);
    match max_width {
        Some(max_width) if filled && !right.is_empty() => {
            let used = line_width(left, separator_width) + line_width(right, separator_width);
            let gap = " ".repeat(max_width.saturating_sub(used));
            format!("{}{gap}{}", join(left), join(right))
        }
        _ => join(&segments),
    }
}

fn strip_controls(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// Drops segments until the line fits, keeping `boundary`, the index of a fill, in place.
fn fit(
    segments: &mut Vec<Segment>,
    separator_width: usize,
    max_width: usize,
    boundary: &mut usize,
) {
    while segments.len() > 1 && line_width(segments, separator_width) > max_width {
        // `min_by_key` keeps the first minimum, so walking backwards drops the rightmost of a tie.
        if let Some((index, _)) = segments
            .iter()
            .enumerate()
            .rev()
            .min_by_key(|(_, s)| s.priority)
        {
            segments.remove(index);
            if index < *boundary {
                *boundary -= 1;
            }
        }
    }
    if let [only] = segments.as_mut_slice()
        && only.width() > max_width
    {
        truncate(&mut only.pieces, max_width);
    }
}

fn line_width(segments: &[Segment], separator_width: usize) -> usize {
    let text: usize = segments.iter().map(Segment::width).sum();
    text + separator_width * segments.len().saturating_sub(1)
}

/// Cuts `pieces` to at most `max_width` columns, ending in an ellipsis in the style of the piece
/// it cuts.
fn truncate(pieces: &mut Vec<Piece>, max_width: usize) {
    let Some(budget) = max_width.checked_sub(1) else {
        pieces.clear();
        return;
    };
    let mut used = 0;
    let mut kept = Vec::new();
    for mut piece in pieces.drain(..) {
        let width = piece.text.width();
        if used + width <= budget {
            used += width;
            kept.push(piece);
            continue;
        }
        piece.text = cut(&piece.text, budget - used);
        kept.push(piece);
        break;
    }
    if let Some(last) = kept.last_mut() {
        last.text.push('…');
    }
    *pieces = kept;
}

/// The start of `text` that fits in `width` columns.
fn cut(text: &str, width: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let start = out.len();
        out.push(c);
        if out.width() > width {
            out.truncate(start);
            break;
        }
    }
    out
}

fn paint(text: &str, style: &Style, enabled: bool) -> String {
    match style.escape() {
        Some(escape) if enabled && !text.is_empty() => format!("{escape}{text}{RESET}"),
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
        super::render(segments, " │ ", &style("dim"), max_width, color)
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
        let line = super::render(segments, " │ ", &style(""), None, true);
        assert_eq!(line, "Opus │ \x1b[36mproj\x1b[0m");
    }

    #[test]
    fn fits_with_the_separator_it_is_given() {
        let segments = vec![seg("aaaa", 2), seg("bbbb", 1)];
        let line = super::render(segments.clone(), " · ", &style("dim"), Some(11), false);
        assert_eq!(line, "aaaa · bbbb");
        let line = super::render(segments.clone(), " -- ", &style("dim"), Some(11), false);
        assert_eq!(line, "aaaa");
        // Control characters can't sneak in through the config either.
        let line = super::render(segments.clone(), "\x1b[31m|\n", &style(""), None, false);
        assert_eq!(line, "aaaa[31m|bbbb");
        // An empty separator draws no escape at all.
        let line = super::render(segments, "", &style("dim"), Some(8), true);
        assert_eq!(line, "\x1b[36maaaa\x1b[0m\x1b[36mbbbb\x1b[0m");
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
    fn draws_each_piece_in_its_style_and_joins_neighbours_of_the_same() {
        let pieces = |list: &[(&str, &str)]| {
            Segment::from_pieces(list.iter().map(|(text, s)| (text.to_string(), style(s))), 1)
        };
        let segment = pieces(&[
            ("ctx ", "dim"),
            ("42", "green"),
            ("%", "green"),
            ("", "red"),
        ]);
        assert_eq!(
            render(vec![segment], None, true),
            "\x1b[2mctx \x1b[0m\x1b[32m42%\x1b[0m"
        );
        let segment = pieces(&[("a\x1b", "dim"), ("\x07", "red"), ("b", "dim")]);
        assert_eq!(render(vec![segment], None, true), "\x1b[2mab\x1b[0m");
        // A segment of empty pieces is left out.
        let empty = pieces(&[("", "dim")]);
        assert_eq!(render(vec![empty, seg("x", 1)], None, false), "x");
    }

    #[test]
    fn pushes_what_follows_a_fill_to_the_right_edge() {
        let row = |left: Vec<Segment>, right: Vec<Segment>, max_width, color| {
            super::render_row(left, Some(right), " │ ", &style("dim"), max_width, color)
        };
        let (a, b, c) = (seg("aaaa", 3), seg("bbbb", 2), seg("cc", 1));
        assert_eq!(
            row(vec![a.clone()], vec![b.clone()], Some(12), false),
            "aaaa    bbbb"
        );
        // "aaaa │ cc" is 9 columns and "bbbb" 4, so 20 leave 7 between.
        assert_eq!(
            row(vec![a.clone(), c.clone()], vec![b.clone()], Some(20), false),
            format!("aaaa │ cc{}bbbb", " ".repeat(7))
        );
        // At 16 the gap is exactly a separator's width. At 15 it would be narrower, so the lowest
        // priority drops.
        assert_eq!(
            row(vec![a.clone(), c.clone()], vec![b.clone()], Some(16), false),
            format!("aaaa │ cc{}bbbb", " ".repeat(3))
        );
        assert_eq!(
            row(vec![a.clone(), c.clone()], vec![b.clone()], Some(15), false),
            format!("aaaa{}bbbb", " ".repeat(7))
        );
        // Without a width to fill, the fill is a separator.
        assert_eq!(
            row(vec![a.clone()], vec![b.clone()], None, false),
            "aaaa │ bbbb"
        );
        // Nothing on the left: the right side sits at the edge. Nothing on the right: no padding.
        assert_eq!(row(vec![], vec![b.clone()], Some(8), false), "    bbbb");
        assert_eq!(row(vec![a.clone()], vec![], Some(8), false), "aaaa");
        // The padding is plain, outside any color.
        assert_eq!(
            row(vec![a], vec![b], Some(11), true),
            "\x1b[36maaaa\x1b[0m   \x1b[36mbbbb\x1b[0m"
        );
    }

    #[test]
    fn truncates_across_pieces_in_the_style_of_the_piece_it_cuts() {
        let segment = Segment::from_pieces(
            [
                ("ctx ".to_string(), style("dim")),
                ("4242".to_string(), style("red")),
            ],
            1,
        );
        assert_eq!(
            render(vec![segment.clone()], Some(6), true),
            "\x1b[2mctx \x1b[0m\x1b[31m4…\x1b[0m"
        );
        assert_eq!(render(vec![segment.clone()], Some(5), false), "ctx …");
        assert_eq!(render(vec![segment], Some(3), true), "\x1b[2mct…\x1b[0m");
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

//! Drawing the editor: the preview on top, the settings list on the left, the chosen settings on
//! the right, key hints at the bottom, and pop-ups for styles and questions.

use std::env;
use std::path::Path;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::doc::Field;
use super::preview::{self, SCENARIOS};
use super::{
    App, Confirm, Focus, GRID_COLUMNS, LayoutItem, Mode, Picker, SECTIONS, Section, claude,
    layout_items,
};
use crate::config::Config;
use crate::segments::Kind;

const MIN_WIDTH: u16 = 72;
const MIN_HEIGHT: u16 = 20;
/// The width of a field's label in the right-hand pane.
const LABEL_WIDTH: usize = 13;

fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn red() -> Style {
    Style::default().fg(Color::Red)
}

fn highlight(focused: bool) -> Style {
    if focused {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        bold()
    }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let text = format!(
            "Make the terminal at least {MIN_WIDTH}×{MIN_HEIGHT} to edit the config (it's {}×{}). \
             q quits.",
            area.width, area.height
        );
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    }

    let (config, problem) = match app.preview_config() {
        Ok(config) => (config, None),
        Err(error) => (Config::default(), Some(error)),
    };
    let columns = app.preview_columns();
    let lines = preview::lines(&config, &SCENARIOS[app.scenario], columns);
    let uncertain = preview::uncertain_width(&lines.concat());
    let notes = usize::from(problem.is_some()) + usize::from(!uncertain.is_empty());
    let preview_height = (lines.len().max(1) + notes + 2) as u16;

    let [top, middle, footer] = Layout::vertical([
        Constraint::Length(preview_height),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_preview(frame, app, top, &lines, problem, &uncertain);
    let [left, right] =
        Layout::horizontal([Constraint::Length(20), Constraint::Fill(1)]).areas(middle);
    draw_sections(frame, app, left);
    draw_detail(frame, app, right);
    draw_footer(frame, app, footer);
    match &app.mode {
        Mode::Style(picker) => draw_picker(frame, app, middle, picker),
        Mode::Confirm(confirm) => draw_confirm(frame, app, area, confirm),
        Mode::Normal | Mode::Edit(_) => {}
    }
}

fn draw_preview(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    lines: &[String],
    problem: Option<String>,
    uncertain: &[char],
) {
    let columns = app.preview_columns();
    let block = Block::bordered()
        .title(format!(
            " Preview · {} · {columns} columns ",
            SCENARIOS[app.scenario].name
        ))
        .title(Line::from(" [ ] session · < > width ").right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut text: Vec<Line> = if lines.is_empty() {
        vec![Line::styled("  (nothing to show in this session)", dim())]
    } else {
        lines
            .iter()
            .map(|line| {
                // Claude Code draws the line two columns in.
                let mut line = preview::to_line(line);
                line.spans.insert(0, Span::raw("  "));
                line
            })
            .collect()
    };
    if let Some(problem) = problem {
        text.push(Line::styled(
            format!("Claude Code shows the defaults: {problem}"),
            red(),
        ));
    }
    if !uncertain.is_empty() {
        let list: Vec<String> = uncertain
            .iter()
            .map(|&c| match c as u32 {
                0xFE0F | 0x200D => format!("U+{:04X}", c as u32),
                code => format!("{c} U+{code:04X}"),
            })
            .collect();
        text.push(Line::styled(
            format!(
                "⚠ {}: terminals and fonts disagree on how wide these are, so the line can \
                 shift or wrap",
                list.join(", ")
            ),
            Style::default().fg(Color::Yellow),
        ));
    }
    frame.render_widget(Paragraph::new(text), inner);

    // The right edge of the terminal the preview stands in for.
    if columns < usize::from(inner.width) {
        let x = inner.x + columns as u16;
        for y in inner.y..inner.y + lines.len().max(1) as u16 {
            frame.buffer_mut()[(x, y)].set_symbol("┊").set_style(dim());
        }
    }
}

fn section_title(section: Section) -> &'static str {
    match section {
        Section::Layout => "Layout",
        Section::DropOrder => "Drop order",
        Section::Segment(kind) => segment_label(kind),
        Section::Separator => "Separator",
        Section::UsageColors => "Usage colors",
        Section::Bar => "Bar",
        Section::Graph => "Graph",
        Section::ClaudeCode => "Claude Code",
    }
}

fn segment_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Model => "Model",
        Kind::Directory => "Directory",
        Kind::Branch => "Branch",
        Kind::Context => "Context",
        Kind::FiveHour => "5-hour limit",
        Kind::SevenDay => "7-day limit",
        Kind::Cost => "Cost",
    }
}

fn draw_sections(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Sections && matches!(app.mode, Mode::Normal);
    let lines: Vec<Line> = SECTIONS
        .iter()
        .enumerate()
        .map(|(i, &section)| {
            let mut spans = vec![Span::raw(format!(" {}", section_title(section)))];
            if section == Section::ClaudeCode
                && !matches!(app.claude_status, claude::Status::Runs(_))
            {
                spans.push(Span::styled(" !", Style::default().fg(Color::Yellow)));
            }
            let style = if i == app.section {
                highlight(focused)
            } else {
                Style::default()
            };
            Line::from(spans).style(style)
        })
        .collect();
    let block = Block::bordered()
        .title(" Settings ")
        .border_style(if focused { bold() } else { dim() });
    // Scrolls so the chosen section stays in view.
    let height = usize::from(block.inner(area).height);
    let offset = app.section.saturating_sub(height.saturating_sub(1));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((offset as u16, 0)),
        area,
    );
}

fn draw_detail(frame: &mut Frame, app: &App, area: Rect) {
    let section = app.section();
    let focused = app.focus == Focus::Detail;
    let block = Block::bordered()
        .title(format!(" {} ", section_title(section)))
        .border_style(if focused { bold() } else { dim() });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    match section {
        Section::Layout => draw_layout(frame, app, inner, focused),
        Section::DropOrder => draw_drop_order(frame, app, inner, focused),
        Section::ClaudeCode => draw_claude(frame, app, inner, focused),
        section => draw_fields(frame, app, inner, section, focused),
    }
}

/// The usage style a sample of a `usage` style is drawn in: the medium one, from the config being
/// edited.
fn usage_sample(app: &App) -> crate::style::Style {
    app.preview_config().unwrap_or_default().colors.usage_medium
}

/// The field's label, in a config with `rows` rows.
fn field_label(field: Field, rows: usize) -> String {
    match field {
        Field::Template(_) => "Template",
        Field::Style("usage_low") => "Low",
        Field::Style("usage_medium") => "Medium",
        Field::Style("usage_high") => "High",
        Field::Thresholds(_) => "Medium, high",
        Field::Style(_) => "Style",
        Field::Separator(_) if rows <= 1 => "Text",
        Field::Separator(row) => return format!("Row {}", row + 1),
        Field::Padding => "Padding",
        Field::ShowFrom(_) => "Show from",
        Field::MaxWidth(_) => "Max width",
        Field::BarWidth | Field::GraphWidth => "Width",
        Field::BarFull => "Full cell",
        Field::BarEmpty => "Empty cell",
        Field::BarMarker => "Pace marker",
        Field::GraphChars => "Cells",
    }
    .to_string()
}

/// A field's value as shown: text in quotes, so spaces at its ends show, and a style with a
/// sample of itself.
/// A field's value as shown, with a style's sample drawn in `usage` where it says usage.
fn value_spans(field: Field, text: &str, usage: &crate::style::Style) -> Vec<Span<'static>> {
    match field {
        Field::BarWidth
        | Field::GraphWidth
        | Field::Padding
        | Field::Thresholds(_)
        | Field::ShowFrom(_)
        | Field::MaxWidth(_) => {
            vec![Span::raw(text.to_string())]
        }
        Field::Style(_) => {
            let mut spans = vec![Span::raw(if text.is_empty() {
                "\"\" (plain)".to_string()
            } else {
                text.to_string()
            })];
            if let Ok(style) = text.parse::<crate::style::Style>() {
                spans.push(Span::raw("   "));
                let style = style.resolve(usage);
                let sample = format!("{}Sample\x1b[0m", style.escape().unwrap_or_default());
                spans.extend(preview::to_line(&sample).spans);
            }
            spans
        }
        _ => vec![Span::raw(format!("\"{text}\""))],
    }
}

fn draw_fields(frame: &mut Frame, app: &App, area: Rect, section: Section, focused: bool) {
    let rows = app.doc.rows().len();
    let fields = section.fields(rows);
    let mut lines: Vec<Line> = Vec::new();
    let mut cursor = None;
    let mut popup_row = None;
    for (i, &field) in fields.iter().enumerate() {
        let label = format!(" {:<LABEL_WIDTH$}", field_label(field, rows));
        let selected =
            focused && i + section.first_field() == app.cursor && matches!(app.mode, Mode::Normal);
        let label = Span::styled(label, if selected { highlight(true) } else { bold() });
        match &app.mode {
            Mode::Edit(edit) if edit.field == field => {
                let text = edit.input.text();
                let before: String = edit.input.chars[..edit.input.cursor].iter().collect();
                let x = area.x + 1 + LABEL_WIDTH as u16 + 1 + 1 + before.width() as u16;
                cursor = Some((x.min(area.right() - 1), area.y + lines.len() as u16));
                popup_row = Some(area.y + lines.len() as u16 + 1);
                lines.push(Line::from(vec![
                    label,
                    Span::raw(" "),
                    Span::styled(format!("\"{text}\""), Style::default().fg(Color::Cyan)),
                ]));
                if let Err(error) = app.doc.clone().put(field, &text) {
                    lines.push(Line::styled(
                        format!("{:LABEL_WIDTH$}   {error}", ""),
                        red(),
                    ));
                }
            }
            _ => {
                let shown = app.doc.get(field);
                let mut spans = vec![label, Span::raw(" ")];
                if field.optional() && !shown.set {
                    let unset = match field {
                        Field::MaxWidth(_) => "no limit",
                        _ => "always",
                    };
                    spans.push(Span::styled(unset, dim()));
                } else {
                    spans.extend(value_spans(field, &shown.text, &usage_sample(app)));
                }
                if !shown.set && !field.optional() {
                    spans.push(Span::styled("  default", dim()));
                }
                lines.push(Line::from(spans));
                if let Some(error) = shown.error {
                    lines.push(Line::styled(
                        format!("{:LABEL_WIDTH$}   {error}", ""),
                        red(),
                    ));
                }
            }
        }
    }

    lines.push(Line::raw(""));
    if let Section::Segment(kind) = section {
        if kind.has_usage() {
            lines.push(Line::styled(
                " Style usage takes the usage color, from Usage colors. Medium, high: the \
                 percentages where the color turns medium, then high.",
                dim(),
            ));
        }
        let unit = match kind {
            Kind::Cost => Some("dollars"),
            Kind::Context | Kind::FiveHour | Kind::SevenDay => Some("percent"),
            Kind::Model | Kind::Directory | Kind::Branch => None,
        };
        if let Some(unit) = unit {
            lines.push(Line::styled(
                format!(
                    " Show from: the segment shows only from this many {unit} up. Leave it \
                     empty to always show it."
                ),
                dim(),
            ));
        }
        let placeholders: Vec<String> = kind
            .placeholders()
            .iter()
            .map(|name| format!("{{{name}}}"))
            .collect();
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<LABEL_WIDTH$} ", "Placeholders"), bold()),
            Span::raw(placeholders.join(" ")),
        ]));
        lines.push(Line::styled(
            " Text in [ ] shows only when every value in it is there. {{ }} [[ ]] write braces \
             and brackets. The README says what each placeholder shows.",
            dim(),
        ));
    }
    if section == Section::Separator && rows > 1 {
        lines.push(Line::styled(
            " Each row can have its own. The same for every row is written as one.",
            dim(),
        ));
    }
    if section == Section::ClaudeCode {
        lines.push(Line::styled(
            " Padding: the same number as the padding in Claude Code's statusLine settings, which \
             takes that many columns at each end of the line.",
            dim(),
        ));
        let ours: u64 = app.doc.get(Field::Padding).text.parse().unwrap_or(0);
        let theirs = app.claude_padding.unwrap_or(0);
        if ours != theirs {
            lines.push(Line::styled(
                format!(
                    " Claude Code's padding is {theirs}, so rows are fitted to the wrong width: \
                     set Padding to {theirs}."
                ),
                Style::default().fg(Color::Yellow),
            ));
        }
    }
    if section == Section::UsageColors {
        lines.push(Line::styled(
            " The context and rate limits take these by how much is used. Each segment sets where \
             they change.",
            dim(),
        ));
    }
    if section == Section::Bar {
        lines.push(Line::styled(
            " {bar} in a template draws this. The pace marker shows on rate limits.",
            dim(),
        ));
    }
    if section == Section::Graph {
        lines.push(Line::styled(
            " {graph} in a template draws this: the latest values, lowest cell first.",
            dim(),
        ));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }

    if let (Mode::Edit(edit), Some(row)) = (&app.mode, popup_row)
        && let (Some(selected), Field::Template(kind)) = (edit.placeholders, edit.field)
    {
        let names = kind.placeholders();
        let height = (names.len() as u16 + 2).min(area.bottom().saturating_sub(row));
        let popup = Rect::new(area.x + LABEL_WIDTH as u16 + 3, row, 16, height);
        let items: Vec<Line> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let style = if i == selected {
                    highlight(true)
                } else {
                    Style::default()
                };
                Line::styled(format!(" {{{name}}}"), style)
            })
            .collect();
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(items).block(Block::bordered().title(" Insert ")),
            popup,
        );
    }
}

fn draw_drop_order(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let sequence = app.doc.drop_sequence();
    let mut lines: Vec<Line> = sequence
        .iter()
        .enumerate()
        .map(|(i, &kind)| {
            let note = match i {
                0 => "  drops first",
                i if i + 1 == sequence.len() => "  drops last",
                _ => "",
            };
            let style = if focused && i == app.cursor.min(sequence.len() - 1) {
                highlight(true)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(format!(" {}. {}", i + 1, segment_label(kind)), style),
                Span::styled(note, dim()),
            ])
        })
        .collect();
    lines.push(Line::raw(""));
    let source = if app.doc.has_drop_order() {
        " Segments named in drop_order go first, and the rest follow in the default order."
    } else {
        " The default order."
    };
    lines.push(Line::styled(
        format!(
            "{source} When a row is wider than the terminal, segments drop from the top of this \
             list until it fits. Try it with < and >."
        ),
        dim(),
    ));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn draw_layout(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let rows = app.doc.rows();
    let items = layout_items(&rows);
    let cursor = app.cursor.min(items.len() - 1);
    let lines: Vec<Line> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let (text, style) = match *item {
                LayoutItem::Row(row) if rows[row].is_empty() => {
                    (format!(" Row {} (empty)", row + 1), bold())
                }
                LayoutItem::Row(row) => (format!(" Row {}", row + 1), bold()),
                LayoutItem::Segment { kind, .. } => {
                    (format!("    {}", segment_label(kind)), Style::default())
                }
                LayoutItem::HiddenHeader => {
                    (" Not shown".to_string(), bold().add_modifier(Modifier::DIM))
                }
                LayoutItem::Hidden(kind) => (format!("    {}", segment_label(kind)), dim()),
            };
            let style = if focused && i == cursor {
                highlight(true)
            } else {
                style
            };
            Line::styled(text, style)
        })
        .collect();
    // Scrolls so the cursor stays in view.
    let height = usize::from(area.height);
    let offset = cursor.saturating_sub(height.saturating_sub(1));
    frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), area);
}

fn draw_claude(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let path = app
        .claude_path
        .as_deref()
        .map_or("(no home directory)".to_string(), tilde);
    let status = match &app.claude_status {
        claude::Status::Runs(command) => Line::from(vec![
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::raw(format!("The status line runs astro-statusline: {command}")),
        ]),
        claude::Status::NotSet => Line::from(vec![
            Span::styled("✗ ", Style::default().fg(Color::Yellow)),
            Span::raw("No status line is set."),
        ]),
        claude::Status::Other(command) => Line::from(vec![
            Span::styled("! ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("The status line runs another command: {command}")),
        ]),
        claude::Status::Unreadable(error) => Line::from(vec![
            Span::styled("✗ ", red()),
            Span::raw(format!("Can't read the settings: {error}")),
        ]),
    };
    let selected = focused && app.cursor == 0 && matches!(app.mode, Mode::Normal);
    let label = Span::styled(
        format!(" {:<LABEL_WIDTH$}", "Status line"),
        if selected { highlight(true) } else { bold() },
    );
    let header = vec![
        Line::from(vec![
            Span::styled(format!(" {:<LABEL_WIDTH$} ", "Settings"), bold()),
            Span::raw(path),
        ]),
        Line::from(
            [label, Span::raw(" ")]
                .into_iter()
                .chain(status.spans)
                .collect::<Vec<_>>(),
        ),
        Line::styled(
            format!(
                " {:LABEL_WIDTH$} Enter sets it to run {}, every second so it fits the terminal \
                 again soon after a resize. It shows what changes before writing.",
                "",
                app.exe.display()
            ),
            dim(),
        ),
    ];
    let height = header
        .iter()
        .map(|line| line_count(line, area.width))
        .sum::<usize>() as u16;
    let [top, rest] =
        Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(area);
    frame.render_widget(Paragraph::new(header).wrap(Wrap { trim: false }), top);
    draw_fields(frame, app, rest, Section::ClaudeCode, focused);
}

/// How many rows `line` takes when wrapped to `width` columns.
fn line_count(line: &Line, width: u16) -> usize {
    let text: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    wrapped_lines(&text, usize::from(width))
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let hints = match (&app.mode, app.focus, app.section()) {
        (Mode::Edit(edit), ..) => match (edit.placeholders, edit.field) {
            (Some(_), _) => "↑↓ choose  enter insert  esc close",
            (None, Field::Template(_)) => "enter keep  esc cancel  tab insert a placeholder",
            (None, _) => "enter keep  esc cancel",
        },
        (Mode::Style(picker), ..) if picker.allows_usage() => {
            "←↑↓→ color  b d i u attributes  U usage  x no color  t type it  enter keep  esc cancel"
        }
        (Mode::Style(_), ..) => {
            "←↑↓→ color  b d i u attributes  x no color  t type it  enter keep  esc cancel"
        }
        (Mode::Confirm(_), ..) => "",
        (Mode::Normal, Focus::Sections, _) => {
            "↑↓ choose  → open  s save  q quit  [ ] session  < > width"
        }
        (Mode::Normal, Focus::Detail, Section::Layout) => {
            "↑↓ choose  J K move  n new row  d remove  enter open  r reset  ← back  s save"
        }
        (Mode::Normal, Focus::Detail, Section::DropOrder) => {
            "↑↓ choose  J K move  r reset to default  ← back  s save  q quit"
        }
        (Mode::Normal, Focus::Detail, Section::ClaudeCode) => {
            "↑↓ choose  enter set up or edit  r reset  ← back  s save  q quit"
        }
        (Mode::Normal, Focus::Detail, _) => {
            "↑↓ choose  enter edit  r reset to default  ← back  s save  q quit"
        }
    };
    let line = match &app.message {
        Some((message, true)) => Line::styled(format!(" {message}"), red()),
        Some((message, false)) => Line::raw(format!(" {message}")),
        None => Line::styled(format!(" {hints}"), dim()),
    };
    let unsaved = if app.dirty() { "● unsaved " } else { "" };
    let [left, right] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(unsaved.width() as u16),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(line), left);
    frame.render_widget(
        Paragraph::new(Span::styled(unsaved, Style::default().fg(Color::Yellow))),
        right,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// Draws the style picker over `area`, which leaves the preview above it in view.
fn draw_picker(frame: &mut Frame, app: &App, area: Rect, picker: &Picker) {
    let popup = centered(area, 70, 13);
    let text = picker.text();
    let mut lines = vec![Line::from(
        [Span::styled(" Style  ", bold())]
            .into_iter()
            .chain(value_spans(picker.field, &text, &usage_sample(app)))
            .collect::<Vec<_>>(),
    )];
    let attributes: Vec<Span> = ["bold", "dim", "italic", "underline"]
        .iter()
        .zip(picker.attributes)
        .flat_map(|(name, on)| {
            let style = if on { highlight(true) } else { dim() };
            [
                Span::raw(" "),
                Span::styled(format!("{} {name}", &name[..1]), style),
            ]
        })
        .collect();
    lines.push(Line::from(attributes));
    for row in 0..256 / u16::from(GRID_COLUMNS) {
        let mut spans = vec![Span::raw("  ")];
        for column in 0..u16::from(GRID_COLUMNS) {
            let index = (row * u16::from(GRID_COLUMNS) + column) as u8;
            let cell = Style::default().bg(Color::Indexed(index));
            spans.push(if index == picker.cursor {
                let ink = if is_dark(index) {
                    Color::White
                } else {
                    Color::Black
                };
                Span::styled("[]", cell.fg(ink).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("  ", cell)
            });
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::styled(
        " The first 16 are your theme's colors. t types any style, as #ff8700",
        dim(),
    ));
    let title = format!(" {} ", field_name(picker.field));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title)),
        popup,
    );
}

/// The field's key, as the config file writes it.
fn field_name(field: Field) -> String {
    let (table, key) = field.path();
    format!("{table}.{key}")
}

/// Whether a 256-color cell is dark enough to need light text on it.
pub(super) fn is_dark(index: u8) -> bool {
    let (r, g, b) = match index {
        // xterm's defaults for the 16 theme colors, which a theme may change.
        0..16 => [
            (0, 0, 0),
            (205, 0, 0),
            (0, 205, 0),
            (205, 205, 0),
            (0, 0, 238),
            (205, 0, 205),
            (0, 205, 205),
            (229, 229, 229),
            (127, 127, 127),
            (255, 0, 0),
            (0, 255, 0),
            (255, 255, 0),
            (92, 92, 255),
            (255, 0, 255),
            (0, 255, 255),
            (255, 255, 255),
        ][usize::from(index)],
        16..232 => {
            let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * u16::from(n) };
            let n = index - 16;
            (level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        232.. => {
            let gray = 8 + 10 * u16::from(index - 232);
            (gray, gray, gray)
        }
    };
    // Rec. 601 luma.
    299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b) < 128_000
}

fn draw_confirm(frame: &mut Frame, app: &App, area: Rect, confirm: &Confirm) {
    let path = tilde(&app.path);
    let (title, lines, keys) = match confirm {
        Confirm::Quit => (
            " Unsaved changes ",
            vec![format!("Save your changes to {path}?")],
            "y save and quit   n quit without saving   esc keep editing",
        ),
        Confirm::Overwrite => (
            " Changed on disk ",
            vec![format!(
                "{path} changed on disk since the editor read it. Saving now replaces those \
                 changes."
            )],
            "o overwrite it   r reload it, dropping your changes   esc cancel",
        ),
        Confirm::ReadOnly(error) => (
            " Can't save ",
            vec![
                format!("Can't write {path}: {error}."),
                "If a tool such as Nix home-manager makes this file, put the config there \
                 instead."
                    .to_string(),
            ],
            "p print the TOML when you quit   esc back",
        ),
        Confirm::Claude(change) => {
            let mut lines = vec![format!("Change {}:", tilde(&change.path)), String::new()];
            lines.extend(change.summary.iter().map(|line| format!("  {line}")));
            lines.push(String::new());
            lines.push("Every other setting stays as it is.".to_string());
            if change.path.exists() {
                lines.push("The previous file is kept as settings.json.bak.".to_string());
            }
            (" Set up Claude Code ", lines, "y change it   n leave it")
        }
    };
    let width = 76;
    // The keys sit on the border, where text too long for the screen can't push them out of view.
    let wrapped: usize = lines
        .iter()
        .map(|line| wrapped_lines(line, usize::from(width) - 2))
        .sum();
    let popup = centered(area, width, wrapped as u16 + 2);
    let text: Vec<Line> = lines.into_iter().map(Line::raw).collect();
    let block = Block::bordered()
        .title(title)
        .title_bottom(Line::styled(format!(" {keys} "), bold()));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).block(block),
        popup,
    );
}

/// How many lines `text` takes when wrapped at spaces to `width` columns, with a word too long
/// for a line split across lines, as a wrapping `Paragraph` draws it.
pub(super) fn wrapped_lines(text: &str, width: usize) -> usize {
    let width = width.max(1);
    let words = text.trim_start_matches(' ');
    // An indent stays on the first line.
    let (mut lines, mut used) = (1, text.len() - words.len());
    for (i, word) in words.split(' ').enumerate() {
        let mut word_width = word.width();
        let gap = usize::from(i > 0);
        if used + gap + word_width <= width {
            used += gap + word_width;
            continue;
        }
        if used > 0 {
            lines += 1;
        }
        while word_width > width {
            lines += 1;
            word_width -= width;
        }
        used = word_width;
    }
    lines
}

/// `path` with the home directory written as `~`.
fn tilde(path: &Path) -> String {
    match env::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

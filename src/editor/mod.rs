//! `astro-statusline config`: a full-screen editor for the config file, with a live preview drawn
//! by the same code as the status line.

mod claude;
mod doc;
mod file;
mod preview;
mod ui;

use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::config::{self, Config, Thresholds};
use crate::segments::Kind;
use crate::style::COLORS;
use doc::{Doc, Field};
use preview::SCENARIOS;

/// Put at the top of a config file the editor makes.
const HEADER: &str = "# astro-statusline config, made with `astro-statusline config`. Every key is optional;\n# see the README for all of them.\n\n";

/// The narrowest terminal the preview can stand in for.
const MIN_COLUMNS: usize = 10;

/// Cells in each row of the style picker's 256-color grid.
const GRID_COLUMNS: u8 = 32;

pub fn main() -> ExitCode {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!("astro-statusline config: needs a terminal to run in");
        return ExitCode::FAILURE;
    }
    let mut app = match App::open() {
        Ok(app) => app,
        Err(error) => {
            eprintln!("astro-statusline config: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = run(&mut app) {
        eprintln!("astro-statusline config: {error}");
        return ExitCode::FAILURE;
    }
    if app.print_on_exit {
        print!("{}", app.file_text());
    }
    ExitCode::SUCCESS
}

fn run(app: &mut App) -> io::Result<()> {
    // `try_init` restores the terminal on a panic, but not when it fails partway itself.
    let mut terminal = ratatui::try_init().inspect_err(|_| ratatui::restore())?;
    let result = (|| {
        while !app.quit {
            app.screen_width = terminal.size()?.width.into();
            terminal.draw(|frame| ui::draw(frame, app))?;
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                app.handle(key);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

/// A group of settings in the editor's left-hand list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Layout,
    DropOrder,
    Segment(Kind),
    Separator,
    UsageColors,
    Bar,
    Graph,
    ClaudeCode,
}

const SECTIONS: [Section; 14] = [
    Section::Layout,
    Section::DropOrder,
    Section::Segment(Kind::Model),
    Section::Segment(Kind::Directory),
    Section::Segment(Kind::Branch),
    Section::Segment(Kind::Context),
    Section::Segment(Kind::FiveHour),
    Section::Segment(Kind::SevenDay),
    Section::Segment(Kind::Cost),
    Section::Separator,
    Section::UsageColors,
    Section::Bar,
    Section::Graph,
    Section::ClaudeCode,
];

impl Section {
    /// The line of the right-hand pane the section's first field is on. Claude Code's first line
    /// sets up the status line.
    fn first_field(self) -> usize {
        usize::from(self == Section::ClaudeCode)
    }

    /// The fields the section edits, in a config with `rows` rows.
    fn fields(self, rows: usize) -> Vec<Field> {
        match self {
            Section::Segment(kind) => {
                let mut fields = vec![Field::Template(kind)];
                fields.extend(own_style(kind).map(Field::Style));
                if Thresholds::default().get(kind).is_some() {
                    fields.push(Field::Thresholds(kind));
                }
                fields
            }
            Section::Separator => {
                let mut fields: Vec<Field> = (0..rows.max(1)).map(Field::Separator).collect();
                fields.push(Field::Style("separator"));
                fields
            }
            Section::ClaudeCode => vec![Field::Padding],
            Section::UsageColors => vec![
                Field::Style("usage_low"),
                Field::Style("usage_medium"),
                Field::Style("usage_high"),
            ],
            Section::Bar => vec![
                Field::BarWidth,
                Field::BarFull,
                Field::BarEmpty,
                Field::BarMarker,
            ],
            Section::Graph => vec![Field::GraphWidth, Field::GraphChars],
            Section::Layout | Section::DropOrder => Vec::new(),
        }
    }
}

/// The `[colors]` key of a segment drawn in a style of its own. The others take the usage colors.
fn own_style(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Model | Kind::Directory | Kind::Branch | Kind::Cost => Some(kind.name()),
        Kind::Context | Kind::FiveHour | Kind::SevenDay => None,
    }
}

/// One line of the layout list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutItem {
    Row(usize),
    Segment {
        row: usize,
        index: usize,
        kind: Kind,
    },
    HiddenHeader,
    Hidden(Kind),
}

fn layout_items(rows: &[Vec<Kind>]) -> Vec<LayoutItem> {
    let mut items = Vec::new();
    for (row, kinds) in rows.iter().enumerate() {
        items.push(LayoutItem::Row(row));
        items.extend(
            kinds
                .iter()
                .enumerate()
                .map(|(index, &kind)| LayoutItem::Segment { row, index, kind }),
        );
    }
    items.push(LayoutItem::HiddenHeader);
    let shown = |kind: &Kind| rows.iter().flatten().any(|shown| shown == kind);
    items.extend(
        Kind::ALL
            .into_iter()
            .filter(|kind| !shown(kind))
            .map(LayoutItem::Hidden),
    );
    items
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Sections,
    Detail,
}

enum Mode {
    Normal,
    Edit(Edit),
    Style(Picker),
    Confirm(Confirm),
}

/// A field being typed into.
struct Edit {
    field: Field,
    input: Input,
    /// The highlighted entry of the open placeholder list, for a template.
    placeholders: Option<usize>,
}

/// One line of text and a cursor, counted in characters.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Input {
    chars: Vec<char>,
    cursor: usize,
}

impl Input {
    fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        Input {
            cursor: chars.len(),
            chars,
        }
    }

    fn text(&self) -> String {
        self.chars.iter().collect()
    }

    fn insert(&mut self, text: &str) {
        for c in text.chars() {
            self.chars.insert(self.cursor, c);
            self.cursor += 1;
        }
    }

    /// Handles an editing key, returning whether it was one.
    fn key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char(c) => self.insert(c.encode_utf8(&mut [0; 4])),
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.chars.len(),
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => return false,
        }
        true
    }
}

/// A style being picked: its attributes and at most one color.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Picker {
    field: Field,
    /// Bold, dim, italic and underline, in that order.
    attributes: [bool; 4],
    color: Option<PickedColor>,
    /// The highlighted cell of the 256-color grid.
    cursor: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PickedColor {
    Index(u8),
    /// A color the grid doesn't hold, such as `#ff8700`, as written.
    Other(String),
}

impl Picker {
    fn new(field: Field, text: &str) -> Self {
        let mut picker = Picker {
            field,
            attributes: [false; 4],
            color: None,
            cursor: 0,
        };
        for word in text.split_whitespace() {
            let lower = word.to_ascii_lowercase();
            if let Some(i) = crate::style::ATTRIBUTES
                .iter()
                .position(|(name, _)| *name == lower)
            {
                picker.attributes[i] = true;
                continue;
            }
            let (offset, name) = match lower.strip_prefix("bright_") {
                Some(name) => (8, name),
                None => (0, lower.as_str()),
            };
            let index = COLORS
                .iter()
                .position(|color| *color == name)
                .map(|i| i as u8 + offset)
                .or_else(|| {
                    lower
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                        .then(|| lower.parse().ok())
                        .flatten()
                });
            picker.color = Some(match index {
                Some(index) => {
                    picker.cursor = index;
                    PickedColor::Index(index)
                }
                None => PickedColor::Other(word.to_string()),
            });
        }
        picker
    }

    /// The style as the config file spells it.
    fn text(&self) -> String {
        let mut words: Vec<String> = crate::style::ATTRIBUTES
            .iter()
            .zip(self.attributes)
            .filter(|(_, on)| *on)
            .map(|((name, _), _)| name.to_string())
            .collect();
        words.extend(self.color.as_ref().map(|color| match color {
            PickedColor::Index(index @ 0..8) => COLORS[*index as usize].to_string(),
            PickedColor::Index(index @ 8..16) => format!("bright_{}", COLORS[*index as usize - 8]),
            PickedColor::Index(index) => index.to_string(),
            PickedColor::Other(text) => text.clone(),
        }));
        words.join(" ")
    }

    fn move_cursor(&mut self, step: i16) {
        self.cursor = (i16::from(self.cursor) + step).clamp(0, 255) as u8;
        self.color = Some(PickedColor::Index(self.cursor));
    }
}

enum Confirm {
    /// Quitting with unsaved changes.
    Quit,
    /// Saving over a file that changed on disk since it was read.
    Overwrite,
    /// The config file can't be written, for this reason.
    ReadOnly(String),
    /// Setting Claude Code's status line.
    Claude(claude::Change),
}

struct App {
    path: PathBuf,
    /// The file's text as last read or written, or empty when there is no file.
    loaded: String,
    doc: Doc,
    focus: Focus,
    section: usize,
    /// The highlighted line of the right-hand pane.
    cursor: usize,
    mode: Mode,
    scenario: usize,
    /// The terminal width the preview stands in for, or `None` for as wide as it can.
    columns: Option<usize>,
    screen_width: usize,
    /// The latest message for the footer, and whether it's an error.
    message: Option<(String, bool)>,
    claude_path: Option<PathBuf>,
    claude_status: claude::Status,
    /// The `padding` of Claude Code's status line, when its settings set one.
    claude_padding: Option<u64>,
    exe: PathBuf,
    print_on_exit: bool,
    quit_after_save: bool,
    quit: bool,
}

impl App {
    /// Opens the config file. A file that isn't TOML at all can't be edited key by key, so it's
    /// an error here, and the file is left as it is.
    fn open() -> Result<App, String> {
        let path =
            config::path().ok_or("can't find the config directory: set HOME or XDG_CONFIG_HOME")?;
        let loaded =
            read_or_empty(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let doc = Doc::parse(&loaded).map_err(|error| {
            format!(
                "{}: {error}\nFix it in a text editor, then run astro-statusline config again.",
                path.display()
            )
        })?;
        let exe =
            env::current_exe().map_err(|error| format!("can't find this program: {error}"))?;
        Ok(App::new(path, loaded, doc, claude::settings_path(), exe))
    }

    fn new(
        path: PathBuf,
        loaded: String,
        doc: Doc,
        claude_path: Option<PathBuf>,
        exe: PathBuf,
    ) -> App {
        let mut app = App {
            path,
            loaded,
            doc,
            focus: Focus::Sections,
            section: 0,
            cursor: 0,
            mode: Mode::Normal,
            scenario: 0,
            columns: None,
            screen_width: 80,
            message: None,
            claude_path,
            claude_status: claude::Status::NotSet,
            claude_padding: None,
            exe,
            print_on_exit: false,
            quit_after_save: false,
            quit: false,
        };
        app.refresh_claude();
        if let Err(error) = config::parse(&app.loaded) {
            app.error(format!(
                "The config has a mistake, so Claude Code shows the defaults: {}",
                short_error(&error)
            ));
        } else if !matches!(app.claude_status, claude::Status::Runs(_)) {
            app.info("Claude Code doesn't run astro-statusline yet: see Claude Code at the bottom of the list");
        }
        app
    }

    fn section(&self) -> Section {
        SECTIONS[self.section]
    }

    fn dirty(&self) -> bool {
        self.doc.text() != self.loaded
    }

    /// The text saving would write.
    fn file_text(&self) -> String {
        let text = self.doc.text();
        if self.loaded.trim().is_empty() && !text.trim().is_empty() {
            format!("{HEADER}{text}")
        } else {
            text
        }
    }

    /// The config the preview draws: the file with the edit under way. An error is why Claude
    /// Code would fall back to the defaults.
    fn preview_config(&self) -> Result<Config, String> {
        let mut doc = self.doc.clone();
        match &self.mode {
            Mode::Edit(edit) => {
                if let Ok(value) = edit.field.value(&edit.input.text()) {
                    doc.set(edit.field, value);
                }
            }
            Mode::Style(picker) => {
                if let Ok(value) = picker.field.value(&picker.text()) {
                    doc.set(picker.field, value);
                }
            }
            Mode::Normal | Mode::Confirm(_) => {}
        }
        config::parse(&doc.text()).map_err(|error| short_error(&error))
    }

    /// The terminal width the preview stands in for.
    fn preview_columns(&self) -> usize {
        let widest = self.screen_width.saturating_sub(2).max(MIN_COLUMNS);
        self.columns.unwrap_or(widest).clamp(MIN_COLUMNS, widest)
    }

    fn info(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), false));
    }

    fn error(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), true));
    }

    fn refresh_claude(&mut self) {
        (self.claude_status, self.claude_padding) = match &self.claude_path {
            None => (
                claude::Status::Unreadable("can't find the home directory".to_string()),
                None,
            ),
            Some(path) => match claude::Settings::read(path) {
                Ok(settings) => (settings.status(), settings.padding()),
                Err(error) => (claude::Status::Unreadable(error), None),
            },
        };
    }

    fn handle(&mut self, key: KeyEvent) {
        // Ctrl+C always leaves, asking first only about unsaved changes.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if matches!(self.mode, Mode::Confirm(Confirm::Quit)) {
                self.quit = true;
            } else {
                self.mode = Mode::Normal;
                self.request_quit();
            }
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.normal_key(key),
            Mode::Edit(edit) => self.edit_key(edit, key),
            Mode::Style(picker) => self.picker_key(picker, key),
            Mode::Confirm(confirm) => self.confirm_key(confirm, key),
        }
    }

    fn normal_key(&mut self, key: KeyEvent) {
        self.message = None;
        let count = SCENARIOS.len();
        match key.code {
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('s') => self.save(false),
            KeyCode::Char(']') => self.scenario = (self.scenario + 1) % count,
            KeyCode::Char('[') => self.scenario = (self.scenario + count - 1) % count,
            KeyCode::Char('>' | '.') => self.widen(1),
            KeyCode::Char('<' | ',') => self.widen(-1),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Sections => Focus::Detail,
                    Focus::Detail => Focus::Sections,
                }
            }
            _ => match self.focus {
                Focus::Sections => self.sections_key(key),
                Focus::Detail => self.detail_key(key),
            },
        }
    }

    fn widen(&mut self, step: isize) {
        let widest = self.screen_width.saturating_sub(2).max(MIN_COLUMNS);
        let columns = self.preview_columns().saturating_add_signed(step);
        self.columns = (columns < widest).then_some(columns.max(MIN_COLUMNS));
    }

    fn sections_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.section = self.section.saturating_sub(1);
                self.cursor = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.section = (self.section + 1).min(SECTIONS.len() - 1);
                self.cursor = 0;
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => self.focus = Focus::Detail,
            _ => {}
        }
    }

    fn detail_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Left | KeyCode::Char('h') | KeyCode::Esc) {
            self.focus = Focus::Sections;
            return;
        }
        match self.section() {
            Section::Layout => self.layout_key(key),
            Section::DropOrder => self.drop_order_key(key),
            section => {
                let fields = section.fields(self.doc.rows().len());
                let first = section.first_field();
                let last = first + fields.len() - 1;
                // Rows taken away can leave the cursor past the last field.
                self.cursor = self.cursor.min(last);
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(last),
                    KeyCode::Enter if self.cursor < first => self.offer_claude_setup(),
                    KeyCode::Enter => self.open_field(fields[self.cursor - first]),
                    KeyCode::Char('r') if self.cursor >= first => {
                        self.doc.reset(fields[self.cursor - first]);
                    }
                    _ => {}
                }
            }
        }
    }

    fn open_field(&mut self, field: Field) {
        let text = self.doc.get(field).text;
        self.mode = match field {
            Field::Style(_) => Mode::Style(Picker::new(field, &text)),
            _ => Mode::Edit(Edit {
                field,
                input: Input::new(&text),
                placeholders: None,
            }),
        };
    }

    fn edit_key(&mut self, mut edit: Edit, key: KeyEvent) {
        let placeholders = match edit.field {
            Field::Template(kind) => kind.placeholders(),
            _ => &[],
        };
        match (edit.placeholders, key.code) {
            (Some(i), KeyCode::Up) => edit.placeholders = Some(i.saturating_sub(1)),
            (Some(i), KeyCode::Down) => {
                edit.placeholders = Some((i + 1).min(placeholders.len() - 1));
            }
            (Some(i), KeyCode::Enter) => {
                edit.input.insert(&format!("{{{}}}", placeholders[i]));
                edit.placeholders = None;
            }
            (Some(_), KeyCode::Esc | KeyCode::Tab) => edit.placeholders = None,
            (Some(_), _) => {}
            (None, KeyCode::Tab) if !placeholders.is_empty() => edit.placeholders = Some(0),
            (None, KeyCode::Enter) => {
                // A value with a mistake stays open, with the mistake shown under it.
                if let Ok(value) = edit.field.value(&edit.input.text()) {
                    self.doc.set(edit.field, value);
                    return;
                }
            }
            (None, KeyCode::Esc) => return,
            // Ctrl with a letter is a shortcut elsewhere, not the letter.
            (None, _) if key.modifiers.contains(KeyModifiers::CONTROL) => {}
            (None, code) => {
                edit.input.key(code);
            }
        }
        self.mode = Mode::Edit(edit);
    }

    fn picker_key(&mut self, mut picker: Picker, key: KeyEvent) {
        let row = i16::from(GRID_COLUMNS);
        match key.code {
            KeyCode::Left => picker.move_cursor(-1),
            KeyCode::Right => picker.move_cursor(1),
            KeyCode::Up => picker.move_cursor(-row),
            KeyCode::Down => picker.move_cursor(row),
            KeyCode::Char(c @ ('b' | 'd' | 'i' | 'u')) => {
                let i = "bdiu".find(c).expect("one of the four");
                picker.attributes[i] = !picker.attributes[i];
            }
            KeyCode::Char('x') => picker.color = None,
            KeyCode::Char('t') => {
                self.mode = Mode::Edit(Edit {
                    field: picker.field,
                    input: Input::new(&picker.text()),
                    placeholders: None,
                });
                return;
            }
            KeyCode::Enter => match picker.field.value(&picker.text()) {
                Ok(value) => {
                    self.doc.set(picker.field, value);
                    return;
                }
                Err(error) => self.error(error),
            },
            KeyCode::Esc => return,
            _ => {}
        }
        self.mode = Mode::Style(picker);
    }

    fn drop_order_key(&mut self, key: KeyEvent) {
        let mut sequence = self.doc.drop_sequence();
        let last = sequence.len() - 1;
        self.cursor = self.cursor.min(last);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let target = match key.code {
            KeyCode::Up if shift => self.cursor.checked_sub(1),
            KeyCode::Down if shift => (self.cursor < last).then_some(self.cursor + 1),
            KeyCode::Char('K') => self.cursor.checked_sub(1),
            KeyCode::Char('J') => (self.cursor < last).then_some(self.cursor + 1),
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1).min(last);
                return;
            }
            KeyCode::Char('r') => {
                self.doc
                    .set_drop_sequence(&Config::default().drop_sequence());
                return;
            }
            _ => return,
        };
        if let Some(target) = target {
            sequence.swap(self.cursor, target);
            self.doc.set_drop_sequence(&sequence);
            self.cursor = target;
        }
    }

    fn layout_key(&mut self, key: KeyEvent) {
        let mut rows = self.doc.rows();
        let items = layout_items(&rows);
        self.cursor = self.cursor.min(items.len() - 1);
        let item = items[self.cursor];
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let moved = match key.code {
            KeyCode::Up if shift => move_up(&mut rows, item),
            KeyCode::Down if shift => move_down(&mut rows, item),
            KeyCode::Char('K') => move_up(&mut rows, item),
            KeyCode::Char('J') => move_down(&mut rows, item),
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1).min(items.len() - 1);
                return;
            }
            KeyCode::Char('n') => {
                let row = match item {
                    LayoutItem::Row(row) | LayoutItem::Segment { row, .. } => row + 1,
                    LayoutItem::HiddenHeader | LayoutItem::Hidden(_) => rows.len(),
                };
                rows.insert(row, Vec::new());
                Some(LayoutItem::Row(row))
            }
            KeyCode::Char('d') | KeyCode::Delete => match item {
                LayoutItem::Row(row) => {
                    rows.remove(row);
                    None
                }
                LayoutItem::Segment { row, index, .. } => {
                    rows[row].remove(index);
                    None
                }
                LayoutItem::HiddenHeader | LayoutItem::Hidden(_) => return,
            },
            KeyCode::Char('r') => {
                self.doc.reset_rows();
                self.cursor = 0;
                return;
            }
            KeyCode::Enter => {
                if let LayoutItem::Segment { kind, .. } | LayoutItem::Hidden(kind) = item {
                    self.section = SECTIONS
                        .iter()
                        .position(|section| *section == Section::Segment(kind))
                        .expect("every segment has a section");
                    self.cursor = 0;
                }
                return;
            }
            _ => return,
        };
        self.doc.set_rows(&rows);
        let items = layout_items(&rows);
        // The cursor follows what moved, and otherwise stays on its line.
        self.cursor = moved
            .and_then(|moved| items.iter().position(|item| same_place(*item, moved)))
            .unwrap_or(self.cursor)
            .min(items.len() - 1);
    }

    fn request_quit(&mut self) {
        // Unsaved changes are kept when the file is printed on the way out.
        if self.dirty() && !self.print_on_exit {
            self.mode = Mode::Confirm(Confirm::Quit);
        } else {
            self.quit = true;
        }
    }

    /// Saves the file, unless it changed on disk since it was read and `force` isn't set.
    fn save(&mut self, force: bool) {
        if !self.dirty() {
            self.info("Nothing to save");
            self.quit = self.quit_after_save;
            return;
        }
        let text = self.file_text();
        if let Err(error) = config::parse(&text) {
            self.error(format!("Not saved: {}", short_error(&error)));
            self.quit_after_save = false;
            return;
        }
        let on_disk = match read_or_empty(&self.path) {
            Ok(on_disk) => on_disk,
            Err(error) => {
                self.error(format!("Not saved: {}: {error}", self.path.display()));
                self.quit_after_save = false;
                return;
            }
        };
        if on_disk != self.loaded && !force {
            self.mode = Mode::Confirm(Confirm::Overwrite);
            return;
        }
        let existed = self.path.exists();
        let backup = self.path.with_file_name("config.toml.bak");
        match file::write(&self.path, &text, &backup) {
            Ok(()) => {
                self.doc = Doc::parse(&text).expect("the editor writes TOML");
                self.loaded = text;
                self.info(if existed {
                    format!(
                        "Saved {}; the previous file is config.toml.bak",
                        self.path.display()
                    )
                } else {
                    format!("Saved {}", self.path.display())
                });
                self.quit = self.quit_after_save;
            }
            Err(error) if file::is_read_only(&error) => {
                self.mode = Mode::Confirm(Confirm::ReadOnly(error.to_string()));
            }
            Err(error) => {
                self.error(format!("Not saved: {}: {error}", self.path.display()));
                self.quit_after_save = false;
            }
        }
    }

    fn reload(&mut self) {
        let reloaded = read_or_empty(&self.path)
            .map_err(|error| error.to_string())
            .and_then(|text| Ok((Doc::parse(&text)?, text)));
        match reloaded {
            Ok((doc, text)) => {
                self.doc = doc;
                self.loaded = text;
                self.info("Reloaded the file from disk");
            }
            Err(error) => self.error(format!("Can't reload {}: {error}", self.path.display())),
        }
    }

    fn confirm_key(&mut self, confirm: Confirm, key: KeyEvent) {
        match (confirm, key.code) {
            (Confirm::Quit, KeyCode::Char('y')) => {
                self.quit_after_save = true;
                self.save(false);
            }
            (Confirm::Quit, KeyCode::Char('n')) => self.quit = true,
            (Confirm::Overwrite, KeyCode::Char('o')) => self.save(true),
            (Confirm::Overwrite, KeyCode::Char('r')) => {
                self.quit_after_save = false;
                self.reload();
            }
            (Confirm::ReadOnly(_), KeyCode::Char('p')) => {
                self.print_on_exit = true;
                self.info("The config will be printed when you quit");
                self.quit = self.quit_after_save;
            }
            (Confirm::Claude(change), KeyCode::Char('y')) => self.set_up_claude(change),
            (_, KeyCode::Esc | KeyCode::Char('n')) => self.quit_after_save = false,
            (confirm, _) => self.mode = Mode::Confirm(confirm),
        }
    }

    fn offer_claude_setup(&mut self) {
        let Some(path) = &self.claude_path else {
            self.error("Can't find Claude Code's settings: set HOME or CLAUDE_CONFIG_DIR");
            return;
        };
        match claude::Settings::read(path) {
            Ok(settings) => self.mode = Mode::Confirm(Confirm::Claude(settings.run(&self.exe))),
            Err(error) => self.error(error),
        }
    }

    fn set_up_claude(&mut self, change: claude::Change) {
        let backup = change.path.with_file_name("settings.json.bak");
        let existed = change.path.exists();
        match file::write(&change.path, &change.text, &backup) {
            Ok(()) => {
                self.refresh_claude();
                self.info(if existed {
                    format!(
                        "Claude Code now runs astro-statusline; the previous settings are {}",
                        backup.display()
                    )
                } else {
                    "Claude Code now runs astro-statusline".to_string()
                });
            }
            Err(error) => self.error(format!("Not set: {}: {error}", change.path.display())),
        }
    }
}

/// Moves a segment one place up: within its row, to the end of the row above, or from the hidden
/// list to the end of the last row.
fn move_up(rows: &mut Vec<Vec<Kind>>, item: LayoutItem) -> Option<LayoutItem> {
    match item {
        LayoutItem::Segment { row, index, kind } if index > 0 => {
            rows[row].swap(index, index - 1);
            Some(LayoutItem::Segment {
                row,
                index: index - 1,
                kind,
            })
        }
        LayoutItem::Segment { row, index, kind } if row > 0 => {
            rows[row].remove(index);
            rows[row - 1].push(kind);
            let index = rows[row - 1].len() - 1;
            Some(LayoutItem::Segment {
                row: row - 1,
                index,
                kind,
            })
        }
        LayoutItem::Hidden(kind) => {
            if rows.is_empty() {
                rows.push(Vec::new());
            }
            let row = rows.len() - 1;
            rows[row].push(kind);
            let index = rows[row].len() - 1;
            Some(LayoutItem::Segment { row, index, kind })
        }
        _ => None,
    }
}

/// Moves a segment one place down: within its row, to the start of the row below, or off the
/// last row into the hidden list.
fn move_down(rows: &mut [Vec<Kind>], item: LayoutItem) -> Option<LayoutItem> {
    let LayoutItem::Segment { row, index, kind } = item else {
        return None;
    };
    if index + 1 < rows[row].len() {
        rows[row].swap(index, index + 1);
        return Some(LayoutItem::Segment {
            row,
            index: index + 1,
            kind,
        });
    }
    rows[row].remove(index);
    if row + 1 < rows.len() {
        rows[row + 1].insert(0, kind);
        Some(LayoutItem::Segment {
            row: row + 1,
            index: 0,
            kind,
        })
    } else {
        Some(LayoutItem::Hidden(kind))
    }
}

/// Whether two layout items are the same line, comparing where a segment is rather than which.
fn same_place(a: LayoutItem, b: LayoutItem) -> bool {
    match (a, b) {
        (
            LayoutItem::Segment { row, index, .. },
            LayoutItem::Segment {
                row: other_row,
                index: other_index,
                ..
            },
        ) => (row, index) == (other_row, other_index),
        (a, b) => a == b,
    }
}

fn read_or_empty(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        result => result,
    }
}

/// A config error on one line, such as `line 3, column 9: unknown style "purple"`. TOML errors
/// put the position first and the message last, with the line quoted between.
fn short_error(error: &str) -> String {
    let first = error.lines().next().unwrap_or_default();
    let message = error
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    match first.strip_prefix("TOML parse error at ") {
        Some(position) if first != message => format!("{position}: {message}"),
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests;

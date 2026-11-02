//! The editor driven by key presses, as a user would, against files in a temporary directory.

use std::os::unix::fs::PermissionsExt;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tempfile::TempDir;

use super::*;

const EXE: &str = "/usr/bin/astro-statusline";

struct Fixture {
    dir: TempDir,
    app: App,
}

impl Fixture {
    /// The editor opened on `config`, or on no file at all.
    fn new(config: Option<&str>) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("astro-statusline/config.toml");
        if let Some(config) = config {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, config).unwrap();
        }
        let loaded = read_or_empty(&path).unwrap();
        let doc = Doc::parse(&loaded).unwrap();
        let settings = dir.path().join("claude/settings.json");
        let app = App::new(path, loaded, doc, Some(settings), PathBuf::from(EXE));
        Fixture { dir, app }
    }

    fn press(&mut self, codes: &[KeyCode]) -> &mut Self {
        for &code in codes {
            self.app.handle(KeyEvent::new(code, KeyModifiers::NONE));
        }
        self
    }

    fn typing(&mut self, text: &str) -> &mut Self {
        let codes: Vec<KeyCode> = text.chars().map(KeyCode::Char).collect();
        self.press(&codes)
    }

    /// Opens the section titled `title` in the left-hand list.
    fn open(&mut self, section: Section) -> &mut Self {
        self.app.focus = Focus::Sections;
        self.app.section = SECTIONS.iter().position(|s| *s == section).unwrap();
        self.app.cursor = 0;
        self.press(&[KeyCode::Enter])
    }

    fn file(&self) -> String {
        fs::read_to_string(&self.app.path).unwrap()
    }

    fn message(&self) -> String {
        self.app
            .message
            .clone()
            .map(|(text, _)| text)
            .unwrap_or_default()
    }

    /// The screen as text, one line per row.
    fn screen(&mut self, width: u16, height: u16) -> String {
        self.app.screen_width = width.into();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &self.app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn edits_a_template_and_saves_a_new_file() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Model))
        .press(&[KeyCode::Enter])
        .typing(" ⚡")
        .press(&[KeyCode::Enter]);
    assert!(f.app.dirty());
    f.typing("s");
    assert_eq!(
        f.file(),
        format!("{HEADER}[format]\nmodel = \"{{name}} ⚡\"\n")
    );
    assert!(f.message().starts_with("Saved "), "{}", f.message());
    assert!(!f.app.dirty());
    assert!(config::parse(&f.file()).is_ok());
}

#[test]
fn keeps_a_template_with_a_mistake_open_until_fixed() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Model))
        .press(&[KeyCode::Enter, KeyCode::Backspace])
        .typing("x}")
        .press(&[KeyCode::Enter]);
    assert!(matches!(f.app.mode, Mode::Edit(_)));
    assert!(f.screen(100, 30).contains("unknown placeholder {namex}"));
    // The preview keeps the last good value while the edit has a mistake.
    assert!(f.screen(100, 30).contains("Opus 5.5"));
    f.press(&[KeyCode::Esc]);
    assert!(matches!(f.app.mode, Mode::Normal));
    assert!(!f.app.dirty());
}

#[test]
fn ignores_ctrl_shortcuts_while_typing() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Model))
        .press(&[KeyCode::Enter]);
    f.app
        .handle(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    f.typing("!").press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.get(Field::Template(Kind::Model)).text, "{name}!");
}

#[test]
fn inserts_a_placeholder_from_the_list() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Context)).press(&[
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::Down,
        KeyCode::Enter,
        KeyCode::Enter,
    ]);
    assert_eq!(
        f.app.doc.get(Field::Template(Kind::Context)).text,
        "ctx {percent}{bar}"
    );
}

#[test]
fn previews_an_edit_as_it_is_typed() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Context))
        .press(&[KeyCode::Enter])
        .typing(" {bar}");
    assert!(
        f.screen(110, 30).contains("ctx 42% ████░░░░░░"),
        "{}",
        f.screen(110, 30)
    );
}

#[test]
fn picks_a_style_from_the_grid() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Model))
        .press(&[KeyCode::Down, KeyCode::Enter]);
    let Mode::Style(picker) = &f.app.mode else {
        panic!("the style picker is open");
    };
    // The default, cyan, is color 6.
    assert_eq!(picker.cursor, 6);
    f.typing("b").press(&[KeyCode::Right, KeyCode::Enter]);
    assert_eq!(f.app.doc.get(Field::Style("model")).text, "bold white");

    f.press(&[KeyCode::Enter, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    assert_eq!(f.app.doc.get(Field::Style("model")).text, "bold 71");
    f.press(&[KeyCode::Enter])
        .typing("bx")
        .press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.get(Field::Style("model")).text, "");
}

#[test]
fn keeps_the_preview_in_view_while_picking_a_style() {
    let mut f = Fixture::new(Some("segments = [\"model\"]\n"));
    f.open(Section::Segment(Kind::Model))
        .press(&[KeyCode::Down, KeyCode::Enter]);
    // The smallest terminal the editor draws in.
    let screen = f.screen(72, 20);
    assert!(screen.contains("Opus 5.5"), "{screen}");
    assert!(screen.contains(" colors.model "), "{screen}");
    assert!(screen.contains("[]"), "{screen}");
    assert!(screen.contains("t types any style"), "{screen}");
}

#[test]
fn types_a_hex_style() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Cost))
        .press(&[KeyCode::Down, KeyCode::Enter])
        .typing("t");
    assert!(matches!(f.app.mode, Mode::Edit(_)));
    for _ in 0.."dim".len() {
        f.press(&[KeyCode::Backspace]);
    }
    f.typing("italic #ff8700").press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.get(Field::Style("cost")).text, "italic #ff8700");
}

#[test]
fn reads_styles_into_the_picker_and_back() {
    for (text, expected) in [
        ("bold #ff8700", "bold #ff8700"),
        ("Bright_Red underline", "underline bright_red"),
        ("208 dim", "dim 208"),
        ("bold usage", "bold usage"),
        ("", ""),
    ] {
        assert_eq!(Picker::new(Field::Style("context"), text).text(), expected);
    }
    assert_eq!(Picker::new(Field::Style("model"), "bright_red").cursor, 9);
}

#[test]
fn picks_the_usage_color_only_where_there_is_one() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Context))
        .press(&[KeyCode::Down]);
    let screen = f.screen(110, 30);
    assert!(screen.contains("Style         usage"), "{screen}");
    f.press(&[KeyCode::Enter, KeyCode::Right]);
    assert!(f.screen(110, 30).contains("U usage"));
    f.typing("bU").press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.text(), "[colors]\ncontext = \"bold usage\"\n");

    // The model has no percentage, so no usage color.
    f.open(Section::Segment(Kind::Model))
        .press(&[KeyCode::Down, KeyCode::Enter]);
    assert!(!f.screen(110, 30).contains("U usage"));
    f.typing("U").press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.get(Field::Style("model")).text, "cyan");
    assert!(Field::Style("model").value("usage").is_err());
    assert!(Field::Template(Kind::Model).value("{name:usage}").is_err());
    assert!(
        Field::Template(Kind::Context)
            .value("{percent:usage}")
            .is_ok()
    );
}

#[test]
fn arranges_segments_into_rows() {
    let mut f = Fixture::new(Some(
        "# Mine.\nsegments = [\"model\", \"directory\", \"cost\"]\n",
    ));
    f.open(Section::Layout);
    // Row 1, then model.
    f.press(&[KeyCode::Down]).typing("J");
    assert_eq!(
        f.app.doc.rows(),
        [[Kind::Directory, Kind::Model, Kind::Cost]]
    );
    f.typing("n");
    assert_eq!(f.app.doc.rows().len(), 2);
    assert!(f.app.doc.text().starts_with("# Mine.\nlines = [\n"));
    // Cost moves down into the new row, then off it into the hidden list.
    f.press(&[KeyCode::Up]).typing("J");
    assert_eq!(
        f.app.doc.rows(),
        [vec![Kind::Directory, Kind::Model], vec![Kind::Cost]]
    );
    f.typing("J");
    assert_eq!(
        f.app.doc.rows(),
        [vec![Kind::Directory, Kind::Model], vec![]]
    );
    assert!(matches!(
        layout_items(&f.app.doc.rows())[f.app.cursor],
        LayoutItem::Hidden(Kind::Cost)
    ));
    // And back up.
    f.typing("K");
    assert_eq!(
        f.app.doc.rows(),
        [vec![Kind::Directory, Kind::Model], vec![Kind::Cost]]
    );
    // Removing a row hides what was on it.
    f.press(&[KeyCode::Up]).typing("d");
    assert_eq!(f.app.doc.rows(), [vec![Kind::Directory, Kind::Model]]);
    assert!(f.screen(100, 30).contains("Not shown"));
    assert!(config::parse(&f.app.doc.text()).is_ok());
    f.typing("r");
    assert_eq!(f.app.doc.rows(), [Kind::ALL]);
}

#[test]
fn adds_moves_and_removes_a_fill() {
    let mut f = Fixture::new(Some("segments = [\"model\", \"directory\", \"cost\"]\n"));
    f.open(Section::Layout);
    // After the directory.
    f.press(&[KeyCode::Down, KeyCode::Down]).typing("f");
    assert_eq!(
        f.app.doc.text(),
        "segments = [\"model\", \"directory\", \"fill\", \"cost\"]\n"
    );
    assert!(f.screen(100, 30).contains("── fill ──"));
    // The preview stands in for 98 columns, so the line gets 94 after Claude Code's 2 at each
    // edge: after the border and 2 columns, it ends at column 96 with the cost.
    let screen = f.screen(100, 30);
    let line: Vec<char> = screen.lines().nth(1).unwrap().chars().collect();
    let text: String = line[3..97].iter().collect();
    assert!(
        text.starts_with("Opus 5.5 │ my-project ") && text.ends_with(" $1.23"),
        "{text}"
    );
    // Up past the directory, then off the row's end, where it goes away.
    f.typing("K");
    assert_eq!(f.app.doc.rows()[0][1], Item::Fill);
    f.typing("JJJ");
    assert_eq!(
        f.app.doc.rows(),
        [[Kind::Model, Kind::Directory, Kind::Cost]]
    );
    // f on a row with a fill takes it out.
    f.press(&[KeyCode::Up, KeyCode::Up]).typing("ff");
    assert!(!f.app.doc.text().contains("fill"));
}

#[test]
fn opens_a_segment_from_the_layout() {
    let mut f = Fixture::new(None);
    f.open(Section::Layout)
        .press(&[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    assert_eq!(f.app.section(), Section::Segment(Kind::Directory));
}

#[test]
fn scrolls_the_settings_list_to_the_chosen_section() {
    // Two rows of preview leave less room for the list.
    let mut f = Fixture::new(Some("lines = [[\"model\"], [\"cost\"]]\n"));
    f.press(&[KeyCode::Down; SECTIONS.len()]);
    assert_eq!(f.app.section(), Section::ClaudeCode);
    // The smallest terminal the editor draws in.
    let screen = f.screen(72, 20);
    assert!(screen.contains("│ Claude Code"), "{screen}");
}

#[test]
fn caps_a_segment_s_width() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Directory));
    assert!(f.screen(110, 30).contains("Max width     no limit"));
    // Template, style, max width.
    f.press(&[KeyCode::Down, KeyCode::Down, KeyCode::Enter])
        .typing("0");
    assert!(
        f.screen(110, 30)
            .contains("max_width 0 isn't from 1 to 500")
    );
    f.press(&[KeyCode::Backspace])
        .typing("5")
        .press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.text(), "[max_width]\ndirectory = 5\n");
    assert!(f.screen(110, 30).contains("Opus 5.5 │ my-p… │ main"));
}

#[test]
fn shows_a_segment_only_from_a_value() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Cost));
    let screen = f.screen(110, 30);
    assert!(screen.contains("Show from     always"), "{screen}");
    assert!(screen.contains("$1.23"), "{screen}");
    // Template, style, show from.
    f.press(&[KeyCode::Down, KeyCode::Down, KeyCode::Enter])
        .typing("x");
    assert!(f.screen(110, 30).contains("\"x\" isn't a number"));
    f.press(&[KeyCode::Backspace]).typing("1.5");
    // The preview hides the $1.23 cost while the value is typed.
    assert!(!f.screen(110, 30).contains("$1.23"));
    f.press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.text(), "[show_from]\ncost = 1.5\n");
    // Emptied, it's taken out again.
    f.press(&[
        KeyCode::Enter,
        KeyCode::Backspace,
        KeyCode::Backspace,
        KeyCode::Backspace,
        KeyCode::Enter,
    ]);
    assert_eq!(f.app.doc.text(), "[show_from]\n");
    assert!(f.screen(110, 30).contains("$1.23"));
    // Model, directory and branch have no value to compare.
    assert!(
        !Section::Segment(Kind::Model)
            .fields(1)
            .contains(&Field::ShowFrom(Kind::Model))
    );
}

#[test]
fn edits_each_row_s_separator() {
    let mut f = Fixture::new(Some(
        "lines = [[\"model\", \"directory\"], [\"context\", \"cost\"]]\n",
    ));
    f.open(Section::Separator);
    let screen = f.screen(100, 30);
    assert!(
        screen.contains("Row 1         \" │ \"  default"),
        "{screen}"
    );
    assert!(
        screen.contains("Row 2         \" │ \"  default"),
        "{screen}"
    );
    f.press(&[
        KeyCode::Down,
        KeyCode::Enter,
        KeyCode::Left,
        KeyCode::Backspace,
    ])
    .typing("·")
    .press(&[KeyCode::Enter]);
    assert!(
        f.app
            .doc
            .text()
            .ends_with("[format]\nseparator = [\" │ \", \" · \"]\n")
    );
    let screen = f.screen(100, 30);
    assert!(screen.contains("Opus 5.5 │ my-project"), "{screen}");
    assert!(screen.contains("ctx 42% · $1.23"), "{screen}");
    // A single row has a single field.
    let mut f = Fixture::new(None);
    f.open(Section::Separator);
    assert!(f.screen(100, 30).contains("Text          \" │ \"  default"));
}

#[test]
fn sets_the_padding_to_match_claude_code() {
    let mut f = Fixture::new(None);
    let settings = f.dir.path().join("claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        r#"{"statusLine": {"command": "x", "padding": 2}}"#,
    )
    .unwrap();
    f.app.refresh_claude();
    f.open(Section::ClaudeCode);
    assert!(f.screen(100, 30).contains("Claude Code's padding is 2"));
    f.press(&[KeyCode::Down, KeyCode::Enter, KeyCode::Backspace])
        .typing("2")
        .press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.text(), "padding = 2\n");
    assert!(!f.screen(100, 30).contains("Claude Code's padding is"));
    // 2 columns of padding at each end take 4 more from the preview's 40.
    f.app.columns = Some(40);
    assert_eq!(f.app.preview_config().unwrap().padding, 2);
    let screen = f.screen(100, 30);
    assert!(
        screen.contains("Opus 5.5 │ my-project │ ctx 42%"),
        "{screen}"
    );
    assert!(!screen.contains("main"), "{screen}");
}

#[test]
fn reorders_what_drops_first() {
    let mut f = Fixture::new(Some("segments = [\"model\", \"directory\", \"cost\"]\n"));
    f.open(Section::DropOrder);
    let screen = f.screen(100, 30);
    assert!(screen.contains("1. Cost  drops first"), "{screen}");
    assert!(screen.contains("7. Context  drops last"), "{screen}");
    // Directory is fifth; move it to the top.
    f.press(&[KeyCode::Down; 4]).typing("KKKK");
    assert_eq!(f.app.cursor, 0);
    assert_eq!(
        f.app.doc.text(),
        "segments = [\"model\", \"directory\", \"cost\"]\ndrop_order = [\"directory\"]\n"
    );
    // 30 columns leave 26: "Opus 5.5 │ my-project │ $1.23" is 29, so the directory drops.
    f.app.columns = Some(30);
    let screen = f.screen(100, 30);
    assert!(screen.contains("Opus 5.5 │ $1.23"), "{screen}");
    f.typing("r");
    assert_eq!(
        f.app.doc.text(),
        "segments = [\"model\", \"directory\", \"cost\"]\n"
    );
}

#[test]
fn sets_a_segment_s_thresholds() {
    let mut f = Fixture::new(None);
    f.open(Section::Segment(Kind::Context));
    assert!(
        f.screen(110, 30)
            .contains("Medium, high  [50, 80]  default")
    );
    // Template, style, then thresholds.
    f.press(&[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    for _ in 0.."[50, 80]".len() {
        f.press(&[KeyCode::Backspace]);
    }
    f.typing("80, 70").press(&[KeyCode::Enter]);
    assert!(
        f.screen(110, 30)
            .contains("must be from 0 to 100, the first no larger")
    );
    for _ in 0.."80, 70".len() {
        f.press(&[KeyCode::Backspace]);
    }
    f.typing("30, 40").press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.text(), "[thresholds]\ncontext = [30, 40]\n");
    let config = f.app.preview_config().unwrap();
    assert_eq!(config.thresholds.context, [30, 40]);
    // The 5-hour limit has its own, and the cost has none.
    f.open(Section::Segment(Kind::FiveHour));
    assert!(f.screen(110, 30).contains("Medium, high  [50, 80]"));
    assert!(
        !Section::Segment(Kind::Cost)
            .fields(1)
            .contains(&Field::Thresholds(Kind::Cost))
    );
}

#[test]
fn resets_a_field_to_its_default() {
    let mut f = Fixture::new(Some("[bar]\nwidth = 4\nfull = \"#\"\n"));
    f.open(Section::Bar).typing("r");
    assert_eq!(f.app.doc.text(), "[bar]\nfull = \"#\"\n");
    assert!(!f.app.doc.get(Field::BarWidth).set);
}

#[test]
fn checks_numbers_and_cells() {
    let mut f = Fixture::new(None);
    f.open(Section::Bar)
        .press(&[KeyCode::Enter, KeyCode::Backspace, KeyCode::Backspace]);
    f.typing("500").press(&[KeyCode::Enter]);
    assert!(
        f.screen(100, 30)
            .contains("bar width 500 isn't from 1 to 100")
    );
    for _ in 0..3 {
        f.press(&[KeyCode::Backspace]);
    }
    f.typing("6").press(&[KeyCode::Enter]);
    assert_eq!(f.app.doc.text(), "[bar]\nwidth = 6\n");
}

#[test]
fn asks_before_quitting_with_unsaved_changes() {
    let mut f = Fixture::new(Some("[bar]\nwidth = 4\n"));
    f.open(Section::Bar)
        .typing("r")
        .press(&[KeyCode::Left])
        .typing("q");
    assert!(matches!(f.app.mode, Mode::Confirm(Confirm::Quit)));
    assert!(f.screen(100, 30).contains("Save your changes to"));
    f.press(&[KeyCode::Esc]);
    assert!(!f.app.quit);
    f.typing("q").typing("n");
    assert!(f.app.quit);
    assert_eq!(f.file(), "[bar]\nwidth = 4\n");

    let mut f = Fixture::new(Some("[bar]\nwidth = 4\n"));
    f.open(Section::Bar)
        .typing("r")
        .press(&[KeyCode::Left])
        .typing("qy");
    assert!(f.app.quit);
    assert_eq!(f.file(), "[bar]\n");
    assert_eq!(
        fs::read_to_string(f.app.path.with_file_name("config.toml.bak")).unwrap(),
        "[bar]\nwidth = 4\n"
    );
}

#[test]
fn quits_at_once_without_changes_and_on_a_second_ctrl_c() {
    let mut f = Fixture::new(None);
    f.typing("q");
    assert!(f.app.quit);

    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let mut f = Fixture::new(None);
    f.open(Section::Graph)
        .press(&[KeyCode::Enter, KeyCode::Backspace])
        .typing("4")
        .press(&[KeyCode::Enter, KeyCode::Enter]);
    // In the middle of a second edit: that one is dropped, and the first is asked about.
    f.app.handle(ctrl_c);
    assert!(matches!(f.app.mode, Mode::Confirm(Confirm::Quit)));
    assert!(!f.app.quit);
    f.app.handle(ctrl_c);
    assert!(f.app.quit);
    assert!(!f.app.path.exists());
}

#[test]
fn asks_before_overwriting_a_file_changed_on_disk() {
    let mut f = Fixture::new(Some("[bar]\nwidth = 4\n"));
    f.open(Section::Bar).typing("r");
    fs::write(&f.app.path, "[bar]\nwidth = 9\n").unwrap();
    f.typing("s");
    assert!(matches!(f.app.mode, Mode::Confirm(Confirm::Overwrite)));
    f.typing("r");
    assert_eq!(f.app.doc.get(Field::BarWidth).text, "9");
    assert!(!f.app.dirty());

    f.typing("r").typing("s");
    assert!(matches!(f.app.mode, Mode::Normal));
    fs::write(&f.app.path, "changed").unwrap();
    f.press(&[KeyCode::Enter, KeyCode::Backspace, KeyCode::Backspace])
        .typing("1")
        .press(&[KeyCode::Enter])
        .typing("s");
    assert!(matches!(f.app.mode, Mode::Confirm(Confirm::Overwrite)));
    f.typing("o");
    assert_eq!(f.file(), "[bar]\nwidth = 1\n");
}

#[test]
fn offers_to_print_a_read_only_config() {
    let mut f = Fixture::new(Some("[bar]\nwidth = 4\n"));
    fs::set_permissions(&f.app.path, fs::Permissions::from_mode(0o444)).unwrap();
    f.open(Section::Bar).typing("r").typing("s");
    assert!(matches!(f.app.mode, Mode::Confirm(Confirm::ReadOnly(_))));
    assert!(f.screen(100, 30).contains("read-only"));
    f.typing("p");
    assert!(f.app.print_on_exit);
    f.typing("q");
    assert!(f.app.quit);
    assert_eq!(f.app.file_text(), "[bar]\n");
    assert_eq!(f.file(), "[bar]\nwidth = 4\n");
}

#[test]
fn opens_a_config_with_a_mistake_and_saves_once_it_is_fixed() {
    let mut f = Fixture::new(Some("[colors]\nmodel = \"purple\"  # my color\n"));
    assert!(
        f.message()
            .contains("line 2, column 9: unknown style \"purple\""),
        "{}",
        f.message()
    );
    assert!(f.screen(110, 30).contains("Claude Code shows the defaults"));

    f.open(Section::Bar)
        .press(&[KeyCode::Enter, KeyCode::Backspace, KeyCode::Backspace])
        .typing("4")
        .press(&[KeyCode::Enter]);
    f.typing("s");
    assert!(
        f.message().starts_with("Not saved: line 2"),
        "{}",
        f.message()
    );

    // The picker starts on the first cell for a color it doesn't know; one step right is red.
    f.open(Section::Segment(Kind::Model)).press(&[
        KeyCode::Down,
        KeyCode::Enter,
        KeyCode::Right,
        KeyCode::Enter,
    ]);
    f.typing("s");
    assert!(f.message().starts_with("Saved"), "{}", f.message());
    assert_eq!(
        f.file(),
        "[colors]\nmodel = \"red\"  # my color\n\n[bar]\nwidth = 4\n"
    );
}

#[test]
fn sets_up_claude_code() {
    let mut f = Fixture::new(None);
    let settings = f.dir.path().join("claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, "{\"theme\": \"dark\"}").unwrap();
    f.app.refresh_claude();
    assert_eq!(f.app.claude_status, claude::Status::NotSet);

    f.open(Section::ClaudeCode).press(&[KeyCode::Enter]);
    assert!(
        f.screen(100, 30)
            .contains("statusLine.command: /usr/bin/astro-statusline")
    );
    f.typing("y");
    assert_eq!(f.app.claude_status, claude::Status::Runs(EXE.to_string()));
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(json["theme"], "dark");
    assert_eq!(json["statusLine"]["command"], EXE);
    assert_eq!(
        fs::read_to_string(settings.with_file_name("settings.json.bak")).unwrap(),
        "{\"theme\": \"dark\"}"
    );
}

#[test]
fn shows_a_question_s_keys_however_long_its_text() {
    let mut f = Fixture::new(None);
    let settings = f.dir.path().join("claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, "{}").unwrap();
    f.app.exe = PathBuf::from(format!(
        "/home/someone/{}/target/debug/astro-statusline",
        "deep/".repeat(12)
    ));
    f.open(Section::ClaudeCode).press(&[KeyCode::Enter]);
    for (width, height) in [(110, 32), (72, 20)] {
        let screen = f.screen(width, height);
        assert!(screen.contains("y change it"), "{screen}");
        // With room for all of it, none of the text is cut off.
        if height > 20 {
            assert!(screen.contains("kept as settings.json.bak."), "{screen}");
        }
    }
}

#[test]
fn counts_lines_as_they_wrap_at_words() {
    assert_eq!(ui::wrapped_lines("", 10), 1);
    assert_eq!(ui::wrapped_lines("one two", 7), 1);
    assert_eq!(ui::wrapped_lines("one two three", 7), 2);
    // A word longer than the width is split across lines.
    assert_eq!(ui::wrapped_lines("abcdefghijklmnop", 5), 4);
    assert_eq!(ui::wrapped_lines("ab abcdefghij", 5), 3);
    // An indent takes room on the first line.
    assert_eq!(ui::wrapped_lines("  abc de", 7), 2);
    assert_eq!(ui::wrapped_lines("  abc de", 8), 1);
}

#[test]
fn draws_the_preview_for_each_session_and_width() {
    let mut f = Fixture::new(None);
    let screen = f.screen(110, 30);
    assert!(
        screen.contains("Preview · Mid-session · 108 columns"),
        "{screen}"
    );
    assert!(screen.contains("  Opus 5.5 │ my-project │ main │ ctx 42% │ 5h 35% │ 7d 18% │ $1.23"));
    f.typing("]");
    assert!(f.screen(110, 30).contains("Fresh session"));
    f.typing("[[");
    assert!(f.screen(110, 30).contains("Outside git"));
    for _ in 0..60 {
        f.typing("<");
    }
    let screen = f.screen(110, 30);
    assert!(screen.contains("· 48 columns"), "{screen}");
    assert!(screen.contains("┊"));
    assert!(!screen.contains("$1.23"));
    for _ in 0..100 {
        f.typing(">");
    }
    assert_eq!(f.app.columns, None);
}

#[test]
fn flags_glyphs_of_uncertain_width() {
    let mut f = Fixture::new(Some("format.branch = \"\u{e0a0} {branch}\"\n"));
    assert!(f.screen(110, 30).contains("terminals and fonts disagree"));
}

#[test]
fn asks_for_a_bigger_terminal() {
    let mut f = Fixture::new(None);
    assert!(
        f.screen(60, 15)
            .contains("Make the terminal at least 72×20")
    );
}

#[test]
fn edits_text_at_the_cursor() {
    let mut input = Input::new("aé日");
    input.key(KeyCode::Left);
    input.key(KeyCode::Backspace);
    input.insert("{x}");
    input.key(KeyCode::Home);
    input.key(KeyCode::Delete);
    input.key(KeyCode::End);
    input.key(KeyCode::Char('!'));
    assert_eq!(input.text(), "{x}日!");
    assert!(!input.key(KeyCode::Enter));
}

#[test]
fn shortens_toml_errors_to_one_line() {
    let error = config::parse("[colors]\nmodel = \"purple\"").unwrap_err();
    assert!(error.contains('\n'));
    let short = short_error(&error);
    assert!(
        short.starts_with("line 2, column 9: unknown style \"purple\""),
        "{short}"
    );
    assert_eq!(
        short_error("set segments or lines, not both"),
        "set segments or lines, not both"
    );
}

#[test]
fn tells_dark_cells_from_light() {
    for dark in [0, 4, 16, 232] {
        assert!(ui::is_dark(dark), "{dark}");
    }
    for light in [11, 15, 231, 255] {
        assert!(!ui::is_dark(light), "{light}");
    }
}

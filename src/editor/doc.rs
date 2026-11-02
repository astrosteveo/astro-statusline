//! The config file as the editor changes it: a TOML document that keeps the user's comments, key
//! order and formatting. Each key the editor offers is a `Field`, checked the way the config file
//! is, and a key the file leaves out reads as its default.

use std::sync::LazyLock;

use toml_edit::{Array, DocumentMut, Item, Key, Table, TableLike, Value};

use crate::config::{self, Config, DEFAULTS};
use crate::segments::{Item as Entry, Kind};
use crate::style::Style;

static DEFAULT_DOC: LazyLock<DocumentMut> =
    LazyLock::new(|| DEFAULTS.parse().expect("the defaults parse"));

/// A key the editor offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Template(Kind),
    /// A style in `[colors]`, by its key.
    Style(&'static str),
    /// The separator of a row, counted from 0.
    Separator(usize),
    Padding,
    BarWidth,
    BarFull,
    BarEmpty,
    BarMarker,
    GraphWidth,
    GraphChars,
    /// A percentage segment's `[thresholds]`.
    Thresholds(Kind),
    /// A segment's `[show_from]`, which has no default.
    ShowFrom(Kind),
    /// A segment's `[max_width]`, which has no default.
    MaxWidth(Kind),
}

/// The TOML type a field is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Text,
    Integer,
    /// An integer or a float.
    Number,
    List,
}

impl Field {
    /// The table and key the field is written to. An empty table is the top of the file.
    pub fn path(self) -> (&'static str, &'static str) {
        match self {
            Field::Template(kind) => ("format", kind.name()),
            Field::Style(key) => ("colors", key),
            Field::Separator(_) => ("format", "separator"),
            Field::Padding => ("", "padding"),
            Field::BarWidth => ("bar", "width"),
            Field::BarFull => ("bar", "full"),
            Field::BarEmpty => ("bar", "empty"),
            Field::BarMarker => ("bar", "marker"),
            Field::GraphWidth => ("graph", "width"),
            Field::GraphChars => ("graph", "chars"),
            Field::Thresholds(kind) => ("thresholds", kind.name()),
            Field::ShowFrom(kind) => ("show_from", kind.name()),
            Field::MaxWidth(kind) => ("max_width", kind.name()),
        }
    }

    fn shape(self) -> Shape {
        match self {
            Field::BarWidth | Field::GraphWidth | Field::Padding | Field::MaxWidth(_) => {
                Shape::Integer
            }
            Field::ShowFrom(_) => Shape::Number,
            Field::Thresholds(_) => Shape::List,
            _ => Shape::Text,
        }
    }

    /// Whether the field has no default, so that leaving it empty takes it out of the file.
    pub fn optional(self) -> bool {
        matches!(self, Field::ShowFrom(_) | Field::MaxWidth(_))
    }

    /// Checks `text` as the field's value, giving the TOML value to write or what is wrong.
    pub fn value(self, text: &str) -> Result<Value, String> {
        match self {
            Field::ShowFrom(_) => {
                let number: f64 = text
                    .trim()
                    .parse()
                    .map_err(|_| format!("{text:?} isn't a number"))?;
                let number = config::check_show_from(number)?;
                // A whole number is written as one, as a person would.
                return Ok(if number.fract() == 0.0 && number < 1e15 {
                    Value::from(number as i64)
                } else {
                    Value::from(number)
                });
            }
            Field::Template(kind) => kind.parse_template(text).map(drop),
            Field::Style(key) => text.parse::<Style>().and_then(|style| {
                match Kind::from_name(key).is_some_and(Kind::has_usage) {
                    true => Ok(()),
                    false => config::check_plain_style(&style),
                }
            }),
            Field::Separator(_) => Ok(()),
            Field::BarFull | Field::BarEmpty | Field::BarMarker => config::check_cell(text),
            Field::GraphChars => config::check_graph_chars(text).map(drop),
            Field::BarWidth | Field::GraphWidth | Field::Padding | Field::MaxWidth(_) => {
                let number: u64 = text
                    .trim()
                    .parse()
                    .map_err(|_| format!("{text:?} isn't a whole number"))?;
                match self {
                    Field::BarWidth => config::check_bar_width(number).map(drop),
                    Field::GraphWidth => config::check_graph_width(number).map(drop),
                    Field::MaxWidth(_) => config::check_max_width(number).map(drop),
                    _ => config::check_padding(number).map(drop),
                }?;
                // Checked to be at most 500 above.
                return Ok(Value::from(number as i64));
            }
            Field::Thresholds(_) => {
                // Brackets are optional, so "60, 85" and "[60, 85]" both read.
                let numbers = text
                    .trim()
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split([',', ' '])
                    .filter(|word| !word.is_empty())
                    .map(|word| {
                        word.parse::<i64>()
                            .map_err(|_| format!("{word:?} isn't a whole number"))
                    })
                    .collect::<Result<Vec<i64>, String>>()?;
                let pair = config::check_thresholds(&numbers)?;
                return Ok(Value::Array(pair.iter().map(|&n| i64::from(n)).collect()));
            }
        }?;
        Ok(Value::from(text))
    }
}

/// A field as the editor shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    /// Whether the file sets it, rather than leaving it at its default.
    pub set: bool,
    /// What is wrong with the file's value, if anything.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Doc(DocumentMut);

impl Doc {
    /// Parses a config file's text. An error is TOML that doesn't parse at all.
    pub fn parse(text: &str) -> Result<Doc, String> {
        text.parse()
            .map(Doc)
            .map_err(|error: toml_edit::TomlError| error.to_string().trim_end().to_string())
    }

    pub fn text(&self) -> String {
        self.0.to_string()
    }

    /// The field's text in the file, or its default when the file leaves it out.
    pub fn get(&self, field: Field) -> Shown {
        let (table, key) = field.path();
        if let (Field::Separator(row), Some(Item::Value(Value::Array(list)))) =
            (field, lookup(&self.0, table, key))
        {
            let item = list.get(row).or_else(|| list.iter().last());
            return match item.map(|value| value.as_str()) {
                Some(Some(text)) => Shown {
                    text: text.to_string(),
                    set: true,
                    error: None,
                },
                _ => Shown {
                    text: String::new(),
                    set: true,
                    error: Some("format.separator must be text, or a list of text".to_string()),
                },
            };
        }
        let Some(item) = lookup(&self.0, table, key) else {
            // An optional field's default is to be left out.
            let default = lookup(&DEFAULT_DOC, table, key).map(text_of);
            return Shown {
                text: default.unwrap_or_default(),
                set: false,
                error: None,
            };
        };
        let text = text_of(item);
        let error = match (field.shape(), item) {
            (Shape::Integer, item) if item.is_integer() => field.value(&text).err(),
            (Shape::Integer | Shape::Number, _) if item.is_str() => {
                Some(format!("{text} isn't a number; write it without quotes"))
            }
            (Shape::Integer, _) => Some(format!("{text} isn't a whole number")),
            (Shape::Number, item) if item.is_integer() || item.is_float() => {
                field.value(&text).err()
            }
            (Shape::Number, _) => Some(format!("{text} isn't a number")),
            (Shape::List, item) if item.is_array() => field.value(&text).err(),
            (Shape::List, _) => Some(format!("{text} isn't a list; write it as [50, 80]")),
            (Shape::Text, item) if item.is_str() => field.value(&text).err(),
            (Shape::Text, _) => Some(format!("{text} isn't text; put it in quotes")),
        };
        Shown {
            text,
            set: true,
            error,
        }
    }

    /// Sets the field from the text typed into it, or says what is wrong with the text. An optional
    /// field left empty is taken out.
    pub fn put(&mut self, field: Field, text: &str) -> Result<(), String> {
        if field.optional() && text.trim().is_empty() {
            self.reset(field);
        } else {
            self.set(field, field.value(text)?);
        }
        Ok(())
    }

    /// Sets the field, keeping any comment after the value it replaces.
    pub fn set(&mut self, field: Field, value: Value) {
        match field {
            Field::Separator(row) => self.set_separator(row, value),
            field => self.set_at(field.path(), value),
        }
    }

    fn set_at(&mut self, (table, key): (&str, &str), value: Value) {
        let inline = self.0.get(table).is_some_and(Item::is_inline_table);
        let table = table_mut(&mut self.0, table);
        match table.get_mut(key) {
            Some(Item::Value(old)) => {
                let decor = old.decor().clone();
                *old = value;
                *old.decor_mut() = decor;
            }
            _ => {
                table.insert(key, Item::Value(value));
                // An inline table keeps the space before its `}` on the old last entry, so a new
                // entry would land after it. Its own comments can't be lost: it's one line.
                if inline {
                    table.fmt();
                }
            }
        }
    }

    /// Sets one row's separator. When every row then has the same one, it's written as one text,
    /// and otherwise as a list with one for each row.
    fn set_separator(&mut self, row: usize, value: Value) {
        let rows = self.rows().len().max(row + 1);
        let mut each: Vec<String> = (0..rows)
            .map(|row| self.get(Field::Separator(row)).text)
            .collect();
        each[row] = value.as_str().unwrap_or_default().to_string();
        let value = if each.iter().all(|text| *text == each[0]) {
            Value::from(each.swap_remove(0))
        } else {
            Value::Array(each.iter().collect())
        };
        self.set_at(Field::Separator(row).path(), value);
    }

    /// Takes the field out of the file, so it reads as its default.
    pub fn reset(&mut self, field: Field) {
        let (table, key) = field.path();
        let table = match table {
            "" => Some(self.0.as_table_mut() as &mut dyn TableLike),
            table => self.0.get_mut(table).and_then(Item::as_table_like_mut),
        };
        if let Some(table) = table {
            table.remove(key);
        }
    }

    /// The rows the file lays out, top to bottom, leaving out names that aren't segments or a
    /// fill. With neither `segments` nor `lines`, that's every segment on one row.
    pub fn rows(&self) -> Vec<Vec<Entry>> {
        let kinds = |value: &Value| -> Vec<Entry> {
            value.as_array().map_or_else(Vec::new, |array| {
                array
                    .iter()
                    .filter_map(|name| name.as_str().and_then(Entry::from_name))
                    .collect()
            })
        };
        let root = self.0.as_table();
        if let Some(segments) = root.get("segments").and_then(Item::as_value) {
            return vec![kinds(segments)];
        }
        if let Some(lines) = root.get("lines").and_then(Item::as_array) {
            return lines.iter().map(kinds).collect();
        }
        vec![Entry::ALL.to_vec()]
    }

    /// Writes `rows` as `segments` when there is one and the file doesn't use `lines`, otherwise
    /// as `lines`, one row to a line. Comments above the key it replaces stay.
    pub fn set_rows(&mut self, rows: &[Vec<Entry>]) {
        let root = self.0.as_table_mut();
        let as_lines =
            rows.len() != 1 || (root.contains_key("lines") && !root.contains_key("segments"));
        let (key, other, value) = if as_lines {
            ("lines", "segments", lines_value(rows))
        } else {
            ("segments", "lines", Value::Array(names(&rows[0])))
        };
        let replaced = root.remove_entry(other);
        if let Some(Item::Value(old)) = root.get_mut(key) {
            let decor = old.decor().clone();
            *old = value;
            *old.decor_mut() = decor;
            return;
        }
        let mut new_key = Key::new(key);
        if let Some((old_key, _)) = replaced {
            new_key = new_key.with_leaf_decor(old_key.leaf_decor().clone());
        }
        root.insert_formatted(&new_key, Item::Value(value));
    }

    /// Takes the layout out of the file, so it shows every segment on one row.
    pub fn reset_rows(&mut self) {
        let root = self.0.as_table_mut();
        root.remove("segments");
        root.remove("lines");
    }

    /// Whether the file sets `drop_order`.
    pub fn has_drop_order(&self) -> bool {
        self.0.contains_key("drop_order")
    }

    /// Every segment in the order they drop on a narrow terminal, first to last, leaving out names
    /// in `drop_order` that aren't segments or come twice.
    pub fn drop_sequence(&self) -> Vec<Kind> {
        let mut listed: Vec<Kind> = Vec::new();
        let names = self.0.get("drop_order").and_then(Item::as_array);
        for kind in names
            .into_iter()
            .flatten()
            .filter_map(|name| name.as_str().and_then(Kind::from_name))
        {
            if !listed.contains(&kind) {
                listed.push(kind);
            }
        }
        let config = Config {
            drop_order: listed,
            ..Config::default()
        };
        config.drop_sequence()
    }

    /// Writes `drop_order` so segments drop in `sequence`, naming only those the default order
    /// doesn't already put there. A segment added later then drops in its default place, not after
    /// everything named. The default order takes the key out.
    pub fn set_drop_sequence(&mut self, sequence: &[Kind]) {
        let default_from = |start: usize| {
            let mut rest = sequence[start..].to_vec();
            rest.sort_by_key(|kind| kind.default_priority());
            rest == sequence[start..]
        };
        let listed = (0..=sequence.len())
            .find(|&start| default_from(start))
            .unwrap_or(sequence.len());
        let root = self.0.as_table_mut();
        if listed == 0 {
            root.remove("drop_order");
            return;
        }
        let value = Value::Array(sequence[..listed].iter().map(|kind| kind.name()).collect());
        match root.get_mut("drop_order") {
            Some(Item::Value(old)) => {
                let decor = old.decor().clone();
                *old = value;
                *old.decor_mut() = decor;
            }
            _ => {
                root.insert("drop_order", Item::Value(value));
            }
        }
    }
}

fn lookup<'d>(doc: &'d DocumentMut, table: &str, key: &str) -> Option<&'d Item> {
    match table {
        "" => doc.get(key),
        table => doc.get(table)?.as_table_like()?.get(key),
    }
}

/// A value as the editor shows it: a string's contents, or the TOML of anything else.
fn text_of(item: &Item) -> String {
    match item.as_value() {
        Some(Value::String(text)) => text.value().clone(),
        Some(value) => {
            // A value's decor holds the spaces and any comment around it.
            let mut value = value.clone();
            value.decor_mut().clear();
            value.to_string()
        }
        None => item.to_string().trim().to_string(),
    }
}

/// The table called `name`, made as a `[name]` table at the end of the file when there is none.
fn table_mut<'d>(doc: &'d mut DocumentMut, name: &str) -> &'d mut dyn TableLike {
    let root = doc.as_table_mut();
    if name.is_empty() {
        return root;
    }
    if !root.get(name).is_some_and(Item::is_table_like) {
        root.insert(name, Item::Table(Table::new()));
    }
    root.get_mut(name)
        .and_then(Item::as_table_like_mut)
        .expect("the table was just made")
}

fn names(row: &[Entry]) -> Array {
    row.iter().map(|entry| entry.name()).collect()
}

fn lines_value(rows: &[Vec<Entry>]) -> Value {
    let mut array = Array::new();
    for row in rows {
        let mut row = Value::Array(names(row));
        row.decor_mut().set_prefix("\n  ");
        array.push_formatted(row);
    }
    if !rows.is_empty() {
        array.set_trailing_comma(true);
        array.set_trailing("\n");
    }
    Value::Array(array)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Doc {
        Doc::parse(text).unwrap()
    }

    /// Rows of segments as rows of entries.
    fn items(rows: &[&[Kind]]) -> Vec<Vec<Entry>> {
        rows.iter()
            .map(|row| row.iter().map(|&kind| Entry::Segment(kind)).collect())
            .collect()
    }

    #[test]
    fn reads_and_writes_a_fill() {
        let mut doc = doc("segments = [\"model\", \"fill\", \"cost\"]\n");
        assert_eq!(
            doc.rows(),
            [[
                Entry::Segment(Kind::Model),
                Entry::Fill,
                Entry::Segment(Kind::Cost)
            ]]
        );
        doc.set_rows(&[vec![Entry::Fill, Entry::Segment(Kind::Cost)]]);
        assert_eq!(doc.text(), "segments = [\"fill\", \"cost\"]\n");
    }

    #[test]
    fn reads_the_file_or_the_default() {
        let doc = doc("[colors]\nmodel = \"bold red\"\n[bar]\nwidth = 6\n");
        let shown = doc.get(Field::Style("model"));
        assert_eq!((shown.text.as_str(), shown.set), ("bold red", true));
        let shown = doc.get(Field::Style("branch"));
        assert_eq!((shown.text.as_str(), shown.set), ("magenta", false));
        assert_eq!(doc.get(Field::BarWidth).text, "6");
        assert_eq!(doc.get(Field::GraphWidth).text, "8");
        assert_eq!(
            doc.get(Field::Template(Kind::FiveHour)).text,
            "5h {percent}"
        );
        assert_eq!(doc.get(Field::Separator(0)).text, " │ ");
    }

    #[test]
    fn reads_and_writes_thresholds() {
        let mut doc = doc("thresholds.context = [60, 85]  # mine\nthresholds.seven_day = 50\n");
        let field = Field::Thresholds(Kind::Context);
        assert_eq!(doc.get(field).text, "[60, 85]");
        assert_eq!(doc.get(Field::Thresholds(Kind::FiveHour)).text, "[50, 80]");
        assert!(
            doc.get(Field::Thresholds(Kind::SevenDay))
                .error
                .unwrap()
                .contains("isn't a list")
        );

        for text in ["70, 90", "[70, 90]", " 70 90 "] {
            doc.set(field, field.value(text).unwrap());
            assert_eq!(doc.get(field).text, "[70, 90]", "{text}");
        }
        assert!(
            doc.text()
                .starts_with("thresholds.context = [70, 90]  # mine\n")
        );
        for text in ["90, 70", "50", "a, b", "50, 80, 90", ""] {
            assert!(field.value(text).is_err(), "{text}");
        }
    }

    #[test]
    fn reads_a_number_without_the_comment_after_it() {
        let doc = doc("[bar]\nwidth = 6  # six cells\n");
        let shown = doc.get(Field::BarWidth);
        assert_eq!((shown.text.as_str(), shown.error), ("6", None));
    }

    #[test]
    fn shows_what_is_wrong_with_a_value() {
        let doc = doc(
            "colors.model = \"purple\"\ncolors.branch = 208\nbar.width = \"6\"\nbar.full = \"==\"\n\
             format.cost = \"{percent}\"\ngraph.width = 0\n",
        );
        let error = |field| doc.get(field).error.unwrap();
        assert!(error(Field::Style("model")).contains("unknown style \"purple\""));
        assert!(error(Field::Style("branch")).contains("put it in quotes"));
        assert!(error(Field::BarWidth).contains("without quotes"));
        assert!(error(Field::BarFull).contains("isn't one column wide"));
        assert!(error(Field::Template(Kind::Cost)).contains("unknown placeholder {percent}"));
        assert!(error(Field::GraphWidth).contains("isn't from 1 to 64"));
    }

    #[test]
    fn checks_values_as_the_config_file_does() {
        assert_eq!(
            Field::BarWidth.value(" 12 ").unwrap().as_integer(),
            Some(12)
        );
        assert!(Field::BarWidth.value("101").is_err());
        assert!(Field::BarWidth.value("-1").is_err());
        assert!(Field::BarWidth.value("wide").is_err());
        assert!(Field::GraphWidth.value("64").is_ok());
        assert!(Field::GraphWidth.value("65").is_err());
        assert_eq!(
            Field::Template(Kind::Model)
                .value("[{name}]")
                .unwrap()
                .as_str(),
            Some("[{name}]")
        );
        assert!(Field::Template(Kind::Model).value("{percent}").is_err());
        assert!(Field::Style("model").value("bold #ff8700").is_ok());
        assert!(Field::Style("model").value("red blue").is_err());
        assert!(Field::BarMarker.value("|").is_ok());
        assert!(Field::GraphChars.value("x").is_err());
        assert!(Field::Separator(0).value("").is_ok());
    }

    #[test]
    fn edits_keep_comments_order_and_formatting() {
        let text = "# My status line.\n\n[format]\n# Shown between segments.\nseparator = \" · \"  # a dot\ncost = \"{usd}\"\n\n[colors]\nmodel = \"cyan\"\n";
        let mut doc = doc(text);
        doc.set(Field::Separator(0), Value::from(" | "));
        doc.set(Field::Style("branch"), Value::from("bold"));
        doc.set(Field::BarWidth, Value::from(6));
        assert_eq!(
            doc.text(),
            "# My status line.\n\n[format]\n# Shown between segments.\nseparator = \" | \"  # a dot\ncost = \"{usd}\"\n\n[colors]\nmodel = \"cyan\"\nbranch = \"bold\"\n\n[bar]\nwidth = 6\n"
        );
        doc.reset(Field::Style("model"));
        doc.reset(Field::GraphChars);
        assert!(!doc.text().contains("model"));
        assert!(config::parse(&doc.text()).is_ok());
    }

    #[test]
    fn edits_dotted_and_inline_tables_in_place() {
        let mut dotted = doc("colors.model = \"cyan\"\n");
        dotted.set(Field::Style("cost"), Value::from("red"));
        assert_eq!(
            dotted.text(),
            "colors.model = \"cyan\"\ncolors.cost = \"red\"\n"
        );

        let mut inline = doc("colors = { model = \"cyan\" }\n");
        inline.set(Field::Style("cost"), Value::from("red"));
        assert_eq!(
            inline.text(),
            "colors = { model = \"cyan\", cost = \"red\" }\n"
        );
    }

    #[test]
    fn writes_one_row_as_segments_and_several_as_lines() {
        let mut doc =
            doc("# Which segments.\nsegments = [\"model\"]\n\n[colors]\nmodel = \"cyan\"\n");
        assert_eq!(doc.rows(), [[Kind::Model]]);

        doc.set_rows(&items(&[&[Kind::Model, Kind::Cost]]));
        assert_eq!(
            doc.text(),
            "# Which segments.\nsegments = [\"model\", \"cost\"]\n\n[colors]\nmodel = \"cyan\"\n"
        );

        let rows = items(&[&[Kind::Model, Kind::Branch], &[], &[Kind::Cost]]);
        doc.set_rows(&rows);
        assert_eq!(
            doc.text(),
            "# Which segments.\nlines = [\n  [\"model\", \"branch\"],\n  [],\n  [\"cost\"],\n]\n\n[colors]\nmodel = \"cyan\"\n"
        );
        assert_eq!(doc.rows(), rows);
        assert_eq!(config::parse(&doc.text()).unwrap().rows().len(), 3);

        // A file that uses `lines` keeps it for one row.
        doc.set_rows(&items(&[&[Kind::Context]]));
        assert!(
            doc.text().contains("lines = [\n  [\"context\"],\n]"),
            "{}",
            doc.text()
        );

        doc.set_rows(&[]);
        assert!(doc.text().contains("lines = []"));
        assert_eq!(doc.rows(), Vec::<Vec<Entry>>::new());

        doc.reset_rows();
        assert_eq!(doc.rows(), [Kind::ALL]);
    }

    #[test]
    fn reads_rows_leniently() {
        assert_eq!(Doc::parse("").unwrap().rows(), [Kind::ALL]);
        let doc = doc("lines = [[\"model\", \"git\"], \"cost\"]");
        assert_eq!(doc.rows(), [vec![Kind::Model], vec![]]);
    }

    #[test]
    fn writes_one_separator_or_one_per_row() {
        let rows = "lines = [[\"model\"], [\"cost\"], [\"context\"]]\n";
        let mut doc = doc(&format!("{rows}[format]\nseparator = \" | \"  # mine\n"));
        let separator = |doc: &Doc, row| doc.get(Field::Separator(row)).text;
        assert_eq!(separator(&doc, 2), " | ");

        doc.set(Field::Separator(1), Value::from(" · "));
        assert_eq!(
            doc.text(),
            format!("{rows}[format]\nseparator = [\" | \", \" · \", \" | \"]  # mine\n")
        );
        assert_eq!(
            (separator(&doc, 0), separator(&doc, 1), separator(&doc, 7)),
            (" | ".into(), " · ".into(), " | ".into())
        );
        assert_eq!(
            config::parse(&doc.text()).unwrap().format.separator.row(1),
            " · "
        );

        // Back to the same for every row, it's one text again.
        doc.set(Field::Separator(1), Value::from(" | "));
        assert_eq!(
            doc.text(),
            format!("{rows}[format]\nseparator = \" | \"  # mine\n")
        );

        let doc = Doc::parse("format.separator = [1]").unwrap();
        assert!(doc.get(Field::Separator(0)).error.is_some());
    }

    #[test]
    fn reads_and_writes_show_from_which_has_no_default() {
        let mut doc = doc("[show_from]\ncost = 1.5\nseven_day = \"50\"\n");
        let field = Field::ShowFrom(Kind::Context);
        assert_eq!(
            doc.get(field),
            Shown {
                text: String::new(),
                set: false,
                error: None
            }
        );
        assert_eq!(doc.get(Field::ShowFrom(Kind::Cost)).text, "1.5");
        let error = doc.get(Field::ShowFrom(Kind::SevenDay)).error.unwrap();
        assert!(error.contains("without quotes"), "{error}");

        doc.set(field, field.value("50").unwrap());
        doc.set(Field::ShowFrom(Kind::Cost), field.value(" 0.25 ").unwrap());
        assert_eq!(
            doc.text(),
            "[show_from]\ncost = 0.25\nseven_day = \"50\"\ncontext = 50\n"
        );
        for text in ["-1", "", "lots", "inf"] {
            assert!(field.value(text).is_err(), "{text}");
        }
        assert!(field.optional() && !Field::Padding.optional());
    }

    #[test]
    fn edits_the_padding_at_the_top_of_the_file() {
        let mut doc = doc("[colors]\nmodel = \"cyan\"\n");
        assert_eq!(doc.get(Field::Padding).text, "0");
        doc.set(Field::Padding, Field::Padding.value("2").unwrap());
        assert_eq!(doc.text(), "padding = 2\n[colors]\nmodel = \"cyan\"\n");
        assert!(Field::Padding.value("101").is_err());
        doc.reset(Field::Padding);
        assert_eq!(doc.text(), "[colors]\nmodel = \"cyan\"\n");
    }

    #[test]
    fn writes_the_shortest_drop_order() {
        use Kind::*;
        let mut doc =
            doc("# Mine.\ndrop_order = [\"branch\", \"git\", \"branch\"]  # first\n[colors]\n");
        assert_eq!(
            doc.drop_sequence(),
            [Branch, Cost, SevenDay, FiveHour, Directory, Model, Context]
        );
        // Moving the cost first leaves the rest in the default order after it.
        doc.set_drop_sequence(&[Cost, Branch, SevenDay, FiveHour, Directory, Model, Context]);
        assert_eq!(
            doc.text(),
            "# Mine.\ndrop_order = [\"cost\", \"branch\"]  # first\n[colors]\n"
        );
        doc.set_drop_sequence(&[Cost, SevenDay, FiveHour, Branch, Directory, Model, Context]);
        assert_eq!(doc.text(), "[colors]\n");
        assert!(!doc.has_drop_order());

        let mut doc = Doc::parse("[colors]\n").unwrap();
        doc.set_drop_sequence(&[Cost, SevenDay, FiveHour, Branch, Directory, Context, Model]);
        assert_eq!(
            doc.text(),
            "drop_order = [\"cost\", \"seven_day\", \"five_hour\", \"branch\", \"directory\", \"context\"]\n[colors]\n"
        );
        assert!(config::parse(&doc.text()).is_ok());
    }

    #[test]
    fn reports_toml_that_does_not_parse() {
        let error = Doc::parse("segments = [").unwrap_err();
        assert!(error.contains("line 1"), "{error}");
    }
}

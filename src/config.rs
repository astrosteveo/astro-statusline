//! Which segments the status line shows, in what order, and the style of each, read from
//! `config.toml` in the user's config directory. Every key is optional and falls back to the
//! default.

use std::fs;
use std::io;
use std::path::Path;

use serde::Deserialize;
use serde::de::{self, Deserializer};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::dirs;
use crate::history;
use crate::segments::Kind;
use crate::style::Style;
use crate::template::Template;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// One row of segments, left to right.
    pub segments: Option<Vec<Kind>>,
    /// Several rows, each a list of segments, top to bottom. Set this or `segments`, not both.
    pub lines: Option<Vec<Vec<Kind>>>,
    pub colors: Colors,
    pub format: Format,
    pub bar: Bar,
    pub graph: Graph,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    pub model: Style,
    pub directory: Style,
    pub branch: Style,
    pub cost: Style,
    pub separator: Style,
    /// Context and rate-limit percentages below 50%.
    pub usage_low: Style,
    /// Context and rate-limit percentages from 50% up to 80%.
    pub usage_medium: Style,
    /// Context and rate-limit percentages from 80% up.
    pub usage_high: Style,
}

/// The text of each part of the line. Each segment's template is checked against the
/// placeholders that segment has, so a mistake is reported with its line and column.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Format {
    /// Drawn between segments, in `colors.separator`.
    pub separator: String,
    #[serde(deserialize_with = "model_template")]
    pub model: Template,
    #[serde(deserialize_with = "directory_template")]
    pub directory: Template,
    #[serde(deserialize_with = "branch_template")]
    pub branch: Template,
    #[serde(deserialize_with = "context_template")]
    pub context: Template,
    #[serde(deserialize_with = "five_hour_template")]
    pub five_hour: Template,
    #[serde(deserialize_with = "seven_day_template")]
    pub seven_day: Template,
    #[serde(deserialize_with = "cost_template")]
    pub cost: Template,
}

/// How `{bar}` is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bar {
    /// Cells in the bar, from 1 to 100.
    #[serde(deserialize_with = "bar_width")]
    pub width: u8,
    /// The cell for each filled step, one column wide.
    #[serde(deserialize_with = "cell")]
    pub full: String,
    /// The cell for each step not yet filled, one column wide.
    #[serde(deserialize_with = "cell")]
    pub empty: String,
}

/// How `{graph}` is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Graph {
    /// The most recent samples shown, one cell each, from 1 to 64.
    #[serde(deserialize_with = "graph_width")]
    pub width: u8,
    /// The cells from lowest to highest, each one column wide.
    #[serde(deserialize_with = "graph_chars")]
    pub chars: Vec<char>,
}

impl Config {
    /// The rows of segments to show, top to bottom. With neither `segments` nor `lines` set, that's
    /// every segment on one row.
    pub fn rows(&self) -> Vec<&[Kind]> {
        match (&self.segments, &self.lines) {
            (Some(segments), _) => vec![segments],
            (None, Some(lines)) => lines.iter().map(Vec::as_slice).collect(),
            (None, None) => vec![&Kind::ALL],
        }
    }

    /// Whether any segment shown needs the session's history: for a graph, or for the context
    /// segment's fill prediction.
    pub fn needs_history(&self) -> bool {
        self.rows().into_iter().flatten().any(|&kind| {
            let template = self.format.template(kind);
            template.uses("graph") || (kind == Kind::Context && template.uses("eta"))
        })
    }
}

impl Format {
    /// The template for `kind`.
    pub fn template(&self, kind: Kind) -> &Template {
        match kind {
            Kind::Model => &self.model,
            Kind::Directory => &self.directory,
            Kind::Branch => &self.branch,
            Kind::Context => &self.context,
            Kind::FiveHour => &self.five_hour,
            Kind::SevenDay => &self.seven_day,
            Kind::Cost => &self.cost,
        }
    }
}

impl Default for Graph {
    fn default() -> Self {
        Graph {
            width: 8,
            chars: "▁▂▃▄▅▆▇█".chars().collect(),
        }
    }
}

fn graph_width<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let width = u64::deserialize(deserializer)?;
    u8::try_from(width)
        .ok()
        .filter(|width| (1..=history::MAX_SAMPLES as u8).contains(width))
        .ok_or_else(|| {
            de::Error::custom(format!(
                "graph width {width} isn't from 1 to {}",
                history::MAX_SAMPLES
            ))
        })
}

fn graph_chars<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<char>, D::Error> {
    let text = String::deserialize(deserializer)?;
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 || chars.iter().any(|&c| c.width() != Some(1)) {
        return Err(de::Error::custom(format!(
            "graph chars {text:?} must be 2 or more characters, each one column wide, such as \
             \"▁▂▃▄▅▆▇█\""
        )));
    }
    Ok(chars)
}

impl Default for Bar {
    fn default() -> Self {
        Bar {
            width: 10,
            full: "█".to_string(),
            empty: "░".to_string(),
        }
    }
}

fn bar_width<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let width = u64::deserialize(deserializer)?;
    u8::try_from(width)
        .ok()
        .filter(|width| (1..=100).contains(width))
        .ok_or_else(|| de::Error::custom(format!("bar width {width} isn't from 1 to 100")))
}

/// One cell of a bar or graph: text exactly one column wide, so every cell takes the same room
/// and the line doesn't shift as a value changes.
fn cell<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.width() != 1 || text.chars().any(char::is_control) {
        return Err(de::Error::custom(format!(
            "{text:?} isn't one column wide; a cell is a single character such as \"█\" or \"#\""
        )));
    }
    Ok(text)
}

impl Default for Format {
    fn default() -> Self {
        let template = |kind: Kind, text: &str| {
            Template::parse(text, kind.placeholders()).expect("default templates parse")
        };
        Format {
            separator: " │ ".to_string(),
            model: template(Kind::Model, "{name}"),
            directory: template(Kind::Directory, "{name}"),
            branch: template(Kind::Branch, "{branch}"),
            context: template(Kind::Context, "ctx {percent}"),
            five_hour: template(Kind::FiveHour, "5h {percent}"),
            seven_day: template(Kind::SevenDay, "7d {percent}"),
            cost: template(Kind::Cost, "{usd}"),
        }
    }
}

fn template<'de, D: Deserializer<'de>>(deserializer: D, kind: Kind) -> Result<Template, D::Error> {
    let text = String::deserialize(deserializer)?;
    Template::parse(&text, kind.placeholders()).map_err(de::Error::custom)
}

fn model_template<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Template, D::Error> {
    template(deserializer, Kind::Model)
}

fn directory_template<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Template, D::Error> {
    template(deserializer, Kind::Directory)
}

fn branch_template<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Template, D::Error> {
    template(deserializer, Kind::Branch)
}

fn context_template<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Template, D::Error> {
    template(deserializer, Kind::Context)
}

fn five_hour_template<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Template, D::Error> {
    template(deserializer, Kind::FiveHour)
}

fn seven_day_template<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Template, D::Error> {
    template(deserializer, Kind::SevenDay)
}

fn cost_template<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Template, D::Error> {
    template(deserializer, Kind::Cost)
}

impl Default for Colors {
    fn default() -> Self {
        let style = |text: &str| text.parse().expect("default styles parse");
        Colors {
            model: style("cyan"),
            directory: style("blue"),
            branch: style("magenta"),
            cost: style("dim"),
            separator: style("dim"),
            usage_low: style("green"),
            usage_medium: style("yellow"),
            usage_high: style("red"),
        }
    }
}

/// Reads the config file, or the defaults when there is none. An error names the file and what
/// is wrong with it.
pub fn load() -> Result<Config, String> {
    dirs::config().map_or_else(
        || Ok(Config::default()),
        |dir| read(&dir.join("config.toml")),
    )
}

fn read(path: &Path) -> Result<Config, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Parses the text of a config file.
fn parse(text: &str) -> Result<Config, String> {
    // A parse error ends in a newline.
    let config: Config =
        toml::from_str(text).map_err(|error| error.to_string().trim_end().to_string())?;
    if config.segments.is_some() && config.lines.is_some() {
        return Err("set segments or lines, not both".to_string());
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(text: &str) -> Style {
        text.parse().unwrap()
    }

    #[test]
    fn the_documented_defaults_are_the_defaults() {
        let documented = r##"
            segments = ["model", "directory", "branch", "context", "five_hour", "seven_day", "cost"]

            [colors]
            model = "cyan"
            directory = "blue"
            branch = "magenta"
            cost = "dim"
            separator = "dim"
            usage_low = "green"
            usage_medium = "yellow"
            usage_high = "red"

            [format]
            separator = " │ "
            model = "{name}"
            directory = "{name}"
            branch = "{branch}"
            context = "ctx {percent}"
            five_hour = "5h {percent}"
            seven_day = "7d {percent}"
            cost = "{usd}"

            [bar]
            width = 10
            full = "█"
            empty = "░"

            [graph]
            width = 8
            chars = "▁▂▃▄▅▆▇█"
        "##;
        let documented = parse(documented).unwrap();
        assert_eq!(documented.rows(), Config::default().rows());
        assert_eq!(
            Config {
                segments: None,
                ..documented
            },
            Config::default()
        );
        assert_eq!(parse("").unwrap(), Config::default());
    }

    #[test]
    fn keys_left_out_keep_their_defaults() {
        let config = parse(
            r##"
            segments = ["context", "model"]
            colors.model = "bold #ff8700"
            "##,
        )
        .unwrap();
        assert_eq!(config.rows(), [[Kind::Context, Kind::Model]]);
        assert_eq!(config.colors.model, style("bold #ff8700"));
        assert_eq!(config.colors.branch, style("magenta"));
        assert_eq!(parse("segments = []").unwrap().rows(), [&[]]);
        assert_eq!(
            parse("format.separator = \"\"").unwrap().format.separator,
            ""
        );
    }

    #[test]
    fn lays_segments_out_in_rows() {
        assert_eq!(Config::default().rows(), [Kind::ALL]);
        let config = parse("lines = [[\"model\", \"branch\"], [], [\"cost\"]]").unwrap();
        assert_eq!(
            config.rows(),
            [&[Kind::Model, Kind::Branch][..], &[], &[Kind::Cost]]
        );
        assert_eq!(parse("lines = []").unwrap().rows(), Vec::<&[Kind]>::new());
        assert_eq!(
            parse("segments = [\"model\"]\nlines = [[\"cost\"]]").unwrap_err(),
            "set segments or lines, not both"
        );
        assert!(
            parse("lines = [\"model\"]")
                .unwrap_err()
                .contains("invalid type")
        );
        let graph_below = "lines = [[\"model\"], [\"cost\"]]\nformat.cost = \"{graph}\"";
        assert!(parse(graph_below).unwrap().needs_history());
    }

    #[test]
    fn rejects_unknown_keys_segments_and_styles() {
        let cases = [
            ("colours.model = \"red\"", "unknown field `colours`"),
            ("colors.modle = \"red\"", "unknown field `modle`"),
            ("segments = [\"git\"]", "unknown variant `git`"),
            ("segments = \"model\"", "invalid type"),
            ("colors.model = \"purple\"", "unknown style \"purple\""),
            ("colors.model = 208", "invalid type"),
            ("format.separator = 1", "invalid type"),
            ("format.sep = \" \"", "unknown field `sep`"),
            (
                "format.model = \"{percent}\"",
                "unknown placeholder {percent}: expected {name}",
            ),
            (
                "format.cost = \"{percent}\"",
                "unknown placeholder {percent}: expected {usd}",
            ),
            ("format.context = \"ctx [{percent}\"", "has a [ with no ]"),
            ("format.context = 5", "invalid type"),
            ("bar.width = 0", "bar width 0 isn't from 1 to 100"),
            ("bar.width = 101", "bar width 101 isn't from 1 to 100"),
            (
                "bar.width = 4000000000",
                "bar width 4000000000 isn't from 1 to 100",
            ),
            ("bar.width = -1", "invalid value"),
            ("bar.full = \"\"", "isn't one column wide"),
            ("bar.full = \"==\"", "isn't one column wide"),
            ("bar.full = \"日\"", "isn't one column wide"),
            ("bar.empty = \"\\t\"", "isn't one column wide"),
            ("graph.width = 0", "graph width 0 isn't from 1 to 64"),
            ("graph.width = 65", "graph width 65 isn't from 1 to 64"),
            ("graph.chars = \"#\"", "must be 2 or more characters"),
            ("graph.chars = \"a\\nb\"", "must be 2 or more characters"),
            ("graph.chars = \"日本\"", "must be 2 or more characters"),
        ];
        for (text, expected) in cases {
            let error = parse(text).unwrap_err();
            assert!(error.contains(expected), "{text}: {error}");
        }
    }

    #[test]
    fn needs_history_only_for_a_graph_that_is_shown() {
        assert!(!Config::default().needs_history());
        assert!(
            parse("format.cost = \"{usd} {graph}\"")
                .unwrap()
                .needs_history()
        );
        let hidden = "segments = [\"model\"]\nformat.cost = \"{usd} {graph}\"";
        assert!(!parse(hidden).unwrap().needs_history());
        assert!(
            parse("format.context = \"[{eta}]\"")
                .unwrap()
                .needs_history()
        );
        // A rate limit's eta comes from its reset time, not from history.
        assert!(
            !parse("format.five_hour = \"[{eta}]\"")
                .unwrap()
                .needs_history()
        );
    }

    #[test]
    fn a_missing_file_reads_as_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(&dir.path().join("config.toml")), Ok(Config::default()));
        assert_eq!(
            read(&dir.path().join("no/such/dir/config.toml")),
            Ok(Config::default())
        );
    }

    #[test]
    fn errors_name_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "segments = [").unwrap();
        let error = read(&path).unwrap_err();
        assert!(
            error.starts_with(&format!("{}: ", path.display())),
            "{error}"
        );

        // A directory where the file should be is an error, not the defaults.
        let error = read(dir.path()).unwrap_err();
        assert!(
            error.starts_with(&format!("{}: ", dir.path().display())),
            "{error}"
        );
    }
}

//! Which segments the status line shows, in what order, and the style of each, read from
//! `config.toml` in the user's config directory. Every key is optional and falls back to the
//! default.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::dirs;
use crate::history;
use crate::segments::Kind;
use crate::style::Style;
use crate::template::Template;

/// Every key at its default, as the README documents them. The editor shows a key the file leaves
/// out with its text from here.
pub const DEFAULTS: &str = r##"# The segments to show, left to right. Leave one out to hide it.
segments = ["model", "directory", "branch", "context", "five_hour", "seven_day", "cost"]
# Or several rows, top to bottom, instead of segments:
# lines = [["model", "directory", "branch"], ["context", "five_hour", "seven_day", "cost"]]

# Segments that drop first when a row is too wide, in this order. The rest drop in the default
# order: cost, seven_day, five_hour, branch, directory, model, and context last.
drop_order = []
# The same number as the padding in Claude Code's statusLine settings, which takes that many
# columns at each end of the line, so a row is fitted to the room that's left.
padding = 0

[colors]
model = "cyan"
directory = "blue"
branch = "magenta"
# usage takes the usage color below, by how much is used.
context = "usage"
five_hour = "usage"
seven_day = "usage"
cost = "dim"
separator = "dim"
# Context and rate-limit percentages: low, medium, and high, as [thresholds] sets them.
usage_low = "green"
usage_medium = "yellow"
usage_high = "red"

# Where each percentage turns from low to medium, and from medium to high.
[thresholds]
context = [50, 80]
five_hour = [50, 80]
seven_day = [50, 80]

# Show a segment only from this value up: a percent for context, five_hour, and seven_day, and
# dollars for cost, such as seven_day = 50 or cost = 1.0. A segment not named here always shows.
[show_from]

[format]
# Or a list, one for each row, such as [" │ ", " · "]. Rows past its end take the last.
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
marker = "┃"

[graph]
width = 8
chars = "▁▂▃▄▅▆▇█"
"##;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// One row of segments, left to right.
    pub segments: Option<Vec<Kind>>,
    /// Several rows, each a list of segments, top to bottom. Set this or `segments`, not both.
    pub lines: Option<Vec<Vec<Kind>>>,
    /// Segments that drop first on a narrow terminal, in this order, ahead of the rest.
    pub drop_order: Vec<Kind>,
    /// The `padding` in Claude Code's status line settings, which takes that many columns at each
    /// end of the line. The command can't see it, so the config repeats it.
    #[serde(deserialize_with = "padding")]
    pub padding: u16,
    pub colors: Colors,
    pub thresholds: Thresholds,
    pub show_from: ShowFrom,
    pub format: Format,
    pub bar: Bar,
    pub graph: Graph,
}

/// The value from which a segment shows: a percent for the context and rate limits, dollars for
/// the cost. A segment without one always shows.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShowFrom {
    #[serde(deserialize_with = "show_from")]
    pub context: Option<f64>,
    #[serde(deserialize_with = "show_from")]
    pub five_hour: Option<f64>,
    #[serde(deserialize_with = "show_from")]
    pub seven_day: Option<f64>,
    #[serde(deserialize_with = "show_from")]
    pub cost: Option<f64>,
}

impl ShowFrom {
    /// Whether `kind` shows at `value`.
    pub fn shows(&self, kind: Kind, value: f64) -> bool {
        let from = match kind {
            Kind::Context => self.context,
            Kind::FiveHour => self.five_hour,
            Kind::SevenDay => self.seven_day,
            Kind::Cost => self.cost,
            Kind::Model | Kind::Directory | Kind::Branch => None,
        };
        from.is_none_or(|from| value >= from)
    }
}

fn show_from<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    check_show_from(f64::deserialize(deserializer)?)
        .map(Some)
        .map_err(de::Error::custom)
}

/// Checks a `[show_from]` value.
pub fn check_show_from(value: f64) -> Result<f64, String> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(format!("show_from {value} isn't a number from 0 up"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    #[serde(deserialize_with = "plain_style")]
    pub model: Style,
    #[serde(deserialize_with = "plain_style")]
    pub directory: Style,
    #[serde(deserialize_with = "plain_style")]
    pub branch: Style,
    pub context: Style,
    pub five_hour: Style,
    pub seven_day: Style,
    #[serde(deserialize_with = "plain_style")]
    pub cost: Style,
    #[serde(deserialize_with = "plain_style")]
    pub separator: Style,
    /// Context and rate-limit percentages below their medium threshold.
    #[serde(deserialize_with = "plain_style")]
    pub usage_low: Style,
    /// Context and rate-limit percentages from their medium threshold up to their high one.
    #[serde(deserialize_with = "plain_style")]
    pub usage_medium: Style,
    /// Context and rate-limit percentages from their high threshold up.
    #[serde(deserialize_with = "plain_style")]
    pub usage_high: Style,
}

impl Colors {
    /// The style of segment `kind`.
    pub fn get(&self, kind: Kind) -> &Style {
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

fn plain_style<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Style, D::Error> {
    let style = Style::deserialize(deserializer)?;
    check_plain_style(&style).map_err(de::Error::custom)?;
    Ok(style)
}

/// Checks a style drawn without a percentage, which has no usage color to take.
pub fn check_plain_style(style: &Style) -> Result<(), String> {
    if style.uses_usage() {
        return Err(
            "usage is only for the context, five_hour, and seven_day styles and templates"
                .to_string(),
        );
    }
    Ok(())
}

/// Where each percentage segment turns from its low to its medium usage color, and from medium to
/// high, as whole percents.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Thresholds {
    #[serde(deserialize_with = "threshold_pair")]
    pub context: [u8; 2],
    #[serde(deserialize_with = "threshold_pair")]
    pub five_hour: [u8; 2],
    #[serde(deserialize_with = "threshold_pair")]
    pub seven_day: [u8; 2],
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            context: [50, 80],
            five_hour: [50, 80],
            seven_day: [50, 80],
        }
    }
}

impl Thresholds {
    /// The thresholds for `kind`, or `None` for a segment without a percentage.
    pub fn get(&self, kind: Kind) -> Option<[u8; 2]> {
        match kind {
            Kind::Context => Some(self.context),
            Kind::FiveHour => Some(self.five_hour),
            Kind::SevenDay => Some(self.seven_day),
            Kind::Model | Kind::Directory | Kind::Branch | Kind::Cost => None,
        }
    }
}

fn threshold_pair<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 2], D::Error> {
    check_thresholds(&Vec::<i64>::deserialize(deserializer)?).map_err(de::Error::custom)
}

/// Checks a pair of thresholds: two whole percents, the first no larger than the second.
pub fn check_thresholds(pair: &[i64]) -> Result<[u8; 2], String> {
    let percent = |value: i64| u8::try_from(value).ok().filter(|value| *value <= 100);
    match *pair {
        [medium, high] => match (percent(medium), percent(high)) {
            (Some(medium), Some(high)) if medium <= high => Ok([medium, high]),
            _ => Err(format!(
                "thresholds [{medium}, {high}] must be from 0 to 100, the first no larger than \
                 the second"
            )),
        },
        _ => Err(format!(
            "thresholds need two numbers, such as [50, 80], not {}",
            pair.len()
        )),
    }
}

/// The text of each part of the line. Each segment's template is checked against the
/// placeholders that segment has, so a mistake is reported with its line and column.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Format {
    /// Drawn between segments, in `colors.separator`.
    pub separator: Separators,
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

/// The separator of each row, written as one text for every row or a list with one per row.
/// Rows past the end of the list take its last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Separators(Vec<String>);

impl<'de> Deserialize<'de> for Separators {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Separators;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("text, or a list of text with one for each row")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<Separators, E> {
                Ok(Separators(vec![text.to_string()]))
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Separators, A::Error> {
                let mut list = Vec::new();
                while let Some(text) = seq.next_element::<String>()? {
                    list.push(text);
                }
                if list.is_empty() {
                    return Err(de::Error::custom(
                        "format.separator is an empty list; give at least one",
                    ));
                }
                Ok(Separators(list))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl Separators {
    /// The separator of row `index`, counted from 0.
    pub fn row(&self, index: usize) -> &str {
        self.0
            .get(index)
            .or(self.0.last())
            .expect("there is at least one")
    }
}

fn padding<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    check_padding(u64::deserialize(deserializer)?).map_err(de::Error::custom)
}

/// Checks `padding`.
pub fn check_padding(padding: u64) -> Result<u16, String> {
    u16::try_from(padding)
        .ok()
        .filter(|padding| *padding <= 100)
        .ok_or_else(|| format!("padding {padding} isn't from 0 to 100"))
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
    /// On a rate-limit bar, the cell for where usage would be by now at an even pace to 100% at
    /// the reset, one column wide.
    #[serde(deserialize_with = "cell")]
    pub marker: String,
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

    /// Where `kind` stands when a row is too wide: lower drops first. The segments in `drop_order`
    /// come first, in its order, and the rest follow in their default order.
    pub fn priority(&self, kind: Kind) -> u8 {
        match self.drop_order.iter().position(|&listed| listed == kind) {
            // Below every default priority, which start at 40.
            Some(index) => index as u8,
            None => kind.default_priority(),
        }
    }

    /// Every segment in the order they drop on a narrow terminal, first to last.
    pub fn drop_sequence(&self) -> Vec<Kind> {
        let mut kinds = Kind::ALL.to_vec();
        kinds.sort_by_key(|&kind| self.priority(kind));
        kinds
    }

    /// Whether any row shows `kind`.
    pub fn shows(&self, kind: Kind) -> bool {
        self.rows()
            .into_iter()
            .flatten()
            .any(|&shown| shown == kind)
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
    check_graph_width(u64::deserialize(deserializer)?).map_err(de::Error::custom)
}

/// Checks `[graph] width`.
pub fn check_graph_width(width: u64) -> Result<u8, String> {
    u8::try_from(width)
        .ok()
        .filter(|width| (1..=history::MAX_SAMPLES as u8).contains(width))
        .ok_or_else(|| {
            format!(
                "graph width {width} isn't from 1 to {}",
                history::MAX_SAMPLES
            )
        })
}

fn graph_chars<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<char>, D::Error> {
    check_graph_chars(&String::deserialize(deserializer)?).map_err(de::Error::custom)
}

/// Checks `[graph] chars`.
pub fn check_graph_chars(text: &str) -> Result<Vec<char>, String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 || chars.iter().any(|&c| c.width() != Some(1)) {
        return Err(format!(
            "graph chars {text:?} must be 2 or more characters, each one column wide, such as \
             \"▁▂▃▄▅▆▇█\""
        ));
    }
    Ok(chars)
}

impl Default for Bar {
    fn default() -> Self {
        Bar {
            width: 10,
            full: "█".to_string(),
            empty: "░".to_string(),
            marker: "┃".to_string(),
        }
    }
}

fn bar_width<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    check_bar_width(u64::deserialize(deserializer)?).map_err(de::Error::custom)
}

/// Checks `[bar] width`.
pub fn check_bar_width(width: u64) -> Result<u8, String> {
    u8::try_from(width)
        .ok()
        .filter(|width| (1..=100).contains(width))
        .ok_or_else(|| format!("bar width {width} isn't from 1 to 100"))
}

fn cell<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    check_cell(&text).map_err(de::Error::custom)?;
    Ok(text)
}

/// Checks one cell of a bar: text exactly one column wide, so every cell takes the same room and
/// the line doesn't shift as a value changes.
pub fn check_cell(text: &str) -> Result<(), String> {
    if text.width() != 1 || text.chars().any(char::is_control) {
        return Err(format!(
            "{text:?} isn't one column wide; a cell is a single character such as \"█\" or \"#\""
        ));
    }
    Ok(())
}

impl Default for Format {
    fn default() -> Self {
        let template =
            |kind: Kind, text: &str| kind.parse_template(text).expect("default templates parse");
        Format {
            separator: Separators(vec![" │ ".to_string()]),
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
    kind.parse_template(&text).map_err(de::Error::custom)
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
            context: style("usage"),
            five_hour: style("usage"),
            seven_day: style("usage"),
            cost: style("dim"),
            separator: style("dim"),
            usage_low: style("green"),
            usage_medium: style("yellow"),
            usage_high: style("red"),
        }
    }
}

/// Where the config file is, or `None` without a usable home or config directory.
pub fn path() -> Option<PathBuf> {
    dirs::config().map(|dir| dir.join("config.toml"))
}

/// Reads the config file, or the defaults when there is none. An error names the file and what
/// is wrong with it.
pub fn load() -> Result<Config, String> {
    path().map_or_else(|| Ok(Config::default()), |path| read(&path))
}

fn read(path: &Path) -> Result<Config, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Checks `drop_order`: each segment at most once.
pub fn check_drop_order(kinds: &[Kind]) -> Result<(), String> {
    for (i, kind) in kinds.iter().enumerate() {
        if kinds[..i].contains(kind) {
            return Err(format!("drop_order names {} twice", kind.name()));
        }
    }
    Ok(())
}

/// Parses the text of a config file.
pub fn parse(text: &str) -> Result<Config, String> {
    // A parse error ends in a newline.
    let config: Config =
        toml::from_str(text).map_err(|error| error.to_string().trim_end().to_string())?;
    if config.segments.is_some() && config.lines.is_some() {
        return Err("set segments or lines, not both".to_string());
    }
    check_drop_order(&config.drop_order)?;
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
        assert!(
            include_str!("../README.md").contains(&format!("```toml\n{DEFAULTS}```")),
            "the README's defaults differ from DEFAULTS"
        );
        let documented = parse(DEFAULTS).unwrap();
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
    fn drops_the_listed_segments_first() {
        use Kind::*;
        let default = Config::default();
        assert_eq!(
            default.drop_sequence(),
            [Cost, SevenDay, FiveHour, Branch, Directory, Model, Context]
        );
        let config = parse("drop_order = [\"context\", \"branch\"]").unwrap();
        assert_eq!(
            config.drop_sequence(),
            [Context, Branch, Cost, SevenDay, FiveHour, Directory, Model]
        );
        assert!(config.priority(Context) < config.priority(Branch));
        assert!(config.priority(Branch) < config.priority(Cost));
        assert_eq!(config.priority(Model), Model.default_priority());
        let every: Vec<String> = Kind::ALL
            .iter()
            .rev()
            .map(|k| format!("{:?}", k.name()))
            .collect();
        let config = parse(&format!("drop_order = [{}]", every.join(", "))).unwrap();
        assert_eq!(
            config.drop_sequence(),
            Kind::ALL.iter().rev().copied().collect::<Vec<_>>()
        );
    }

    #[test]
    fn shows_a_segment_from_its_value_up() {
        let show_from = parse("show_from.seven_day = 50\nshow_from.cost = 1.5")
            .unwrap()
            .show_from;
        assert!(!show_from.shows(Kind::SevenDay, 49.9));
        assert!(show_from.shows(Kind::SevenDay, 50.0));
        assert!(!show_from.shows(Kind::Cost, 1.49));
        assert!(show_from.shows(Kind::Cost, 1.5));
        assert!(show_from.shows(Kind::Context, 0.0));
        assert!(show_from.shows(Kind::Model, 0.0));
    }

    #[test]
    fn reads_thresholds_per_segment() {
        let config = parse("thresholds.context = [60, 85]\nthresholds.seven_day = [0, 0]").unwrap();
        assert_eq!(config.thresholds.get(Kind::Context), Some([60, 85]));
        assert_eq!(config.thresholds.get(Kind::FiveHour), Some([50, 80]));
        assert_eq!(config.thresholds.get(Kind::SevenDay), Some([0, 0]));
        assert_eq!(config.thresholds.get(Kind::Cost), None);
        assert_eq!(check_thresholds(&[100, 100]), Ok([100, 100]));
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
            parse("format.separator = \"\"")
                .unwrap()
                .format
                .separator
                .row(0),
            ""
        );
    }

    #[test]
    fn reads_one_separator_or_one_per_row() {
        let separators = |text: &str| parse(text).unwrap().format.separator;
        let one = separators("format.separator = \" | \"");
        assert_eq!((one.row(0), one.row(5)), (" | ", " | "));
        let each = separators("format.separator = [\" | \", \" · \"]");
        assert_eq!(
            (each.row(0), each.row(1), each.row(2)),
            (" | ", " · ", " · ")
        );
        assert_eq!(Config::default().format.separator.row(3), " │ ");
    }

    #[test]
    fn reads_the_padding() {
        assert_eq!(Config::default().padding, 0);
        assert_eq!(parse("padding = 3").unwrap().padding, 3);
        assert_eq!(check_padding(100), Ok(100));
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
            ("bar.marker = \"==\"", "isn't one column wide"),
            ("graph.width = 0", "graph width 0 isn't from 1 to 64"),
            ("graph.width = 65", "graph width 65 isn't from 1 to 64"),
            ("graph.chars = \"#\"", "must be 2 or more characters"),
            ("graph.chars = \"a\\nb\"", "must be 2 or more characters"),
            ("graph.chars = \"日本\"", "must be 2 or more characters"),
            ("format.separator = []", "format.separator is an empty list"),
            (
                "format.separator = 1",
                "expected text, or a list of text with one for each row",
            ),
            (
                "format.separator = [1]",
                "invalid type: integer `1`, expected a string",
            ),
            ("padding = 101", "padding 101 isn't from 0 to 100"),
            ("padding = -1", "invalid value"),
            ("padding = \"2\"", "invalid type"),
            (
                "drop_order = [\"cost\", \"cost\"]",
                "drop_order names cost twice",
            ),
            ("drop_order = [\"git\"]", "unknown variant `git`"),
            ("drop_order = \"cost\"", "invalid type"),
            (
                "colors.model = \"usage\"",
                "usage is only for the context, five_hour",
            ),
            (
                "colors.usage_low = \"bold usage\"",
                "usage is only for the context",
            ),
            (
                "colors.separator = \"usage\"",
                "usage is only for the context",
            ),
            (
                "format.cost = \"{usd:usage}\"",
                "usage is only for the context and rate-limit",
            ),
            ("format.cost = \"{usd:purple}\"", "unknown style \"purple\""),
            ("show_from.model = 1", "unknown field `model`"),
            (
                "show_from.cost = -1",
                "show_from -1 isn't a number from 0 up",
            ),
            (
                "show_from.cost = nan",
                "show_from NaN isn't a number from 0 up",
            ),
            ("show_from.cost = \"1\"", "invalid type"),
            ("thresholds.model = [50, 80]", "unknown field `model`"),
            (
                "thresholds.context = [90, 80]",
                "thresholds [90, 80] must be from 0",
            ),
            (
                "thresholds.context = [50, 101]",
                "thresholds [50, 101] must be from 0",
            ),
            (
                "thresholds.context = [-1, 80]",
                "thresholds [-1, 80] must be from 0",
            ),
            ("thresholds.context = [50]", "thresholds need two numbers"),
            (
                "thresholds.context = [50, 60, 70]",
                "thresholds need two numbers",
            ),
            ("thresholds.context = [50.5, 80]", "invalid type"),
            ("thresholds.context = 50", "invalid type"),
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

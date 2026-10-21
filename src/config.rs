//! Which segments the status line shows, in what order, and the style of each, read from
//! `config.toml` in the user's config directory. Every key is optional and falls back to the
//! default.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer};

use crate::segments::Kind;
use crate::style::Style;
use crate::template::Template;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The segments to show, left to right.
    pub segments: Vec<Kind>,
    pub colors: Colors,
    pub format: Format,
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

impl Default for Config {
    fn default() -> Self {
        Config {
            segments: Kind::ALL.to_vec(),
            colors: Colors::default(),
            format: Format::default(),
        }
    }
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
    path_from(env::var_os("XDG_CONFIG_HOME"), env::home_dir())
        .map_or_else(|| Ok(Config::default()), |path| read(&path))
}

/// `$XDG_CONFIG_HOME/astro-statusline/config.toml`, or `~/.config/astro-statusline/config.toml`
/// when `XDG_CONFIG_HOME` is unset.
fn path_from(xdg_config_home: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    // The XDG spec says to ignore a relative path, which would otherwise resolve against
    // whatever directory Claude Code runs the command in.
    let base = xdg_config_home
        .map(PathBuf::from)
        .filter(|base| base.is_absolute())
        .or_else(|| {
            home.filter(|home| home.is_absolute())
                .map(|home| home.join(".config"))
        })?;
    Some(base.join("astro-statusline").join("config.toml"))
}

fn read(path: &Path) -> Result<Config, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    toml::from_str(&text).map_err(|error| {
        // A parse error ends in a newline.
        format!("{}: {}", path.display(), error.to_string().trim_end())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }

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
        "##;
        assert_eq!(parse(documented).unwrap(), Config::default());
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
        assert_eq!(config.segments, [Kind::Context, Kind::Model]);
        assert_eq!(config.colors.model, style("bold #ff8700"));
        assert_eq!(config.colors.branch, style("magenta"));
        assert_eq!(parse("segments = []").unwrap().segments, []);
        assert_eq!(
            parse("format.separator = \"\"").unwrap().format.separator,
            ""
        );
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
        ];
        for (text, expected) in cases {
            let error = parse(text).unwrap_err();
            assert!(error.contains(expected), "{text}: {error}");
        }
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

    #[test]
    fn finds_the_file_under_xdg_config_home_or_home() {
        let path = |xdg: Option<&str>, home: Option<&str>| {
            path_from(xdg.map(OsString::from), home.map(PathBuf::from))
        };
        let expected = |base: &str| Some(Path::new(base).join("astro-statusline/config.toml"));

        assert_eq!(path(Some("/xdg"), Some("/home/me")), expected("/xdg"));
        assert_eq!(path(None, Some("/home/me")), expected("/home/me/.config"));
        assert_eq!(
            path(Some(""), Some("/home/me")),
            expected("/home/me/.config")
        );
        assert_eq!(
            path(Some("relative"), Some("/home/me")),
            expected("/home/me/.config")
        );
        assert_eq!(path(None, Some("")), None);
        assert_eq!(path(None, None), None);
    }
}

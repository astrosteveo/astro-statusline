//! How a piece of the line is drawn, written the way the config file spells it: words separated
//! by spaces, each an attribute (`bold`, `dim`, `italic`, `underline`) or a foreground color (a
//! name such as `cyan` or `bright_red`, a 256-color index from 0 to 255, or `#rrggbb`). An empty
//! string draws plain text.

use std::str::FromStr;

use serde::Deserialize;

const ATTRIBUTES: [(&str, u8); 4] = [("bold", 1), ("dim", 2), ("italic", 3), ("underline", 4)];

const COLORS: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// The parameters of an ANSI SGR escape, such as `1;36` for bold cyan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Style(Vec<u8>);

impl Style {
    /// The escape that turns this style on, or `None` for plain text.
    pub fn escape(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        let params: Vec<String> = self.0.iter().map(u8::to_string).collect();
        Some(format!("\x1b[{}m", params.join(";")))
    }
}

impl FromStr for Style {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut attributes = Vec::new();
        let mut color: Option<Vec<u8>> = None;
        for word in text.split_whitespace() {
            let lower = word.to_ascii_lowercase();
            if let Some(&(_, code)) = ATTRIBUTES.iter().find(|(name, _)| *name == lower) {
                attributes.push(code);
                continue;
            }
            let parsed = parse_color(&lower).ok_or_else(|| {
                format!(
                    "unknown style {word:?}: expected bold, dim, italic, underline, a color name \
                     such as cyan or bright_red, a number from 0 to 255, or #rrggbb"
                )
            })?;
            if color.replace(parsed).is_some() {
                return Err(format!("style {text:?} has more than one color"));
            }
        }
        attributes.extend(color.unwrap_or_default());
        Ok(Style(attributes))
    }
}

impl TryFrom<String> for Style {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

/// The SGR parameters of one foreground color.
fn parse_color(word: &str) -> Option<Vec<u8>> {
    let (base, name) = match word.strip_prefix("bright_") {
        Some(name) => (90, name),
        None => (30, word),
    };
    if let Some(index) = COLORS.iter().position(|color| *color == name) {
        return Some(vec![base + index as u8]);
    }
    if let Some(hex) = word.strip_prefix('#') {
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        return Some(vec![38, 2, channel(0)?, channel(2)?, channel(4)?]);
    }
    // Digits only, so "+5" isn't read as 5.
    if !word.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let index: u8 = word.parse().ok()?;
    Some(vec![38, 5, index])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escape(text: &str) -> Option<String> {
        text.parse::<Style>().unwrap().escape()
    }

    #[test]
    fn parses_named_colors() {
        assert_eq!(escape("black").as_deref(), Some("\x1b[30m"));
        assert_eq!(escape("cyan").as_deref(), Some("\x1b[36m"));
        assert_eq!(escape("white").as_deref(), Some("\x1b[37m"));
        assert_eq!(escape("bright_black").as_deref(), Some("\x1b[90m"));
        assert_eq!(escape("bright_white").as_deref(), Some("\x1b[97m"));
        assert_eq!(escape("Red").as_deref(), Some("\x1b[31m"));
    }

    #[test]
    fn parses_indexed_and_rgb_colors() {
        assert_eq!(escape("0").as_deref(), Some("\x1b[38;5;0m"));
        assert_eq!(escape("208").as_deref(), Some("\x1b[38;5;208m"));
        assert_eq!(escape("#ff8700").as_deref(), Some("\x1b[38;2;255;135;0m"));
        assert_eq!(escape("#FF8700").as_deref(), Some("\x1b[38;2;255;135;0m"));
    }

    #[test]
    fn puts_attributes_before_the_color() {
        assert_eq!(escape("dim").as_deref(), Some("\x1b[2m"));
        assert_eq!(escape("cyan bold").as_deref(), Some("\x1b[1;36m"));
        assert_eq!(
            escape("  underline   italic #000000 ").as_deref(),
            Some("\x1b[4;3;38;2;0;0;0m")
        );
    }

    #[test]
    fn an_empty_style_is_plain() {
        assert_eq!(escape(""), None);
        assert_eq!(escape("   "), None);
    }

    #[test]
    fn rejects_what_it_cannot_draw() {
        for text in [
            "purple",
            "bright_",
            "bright_dim",
            "bright_208",
            "256",
            "-1",
            "+5",
            "#fff",
            "#ff87000",
            "#gg8700",
            "#+f8700",
            "red,bold",
        ] {
            assert!(text.parse::<Style>().is_err(), "{text:?}");
        }
    }

    #[test]
    fn rejects_two_colors() {
        let error = "red blue".parse::<Style>().unwrap_err();
        assert_eq!(error, r#"style "red blue" has more than one color"#);
    }
}

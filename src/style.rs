//! How a piece of the line is drawn, written the way the config file spells it: words separated
//! by spaces, each an attribute (`bold`, `dim`, `italic`, `underline`) or a foreground color (a
//! name such as `cyan` or `bright_red`, a 256-color index from 0 to 255, `#rrggbb`, or `usage`
//! for the usage color of the value drawn). An empty string draws plain text.

use std::str::FromStr;

use serde::Deserialize;

pub const ATTRIBUTES: [(&str, u8); 4] = [("bold", 1), ("dim", 2), ("italic", 3), ("underline", 4)];

pub const COLORS: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// A terminal color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// One of the 16 colors of the terminal's theme: the 8 in `COLORS`, then their bright versions.
    Basic(u8),
    /// A color of the 256-color palette, by number.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    /// The SGR parameters that set this color as the foreground.
    fn foreground(self) -> Vec<u8> {
        match self {
            Color::Basic(index @ 0..8) => vec![30 + index],
            Color::Basic(index) => vec![90 + index - 8],
            Color::Indexed(index) => vec![38, 5, index],
            Color::Rgb(r, g, b) => vec![38, 2, r, g, b],
        }
    }
}

/// The color text is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Color(Color),
    /// The usage style of the percentage drawn, by how much is used.
    Usage,
}

/// Attributes and at most one color.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Style {
    /// SGR parameters, such as 1 for bold, in the order written.
    attributes: Vec<u8>,
    ink: Option<Ink>,
}

impl Style {
    /// The escape that turns this style on, or `None` for plain text. A style is resolved before
    /// it's drawn, so `usage` has become a color by then.
    pub fn escape(&self) -> Option<String> {
        let mut params = self.attributes.clone();
        if let Some(Ink::Color(color)) = self.ink {
            params.extend(color.foreground());
        }
        if params.is_empty() {
            return None;
        }
        let params: Vec<String> = params.iter().map(u8::to_string).collect();
        Some(format!("\x1b[{}m", params.join(";")))
    }

    /// Whether the style takes the usage color.
    pub fn uses_usage(&self) -> bool {
        self.ink == Some(Ink::Usage)
    }

    /// The style with `usage` replaced by the attributes and color of `usage`.
    pub fn resolve(&self, usage: &Style) -> Style {
        if !self.uses_usage() {
            return self.clone();
        }
        Style {
            attributes: [&self.attributes[..], &usage.attributes[..]].concat(),
            ink: usage.ink,
        }
    }

    /// The attributes that are on, by their place in `ATTRIBUTES`.
    pub fn attribute_flags(&self) -> [bool; 4] {
        ATTRIBUTES.map(|(_, code)| self.attributes.contains(&code))
    }

    pub fn ink(&self) -> Option<Ink> {
        self.ink
    }
}

impl FromStr for Style {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut style = Style::default();
        for word in text.split_whitespace() {
            let lower = word.to_ascii_lowercase();
            if let Some(&(_, code)) = ATTRIBUTES.iter().find(|(name, _)| *name == lower) {
                style.attributes.push(code);
                continue;
            }
            let ink = match lower.as_str() {
                "usage" => Ink::Usage,
                _ => Ink::Color(parse_color(&lower).ok_or_else(|| {
                    format!(
                        "unknown style {word:?}: expected bold, dim, italic, underline, a color \
                         name such as cyan or bright_red, a number from 0 to 255, #rrggbb, or \
                         usage"
                    )
                })?),
            };
            if style.ink.replace(ink).is_some() {
                return Err(format!("style {text:?} has more than one color"));
            }
        }
        Ok(style)
    }
}

impl TryFrom<String> for Style {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

/// One color, as the config file writes it.
fn parse_color(word: &str) -> Option<Color> {
    let (offset, name) = match word.strip_prefix("bright_") {
        Some(name) => (8, name),
        None => (0, word),
    };
    if let Some(index) = COLORS.iter().position(|color| *color == name) {
        return Some(Color::Basic(offset + index as u8));
    }
    if let Some(hex) = word.strip_prefix('#') {
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        return Some(Color::Rgb(channel(0)?, channel(2)?, channel(4)?));
    }
    // Digits only, so "+5" isn't read as 5.
    if !word.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    word.parse().ok().map(Color::Indexed)
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
        let error = "usage red".parse::<Style>().unwrap_err();
        assert_eq!(error, r#"style "usage red" has more than one color"#);
    }

    #[test]
    fn takes_the_usage_style_where_it_says_usage() {
        let style = |text: &str| text.parse::<Style>().unwrap();
        let usage = style("bold red");
        assert!(style("Usage").uses_usage());
        assert_eq!(
            style("italic usage").resolve(&usage),
            style("italic bold red")
        );
        assert_eq!(style("usage").resolve(&style("")), style(""));
        assert_eq!(style("dim cyan").resolve(&usage), style("dim cyan"));
        assert_eq!(
            style("italic usage").resolve(&usage).escape().as_deref(),
            Some("\x1b[3;1;31m")
        );
        assert_eq!(
            style("bold underline").attribute_flags(),
            [true, false, false, true]
        );
    }
}

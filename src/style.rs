//! How a piece of the line is drawn, written the way the config file spells it: words separated
//! by spaces, each an attribute (`bold`, `dim`, `italic`, `underline`), a foreground color (a
//! name such as `cyan` or `bright_red`, a 256-color index from 0 to 255, `#rrggbb`, or `usage`
//! for the usage color of the value drawn), or `on` and a background color. An empty string draws
//! plain text.

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

    /// The SGR parameters that set this color as the background.
    fn background(self) -> Vec<u8> {
        match self {
            Color::Basic(index @ 0..8) => vec![40 + index],
            Color::Basic(index) => vec![100 + index - 8],
            Color::Indexed(index) => vec![48, 5, index],
            Color::Rgb(r, g, b) => vec![48, 2, r, g, b],
        }
    }

    /// The color as the config file writes it.
    pub fn name(self) -> String {
        match self {
            Color::Basic(index @ 0..8) => COLORS[usize::from(index)].to_string(),
            Color::Basic(index) => format!("bright_{}", COLORS[usize::from(index - 8)]),
            Color::Indexed(index) => index.to_string(),
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// A color as a style names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Color(Color),
    /// The usage style of the percentage drawn, by how much is used.
    Usage,
}

/// Attributes, at most one color, and at most one background.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Style {
    /// SGR parameters, such as 1 for bold, in the order written.
    attributes: Vec<u8>,
    ink: Option<Ink>,
    background: Option<Ink>,
}

impl Style {
    /// Plain text in `color` on `background`.
    pub fn colors(color: Option<Color>, background: Option<Color>) -> Style {
        Style {
            attributes: Vec::new(),
            ink: color.map(Ink::Color),
            background: background.map(Ink::Color),
        }
    }

    /// The escape that turns this style on, or `None` for plain text. A style is resolved before
    /// it's drawn, so `usage` has become a color by then.
    pub fn escape(&self) -> Option<String> {
        let mut params = self.attributes.clone();
        if let Some(Ink::Color(color)) = self.ink {
            params.extend(color.foreground());
        }
        if let Some(Ink::Color(color)) = self.background {
            params.extend(color.background());
        }
        if params.is_empty() {
            return None;
        }
        let params: Vec<String> = params.iter().map(u8::to_string).collect();
        Some(format!("\x1b[{}m", params.join(";")))
    }

    /// Whether the style takes the usage color, as its color or its background.
    pub fn uses_usage(&self) -> bool {
        self.ink == Some(Ink::Usage) || self.background == Some(Ink::Usage)
    }

    /// The style with `usage` replaced: as the color, by the attributes and color of `usage`, and
    /// as the background, by the color of `usage`.
    pub fn resolve(&self, usage: &Style) -> Style {
        let mut style = self.clone();
        if self.ink == Some(Ink::Usage) {
            style.attributes.extend(&usage.attributes);
            style.ink = usage.ink;
        }
        if self.background == Some(Ink::Usage) {
            style.background = usage.ink;
        }
        style
    }

    /// The style with the background of `other` when it has none of its own.
    pub fn over(mut self, other: &Style) -> Style {
        self.background = self.background.or(other.background);
        self
    }

    /// The attributes that are on, by their place in `ATTRIBUTES`.
    pub fn attribute_flags(&self) -> [bool; 4] {
        ATTRIBUTES.map(|(_, code)| self.attributes.contains(&code))
    }

    pub fn ink(&self) -> Option<Ink> {
        self.ink
    }

    pub fn background(&self) -> Option<Ink> {
        self.background
    }

    /// The background's color, once resolved.
    pub fn background_color(&self) -> Option<Color> {
        match self.background {
            Some(Ink::Color(color)) => Some(color),
            _ => None,
        }
    }
}

impl FromStr for Style {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut style = Style::default();
        let mut words = text.split_whitespace();
        while let Some(word) = words.next() {
            let lower = word.to_ascii_lowercase();
            if let Some(&(_, code)) = ATTRIBUTES.iter().find(|(name, _)| *name == lower) {
                style.attributes.push(code);
                continue;
            }
            if lower == "on" {
                let word = words
                    .next()
                    .ok_or_else(|| format!("style {text:?} has `on` with no color after it"))?;
                if style.background.replace(parse_ink(word)?).is_some() {
                    return Err(format!("style {text:?} has more than one background"));
                }
                continue;
            }
            if style.ink.replace(parse_ink(word)?).is_some() {
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

/// A color word: a color, or `usage`.
fn parse_ink(word: &str) -> Result<Ink, String> {
    let lower = word.to_ascii_lowercase();
    if lower == "usage" {
        return Ok(Ink::Usage);
    }
    parse_color(&lower).map(Ink::Color).ok_or_else(|| {
        format!(
            "unknown style {word:?}: expected bold, dim, italic, underline, a color name such as \
             cyan or bright_red, a number from 0 to 255, #rrggbb, usage, or on and a color"
        )
    })
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
    fn draws_on_a_background() {
        assert_eq!(escape("on blue").as_deref(), Some("\x1b[44m"));
        assert_eq!(
            escape("bold white on bright_black").as_deref(),
            Some("\x1b[1;37;100m")
        );
        assert_eq!(escape("on 208 black").as_deref(), Some("\x1b[30;48;5;208m"));
        assert_eq!(escape("ON #005f87").as_deref(), Some("\x1b[48;2;0;95;135m"));
        let error = |text: &str| text.parse::<Style>().unwrap_err();
        assert_eq!(
            error("red on"),
            r#"style "red on" has `on` with no color after it"#
        );
        assert_eq!(
            error("on red on blue"),
            r#"style "on red on blue" has more than one background"#
        );
        assert!(error("on bold").contains("unknown style \"bold\""));
        // Over another style, a background is kept unless the style sets its own.
        let style = |text: &str| text.parse::<Style>().unwrap();
        assert_eq!(
            style("bold").over(&style("dim on blue")),
            style("bold on blue")
        );
        assert_eq!(style("on red").over(&style("on blue")), style("on red"));
        assert_eq!(style("on blue").background_color(), Some(Color::Basic(4)));
    }

    #[test]
    fn takes_the_usage_color_as_a_background() {
        let style = |text: &str| text.parse::<Style>().unwrap();
        let usage = style("bold red");
        assert_eq!(
            style("black on usage").resolve(&usage),
            style("black on red")
        );
        assert_eq!(
            style("usage on usage").resolve(&usage),
            style("bold red on red")
        );
        assert!(style("on usage").uses_usage());
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

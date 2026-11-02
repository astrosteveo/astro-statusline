//! Configs to start from. Applying one replaces the config being edited, which stays unsaved until
//! the user saves it.

use crate::config::DEFAULTS;

pub struct Preset {
    pub name: &'static str,
    pub about: &'static str,
    pub text: &'static str,
}

pub const PRESETS: [Preset; 4] = [
    Preset {
        name: "Default",
        about: "Every key at its default, with a comment on each.",
        text: DEFAULTS,
    },
    Preset {
        name: "Plain ASCII",
        about: "Only ASCII characters, for fonts without box drawing or block elements.",
        text: PLAIN_ASCII,
    },
    Preset {
        name: "Powerline",
        about: "Segments on backgrounds with arrows between. Needs a Nerd Font.",
        text: POWERLINE,
    },
    Preset {
        name: "Dashboard",
        about: "Two rows, with bars, graphs, and predictions.",
        text: DASHBOARD,
    },
];

const PLAIN_ASCII: &str = r##"# astro-statusline: the Plain ASCII preset. Only ASCII characters, for fonts without box
# drawing or block elements.
segments = ["model", "directory", "branch", "context", "five_hour", "seven_day", "cost"]

[format]
separator = " | "
context = "ctx {percent} {bar}"

[bar]
width = 8
full = "#"
empty = "-"
marker = "|"

[graph]
chars = "_.-=+*#"
"##;

const POWERLINE: &str = r##"# astro-statusline: the Powerline preset. Segments on backgrounds with arrows between them,
# which need a Nerd Font.
lines = [
  ["model", "directory", "fill", "branch"],
  ["context", "five_hour", "seven_day", "cost"],
]

[colors]
model = "black on cyan"
directory = "black on blue"
branch = "black on magenta"
context = "black on usage"
five_hour = "black on usage"
seven_day = "black on usage"
cost = "white on 238"
separator = "powerline"

[format]
# U+E0B0, the powerline arrow.
separator = "\ue0b0"
model = " {name} "
directory = " {name} "
branch = " \ue0a0 {branch} "
context = " ctx {percent:bold black} "
five_hour = " 5h {percent:bold black} "
seven_day = " 7d {percent:bold black} "
cost = " {usd} "
"##;

const DASHBOARD: &str = r##"# astro-statusline: the Dashboard preset. Two rows, with bars, graphs, and predictions.
lines = [
  ["model", "directory", "branch"],
  ["context", "five_hour", "seven_day", "cost"],
]

[format]
separator = " · "
context = "ctx {bar} {percent} {graph}[ full in {eta}]"
five_hour = "5h {bar} {percent}[ ({delta})][ ⚠ limit in {eta}][ resets {resets_in}]"
seven_day = "7d {bar} {percent}[ ({delta})][ → {projected}]"
cost = "{usd} {graph}[ ({rate}/h)]"

[bar]
width = 8
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::editor::doc::Doc;
    use crate::render::SeparatorStyle;

    #[test]
    fn every_preset_is_a_config_the_editor_can_edit() {
        for preset in &PRESETS {
            assert!(config::parse(preset.text).is_ok(), "{}", preset.name);
            assert_eq!(Doc::parse(preset.text).unwrap().text(), preset.text);
        }
    }

    #[test]
    fn each_preset_is_what_it_says() {
        // The default preset names every segment, which is what leaving them out means.
        let default = config::parse(PRESETS[0].text).unwrap();
        assert_eq!(default.rows(), config::Config::default().rows());
        assert_eq!(
            config::Config {
                segments: None,
                ..default
            },
            config::Config::default()
        );
        assert!(PLAIN_ASCII.is_ascii());
        let powerline = config::parse(POWERLINE).unwrap();
        assert_eq!(powerline.colors.separator, SeparatorStyle::Powerline);
        assert!(config::parse(DASHBOARD).unwrap().needs_history());
    }
}

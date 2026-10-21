//! Where the program keeps its files, following the XDG base directory spec.

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

/// `$XDG_CONFIG_HOME/astro-statusline`, or `~/.config/astro-statusline`.
pub fn config() -> Option<PathBuf> {
    app_dir(env::var_os("XDG_CONFIG_HOME"), env::home_dir(), ".config")
}

/// `$XDG_STATE_HOME/astro-statusline`, or `~/.local/state/astro-statusline`.
pub fn state() -> Option<PathBuf> {
    app_dir(
        env::var_os("XDG_STATE_HOME"),
        env::home_dir(),
        ".local/state",
    )
}

/// The program's directory under `base`, or under `home`'s `fallback` when `base` is unset.
fn app_dir(base: Option<OsString>, home: Option<PathBuf>, fallback: &str) -> Option<PathBuf> {
    // The XDG spec says to ignore a relative path, which would otherwise resolve against
    // whatever directory Claude Code runs the command in.
    let base = base
        .map(PathBuf::from)
        .filter(|base| base.is_absolute())
        .or_else(|| {
            home.filter(|home| home.is_absolute())
                .map(|home| home.join(fallback))
        })?;
    Some(base.join("astro-statusline"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn prefers_an_absolute_xdg_variable_then_home() {
        let dir = |base: Option<&str>, home: Option<&str>| {
            app_dir(base.map(OsString::from), home.map(PathBuf::from), ".config")
        };
        let expected = |base: &str| Some(Path::new(base).join("astro-statusline"));

        assert_eq!(dir(Some("/xdg"), Some("/home/me")), expected("/xdg"));
        assert_eq!(dir(None, Some("/home/me")), expected("/home/me/.config"));
        assert_eq!(
            dir(Some(""), Some("/home/me")),
            expected("/home/me/.config")
        );
        assert_eq!(
            dir(Some("relative"), Some("/home/me")),
            expected("/home/me/.config")
        );
        assert_eq!(dir(None, Some("")), None);
        assert_eq!(dir(None, None), None);
    }
}

mod config;
mod git;
mod render;
mod segments;
mod session;
mod style;
mod template;
mod units;

use std::env;
use std::io::{self, Read, Write};

use config::Config;
use session::Session;

/// Claude Code draws the status line two columns in from each edge (measured in fullscreen
/// rendering), and cuts off anything wider. A `padding` set in settings takes more.
const EDGE_RESERVE: usize = 4;

fn main() {
    let mut input = Vec::new();
    // A read error leaves whatever arrived; anything unparsable renders as an empty session,
    // never as an error.
    let _ = io::stdin().read_to_end(&mut input);
    let session = Session::parse(&input);

    let config = config::load().unwrap_or_else(|error| {
        // Claude Code doesn't show stderr, but running the command by hand does.
        let _ = writeln!(io::stderr().lock(), "astro-statusline: {error}");
        let _ = writeln!(
            io::stderr().lock(),
            "astro-statusline: using the default config"
        );
        Config::default()
    });

    let line = render::render(
        segments::all(&session, &config),
        &config.format.separator,
        &config.colors.separator,
        max_width(),
        color_enabled(),
    );

    // Claude Code shows only stdout, and only after a zero exit, so a failed write has nowhere
    // useful to go.
    let _ = writeln!(io::stdout().lock(), "{line}");
}

/// The widest the line may be. Claude Code sets `COLUMNS` to the terminal width, since the
/// command's output is captured and can't query the terminal itself.
fn max_width() -> Option<usize> {
    let columns: usize = env::var("COLUMNS").ok()?.trim().parse().ok()?;
    (columns > 0).then(|| columns.saturating_sub(EDGE_RESERVE))
}

/// Color is on unless `NO_COLOR` is set to a non-empty value (https://no-color.org).
fn color_enabled() -> bool {
    env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
}

mod git;
mod render;
mod segments;
mod session;

use std::env;
use std::io::{self, Read, Write};

use session::Session;

/// Claude Code indents the status line, plus any `padding` set in settings, so leave a few
/// columns spare to keep the line from wrapping.
const EDGE_RESERVE: usize = 4;

fn main() {
    let mut input = Vec::new();
    // A read error leaves whatever arrived; anything unparsable renders as an empty session,
    // never as an error.
    let _ = io::stdin().read_to_end(&mut input);
    let session = Session::parse(&input);

    let line = render::render(segments::all(&session), max_width(), color_enabled());

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

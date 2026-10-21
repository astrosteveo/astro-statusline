mod session;

use std::io::{self, Read, Write};

use session::Session;

fn main() {
    let mut input = Vec::new();
    // A read error leaves whatever arrived; anything unparsable renders as an empty session,
    // never as an error.
    let _ = io::stdin().read_to_end(&mut input);
    let session = Session::parse(&input);

    let model = session
        .model
        .as_ref()
        .and_then(|m| m.display_name.as_deref())
        .unwrap_or_default();
    let dir = session
        .current_dir()
        .and_then(|d| d.file_name())
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();

    // Claude Code shows only stdout, and only after a zero exit, so a failed write has nowhere
    // useful to go.
    let _ = writeln!(io::stdout().lock(), "{model} {dir}");
}

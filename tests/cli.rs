//! Runs the built binary the way Claude Code does: session JSON on stdin, `COLUMNS` in the
//! environment, and only a zero exit with output on stdout counting as a status line.

use std::io::Write;
use std::process::{Command, Stdio};

struct Output {
    stdout: String,
    stderr: String,
}

fn run(stdin: &[u8], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_astro-statusline"));
    command
        .env_remove("COLUMNS")
        .env_remove("NO_COLOR")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn the binary");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(stdin)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for the binary");
    assert!(output.status.success(), "exit status {}", output.status);
    Output {
        stdout: String::from_utf8(output.stdout).expect("utf-8 stdout"),
        stderr: String::from_utf8(output.stderr).expect("utf-8 stderr"),
    }
}

#[test]
fn renders_a_colored_line_from_the_session() {
    let out = run(
        br#"{"model": {"display_name": "Opus"}, "workspace": {"current_dir": "/home/me/proj"}}"#,
        &[],
    );
    assert_eq!(
        out.stdout,
        "\x1b[36mOpus\x1b[0m\x1b[2m │ \x1b[0m\x1b[34mproj\x1b[0m\n"
    );
    assert_eq!(out.stderr, "");
}

#[test]
fn honors_no_color_and_columns() {
    let input = br#"{"model": {"display_name": "Opus"}, "cwd": "/home/me/project"}"#;
    assert_eq!(run(input, &[("NO_COLOR", "1")]).stdout, "Opus │ project\n");
    // 14 columns less the edge reserve leaves room for "Opus" but not the directory.
    let out = run(input, &[("NO_COLOR", "1"), ("COLUMNS", "14")]);
    assert_eq!(out.stdout, "Opus\n");
}

#[test]
fn bad_input_exits_zero_without_errors() {
    for input in [&b""[..], b"not json", b"[]", b"{\"model\": null}", b"\xff"] {
        let out = run(input, &[("COLUMNS", "nonsense")]);
        assert_eq!(out.stdout, "\n");
        assert_eq!(out.stderr, "");
    }
}

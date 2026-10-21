//! Runs the built binary the way Claude Code does: session JSON on stdin, `COLUMNS` in the
//! environment, and only a zero exit with output on stdout counting as a status line.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct Output {
    stdout: String,
    stderr: String,
}

fn run(stdin: &[u8], env: &[(&str, &str)]) -> Output {
    run_with_config(None, stdin, env)
}

/// Runs with `config` as the config file, or with none, but never with the user's own. History
/// goes to a fresh state directory unless `env` names one in `XDG_STATE_HOME`.
fn run_with_config(config: Option<&str>, stdin: &[u8], env: &[(&str, &str)]) -> Output {
    let config_home = tempfile::tempdir().expect("create a config dir");
    let state_home = tempfile::tempdir().expect("create a state dir");
    if let Some(config) = config {
        let dir = config_home.path().join("astro-statusline");
        fs::create_dir(&dir).expect("create the config's dir");
        fs::write(dir.join("config.toml"), config).expect("write the config");
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_astro-statusline"));
    command
        .env_remove("COLUMNS")
        .env_remove("NO_COLOR")
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("XDG_STATE_HOME", state_home.path())
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

/// Every default segment, colored. Later changes to how segments are built must keep this line
/// byte for byte.
#[test]
fn renders_every_default_segment_byte_for_byte() {
    let root = tempfile::tempdir().expect("create a repo dir");
    let repo = root.path().join("proj");
    fs::create_dir_all(repo.join(".git")).expect("create .git");
    fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").expect("write HEAD");
    let input = format!(
        r#"{{
            "model": {{"display_name": "Opus"}},
            "workspace": {{"current_dir": "{}"}},
            "context_window": {{"used_percentage": 63}},
            "rate_limits": {{
                "five_hour": {{"used_percentage": 12}},
                "seven_day": {{"used_percentage": 85}}
            }},
            "cost": {{"total_cost_usd": 1.234}}
        }}"#,
        repo.display()
    );
    let out = run(input.as_bytes(), &[]);
    let separator = "\x1b[2m │ \x1b[0m";
    let expected = [
        "\x1b[36mOpus\x1b[0m",
        "\x1b[34mproj\x1b[0m",
        "\x1b[35mmain\x1b[0m",
        "\x1b[33mctx 63%\x1b[0m",
        "\x1b[32m5h 12%\x1b[0m",
        "\x1b[31m7d 85%\x1b[0m",
        "\x1b[2m$1.23\x1b[0m",
    ]
    .join(separator);
    assert_eq!(out.stdout, format!("{expected}\n"));
    assert_eq!(out.stderr, "");
}

/// Odd values render the way they always have: control characters stripped, percentages
/// rounded and clamped, and a negative cost shown as is.
#[test]
fn renders_odd_values_byte_for_byte() {
    let input = br#"{
        "model": {"display_name": "O\u0007pus"},
        "context_window": {"used_percentage": -0.4},
        "rate_limits": {"five_hour": {"used_percentage": 1e300}},
        "cost": {"total_cost_usd": -1}
    }"#;
    let out = run(input, &[("NO_COLOR", "1")]);
    assert_eq!(
        out.stdout,
        "Opus │ ctx 0% │ 5h 18446744073709551615% │ $-1.00\n"
    );
    // 44 columns less the edge reserve is exactly the line without cost, the lowest priority.
    let out = run(input, &[("NO_COLOR", "1"), ("COLUMNS", "44")]);
    assert_eq!(out.stdout, "Opus │ ctx 0% │ 5h 18446744073709551615%\n");
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

#[test]
fn reads_segments_and_colors_from_the_config_file() {
    let config = r#"
        segments = ["directory", "model"]

        [colors]
        model = "bold 208"
        separator = ""

        [format]
        separator = " · "
    "#;
    let out = run_with_config(
        Some(config),
        br#"{"model": {"display_name": "Opus"}, "workspace": {"current_dir": "/home/me/proj"}}"#,
        &[],
    );
    assert_eq!(
        out.stdout,
        "\x1b[34mproj\x1b[0m · \x1b[1;38;5;208mOpus\x1b[0m\n"
    );
    assert_eq!(out.stderr, "");
}

#[test]
fn an_invalid_config_falls_back_to_the_defaults() {
    let input = br#"{"model": {"display_name": "Opus"}, "cwd": "/home/me/proj"}"#;
    for config in [
        "segments = [\"git\"]",
        "colors.model = \"purple\"",
        "[colors",
        "format.model = \"{nope}\"",
    ] {
        let out = run_with_config(Some(config), input, &[("NO_COLOR", "1")]);
        assert_eq!(out.stdout, "Opus │ proj\n", "{config}");
        assert!(out.stderr.contains("config.toml: "), "{}", out.stderr);
        assert!(
            out.stderr
                .ends_with("astro-statusline: using the default config\n"),
            "{}",
            out.stderr
        );
    }
}

#[test]
fn draws_templates_and_bars_from_the_config_file() {
    let config = r##"
        segments = ["context", "cost"]

        [format]
        context = "ctx {bar} {percent}[ of {size}]"
        cost = "[[{usd}]]"

        [bar]
        width = 4
        full = "#"
        empty = "."
    "##;
    let input = br#"{"context_window": {"used_percentage": 63}, "cost": {"total_cost_usd": 1.5}}"#;
    let out = run_with_config(Some(config), input, &[("NO_COLOR", "1")]);
    assert_eq!(out.stdout, "ctx ##.. 63% │ [$1.50]\n");
    assert_eq!(out.stderr, "");
}

#[test]
fn predicts_rate_limits_from_the_clock() {
    let config = r#"
        segments = ["five_hour", "cost"]

        [format]
        five_hour = "5h {percent} resets {resets_in}[ ⚠ limit in {eta}]"
        cost = "{usd}[ ({rate}/h)]"
    "#;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // An hour and 30 seconds left of the 5-hour window, so the countdown reads 1h even if the
    // binary reads the clock a few seconds later. With 90% used, 100% comes in about 26 minutes.
    let input = format!(
        r#"{{
            "rate_limits": {{"five_hour": {{"used_percentage": 90, "resets_at": {}}}}},
            "cost": {{"total_cost_usd": 2, "total_duration_ms": 3600000}}
        }}"#,
        now + 3630
    );
    let out = run_with_config(Some(config), input.as_bytes(), &[("NO_COLOR", "1")]);
    assert_eq!(
        out.stdout,
        "5h 90% resets 1h ⚠ limit in 26m │ $2.00 ($2.00/h)\n"
    );
}

const GRAPH_CONFIG: &str = r#"
    segments = ["context"]
    format.context = "ctx {percent} {graph}"
"#;

fn context_update(session_id: &str, used: u32) -> String {
    format!(r#"{{"session_id": "{session_id}", "context_window": {{"used_percentage": {used}}}}}"#)
}

/// Every file under `dir`, relative to it.
fn files_under(dir: &std::path::Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path.strip_prefix(dir).unwrap().display().to_string());
            }
        }
    }
    files
}

#[test]
fn graphs_values_across_runs_of_one_session() {
    let state = tempfile::tempdir().unwrap();
    let env = [
        ("NO_COLOR", "1"),
        ("XDG_STATE_HOME", state.path().to_str().unwrap()),
    ];
    let draw = |used| {
        let out = run_with_config(
            Some(GRAPH_CONFIG),
            context_update("abc", used).as_bytes(),
            &env,
        );
        assert_eq!(out.stderr, "");
        out.stdout
    };
    assert_eq!(draw(10), "ctx 10% ▁\n");
    assert_eq!(draw(60), "ctx 60% ▁▅\n");
    let file = state.path().join("astro-statusline/sessions/abc.json");
    let before = fs::read(&file).unwrap();
    // A refresh with nothing new draws the same graph and leaves the file alone.
    assert_eq!(draw(60), "ctx 60% ▁▅\n");
    assert_eq!(fs::read(&file).unwrap(), before);
    assert_eq!(
        files_under(state.path()),
        ["astro-statusline/sessions/abc.json"]
    );
}

#[test]
fn keeps_no_history_unless_a_graph_is_shown() {
    let state = tempfile::tempdir().unwrap();
    let env = [("XDG_STATE_HOME", state.path().to_str().unwrap())];
    let input = context_update("abc", 10);
    run_with_config(None, input.as_bytes(), &env);
    // A graph in a segment that isn't shown doesn't count.
    let hidden = "segments = [\"model\"]\nformat.context = \"{graph}\"";
    run_with_config(Some(hidden), input.as_bytes(), &env);
    assert_eq!(files_under(state.path()), Vec::<String>::new());
}

#[test]
fn keeps_no_history_for_a_session_id_that_isnt_a_plain_name() {
    let state = tempfile::tempdir().unwrap();
    let env = [
        ("NO_COLOR", "1"),
        ("XDG_STATE_HOME", state.path().to_str().unwrap()),
    ];
    for id in ["../escape", "a/b", ""] {
        let out = run_with_config(Some(GRAPH_CONFIG), context_update(id, 60).as_bytes(), &env);
        // The graph still shows the current value.
        assert_eq!(out.stdout, "ctx 60% ▅\n", "{id}");
    }
    assert_eq!(files_under(state.path()), Vec::<String>::new());
    assert!(!state.path().join("escape.json").exists());
}

#[test]
fn predicts_the_context_filling_from_recent_history() {
    let state = tempfile::tempdir().unwrap();
    let sessions = state.path().join("astro-statusline/sessions");
    fs::create_dir_all(&sessions).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // 40% two minutes ago and 50% one minute ago. At 60% now, that's 10 points a minute, so 100%
    // comes in 4 minutes, even if a few seconds pass before the binary reads the clock.
    fs::write(
        sessions.join("abc.json"),
        format!(
            r#"{{"samples": [{{"t": {}, "context": 40}}, {{"t": {}, "context": 50}}]}}"#,
            now - 120,
            now - 60
        ),
    )
    .unwrap();
    let config = "segments = [\"context\"]\nformat.context = \"ctx {percent}[ full in {eta}]\"";
    let env = [
        ("NO_COLOR", "1"),
        ("XDG_STATE_HOME", state.path().to_str().unwrap()),
    ];
    let out = run_with_config(Some(config), context_update("abc", 60).as_bytes(), &env);
    assert_eq!(out.stdout, "ctx 60% full in 4m\n");
    // A session with no history yet has no rate to go on.
    let out = run_with_config(Some(config), context_update("new", 60).as_bytes(), &env);
    assert_eq!(out.stdout, "ctx 60%\n");
}

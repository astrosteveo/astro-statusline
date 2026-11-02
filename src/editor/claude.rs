//! Pointing Claude Code at astro-statusline: the `statusLine` entry in Claude Code's user
//! settings, `settings.json` in `$CLAUDE_CONFIG_DIR` or `~/.claude`
//! (https://code.claude.com/docs/en/env-vars).

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// Where Claude Code keeps its user settings, or `None` without a home directory.
pub fn settings_path() -> Option<PathBuf> {
    let dir = env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::home_dir()
                .filter(|home| home.is_absolute())
                .map(|home| home.join(".claude"))
        })?;
    Some(dir.join("settings.json"))
}

/// What the settings' status line runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// astro-statusline, by this command.
    Runs(String),
    NotSet,
    /// Some other command.
    Other(String),
    /// The settings can't be read, for this reason.
    Unreadable(String),
}

/// Claude Code's user settings, as read.
#[derive(Debug)]
pub struct Settings {
    pub path: PathBuf,
    json: Map<String, Value>,
}

/// A change to the settings, ready to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    /// The settings' new text.
    pub text: String,
    /// What changes, one line each.
    pub summary: Vec<String>,
}

impl Settings {
    /// Reads the settings. A missing file reads as no settings.
    pub fn read(path: &Path) -> Result<Settings, String> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let json = if text.trim().is_empty() {
            Map::new()
        } else {
            match serde_json::from_str(&text) {
                Ok(Value::Object(json)) => json,
                Ok(_) => return Err(format!("{}: isn't a JSON object", path.display())),
                Err(error) => return Err(format!("{}: {error}", path.display())),
            }
        };
        Ok(Settings {
            path: path.to_path_buf(),
            json,
        })
    }

    /// The status line's `padding`, when the settings set a number for it.
    pub fn padding(&self) -> Option<u64> {
        self.json.get("statusLine")?.get("padding")?.as_u64()
    }

    pub fn status(&self) -> Status {
        let Some(status_line) = self.json.get("statusLine") else {
            return Status::NotSet;
        };
        match status_line.get("command").and_then(Value::as_str) {
            Some(command) if runs_astro_statusline(command) => Status::Runs(command.to_string()),
            Some(command) => Status::Other(command.to_string()),
            None => Status::Other(status_line.to_string()),
        }
    }

    /// The settings with the status line running `exe` every second, keeping every other setting,
    /// their order, and the status line's own `padding` and `refreshInterval`.
    pub fn run(&self, exe: &Path) -> Change {
        let command = shell_quote(&exe.to_string_lossy());
        let mut json = self.json.clone();
        let mut summary = Vec::new();
        match json.get_mut("statusLine").and_then(Value::as_object_mut) {
            Some(status_line) => {
                let before = status_line
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("(none)");
                summary.push(format!("statusLine.command: {before} → {command}"));
                if status_line.get("type").and_then(Value::as_str) != Some("command") {
                    summary.push("statusLine.type: \"command\"".to_string());
                }
                status_line.insert("type".to_string(), json!("command"));
                status_line.insert("command".to_string(), json!(command));
                if !status_line.contains_key("refreshInterval") {
                    summary.push(
                        "statusLine.refreshInterval: 1 (fits the line to a resize within a second)"
                            .to_string(),
                    );
                    status_line.insert("refreshInterval".to_string(), json!(1));
                }
            }
            None => {
                if let Some(old) = json.get("statusLine") {
                    summary.push(format!("statusLine: replaces {old}"));
                }
                summary.push(format!("statusLine.command: {command}"));
                summary.push(
                    "statusLine.refreshInterval: 1 (fits the line to a resize within a second)"
                        .to_string(),
                );
                json.insert(
                    "statusLine".to_string(),
                    json!({"type": "command", "command": command, "refreshInterval": 1}),
                );
            }
        }
        let text = serde_json::to_string_pretty(&Value::Object(json)).expect("JSON serializes");
        Change {
            path: self.path.clone(),
            text: text + "\n",
            summary,
        }
    }
}

/// Whether the command runs astro-statusline, however the path to it is written.
fn runs_astro_statusline(command: &str) -> bool {
    command.split_whitespace().any(|word| {
        let word = word.trim_matches(['\'', '"']);
        Path::new(word)
            .file_name()
            .is_some_and(|name| name == "astro-statusline")
    })
}

/// `text` as one word for the shell Claude Code runs the command with.
fn shell_quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%".contains(c));
    if plain {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(text: &str) -> Settings {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, text).unwrap();
        Settings::read(&path).unwrap()
    }

    #[test]
    fn reads_the_status_line_s_padding() {
        assert_eq!(settings("{}").padding(), None);
        assert_eq!(
            settings(r#"{"statusLine": {"command": "x"}}"#).padding(),
            None
        );
        assert_eq!(
            settings(r#"{"statusLine": {"padding": 3}}"#).padding(),
            Some(3)
        );
        assert_eq!(
            settings(r#"{"statusLine": {"padding": "3"}}"#).padding(),
            None
        );
    }

    #[test]
    fn tells_what_the_status_line_runs() {
        assert_eq!(settings("{}").status(), Status::NotSet);
        assert_eq!(settings("").status(), Status::NotSet);
        let ours =
            r#"{"statusLine": {"type": "command", "command": "~/.cargo/bin/astro-statusline"}}"#;
        assert_eq!(
            settings(ours).status(),
            Status::Runs("~/.cargo/bin/astro-statusline".to_string())
        );
        let quoted = r#"{"statusLine": {"command": "'/my tools/astro-statusline' --x"}}"#;
        assert!(matches!(settings(quoted).status(), Status::Runs(_)));
        let other = r#"{"statusLine": {"type": "command", "command": "~/bin/line.sh"}}"#;
        assert_eq!(
            settings(other).status(),
            Status::Other("~/bin/line.sh".to_string())
        );
        let near_miss = r#"{"statusLine": {"command": "astro-statusline-old"}}"#;
        assert!(matches!(settings(near_miss).status(), Status::Other(_)));
    }

    #[test]
    fn reads_a_missing_file_as_no_settings_and_refuses_a_broken_one() {
        let dir = tempfile::tempdir().unwrap();
        let missing = Settings::read(&dir.path().join("settings.json")).unwrap();
        assert_eq!(missing.status(), Status::NotSet);

        let path = dir.path().join("broken.json");
        fs::write(&path, "{\"model\": ").unwrap();
        assert!(Settings::read(&path).unwrap_err().contains("broken.json"));
        fs::write(&path, "[1]").unwrap();
        assert!(
            Settings::read(&path)
                .unwrap_err()
                .contains("isn't a JSON object")
        );
    }

    #[test]
    fn adds_a_status_line_and_keeps_every_other_setting_in_order() {
        let change = settings(r#"{"model": "opus", "env": {"B": "1", "A": "2"}, "theme": "dark"}"#)
            .run(Path::new("/home/me/.cargo/bin/astro-statusline"));
        assert_eq!(
            change.text,
            r#"{
  "model": "opus",
  "env": {
    "B": "1",
    "A": "2"
  },
  "theme": "dark",
  "statusLine": {
    "type": "command",
    "command": "/home/me/.cargo/bin/astro-statusline",
    "refreshInterval": 1
  }
}
"#
        );
        assert_eq!(change.summary.len(), 2);
    }

    #[test]
    fn replaces_another_command_and_keeps_its_padding_and_interval() {
        let change = settings(
            r#"{"statusLine": {"type": "command", "command": "~/bin/line.sh", "padding": 2, "refreshInterval": 5}}"#,
        )
        .run(Path::new("/opt/my tools/astro-statusline"));
        let json: Value = serde_json::from_str(&change.text).unwrap();
        assert_eq!(
            json["statusLine"],
            json!({"type": "command", "command": "'/opt/my tools/astro-statusline'", "padding": 2, "refreshInterval": 5})
        );
        assert_eq!(
            change.summary,
            ["statusLine.command: ~/bin/line.sh → '/opt/my tools/astro-statusline'"]
        );
    }

    #[test]
    fn quotes_paths_the_shell_would_split() {
        assert_eq!(
            shell_quote("/usr/bin/astro-statusline"),
            "/usr/bin/astro-statusline"
        );
        assert_eq!(shell_quote("/a b/x"), "'/a b/x'");
        assert_eq!(shell_quote("/it's/x"), r"'/it'\''s/x'");
        assert_eq!(shell_quote("/$HOME/x"), "'/$HOME/x'");
    }
}

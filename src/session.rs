//! The session JSON that Claude Code writes to the status line command's stdin.
//!
//! Claude Code omits many fields until they apply and sends `null` for others early in a session,
//! so every field here is optional. A field of the wrong type reads as missing, so one odd value
//! costs one segment, not the whole line.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{DeserializeOwned, Deserializer};
use serde_json::Value;

#[derive(Debug, Default, Deserialize)]
pub struct Session {
    #[serde(default, deserialize_with = "lenient")]
    pub session_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub cwd: Option<PathBuf>,
    #[serde(default, deserialize_with = "lenient")]
    pub model: Option<Model>,
    #[serde(default, deserialize_with = "lenient")]
    pub workspace: Option<Workspace>,
    #[serde(default, deserialize_with = "lenient")]
    pub context_window: Option<ContextWindow>,
    /// Present only for claude.ai subscribers, after the session's first API response.
    #[serde(default, deserialize_with = "lenient")]
    pub rate_limits: Option<RateLimits>,
    #[serde(default, deserialize_with = "lenient")]
    pub cost: Option<Cost>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Model {
    #[serde(default, deserialize_with = "lenient")]
    pub display_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Workspace {
    #[serde(default, deserialize_with = "lenient")]
    pub current_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ContextWindow {
    /// Input tokens as a share of the context window, 0 to 100. `null` early in a session.
    #[serde(default, deserialize_with = "lenient")]
    pub used_percentage: Option<f64>,
    /// Input tokens in the context window, as of the latest response.
    #[serde(default, deserialize_with = "lenient")]
    pub total_input_tokens: Option<u64>,
    /// The most tokens the context window holds.
    #[serde(default, deserialize_with = "lenient")]
    pub context_window_size: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RateLimits {
    #[serde(default, deserialize_with = "lenient")]
    pub five_hour: Option<RateLimit>,
    #[serde(default, deserialize_with = "lenient")]
    pub seven_day: Option<RateLimit>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RateLimit {
    /// Share of the window's limit used, 0 to 100.
    #[serde(default, deserialize_with = "lenient")]
    pub used_percentage: Option<f64>,
    /// When the window resets, in seconds since the Unix epoch.
    #[serde(default, deserialize_with = "lenient")]
    pub resets_at: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Cost {
    /// Estimated at list price, so it may differ from the bill.
    #[serde(default, deserialize_with = "lenient")]
    pub total_cost_usd: Option<f64>,
    /// Wall-clock time the session has run, idle time included.
    #[serde(default, deserialize_with = "lenient")]
    pub total_duration_ms: Option<f64>,
}

impl Session {
    /// Parses the session from stdin's bytes. Input that is not a JSON object parses as an empty
    /// session.
    pub fn parse(input: &[u8]) -> Self {
        serde_json::from_slice(input).unwrap_or_default()
    }

    /// The current working directory, preferring `workspace.current_dir` as Claude Code's docs
    /// suggest.
    pub fn current_dir(&self) -> Option<&Path> {
        self.workspace
            .as_ref()
            .and_then(|w| w.current_dir.as_deref())
            .or(self.cwd.as_deref())
    }
}

/// Deserializes an optional field, reading a value of the wrong type (including `null`) as
/// `None`.
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_fields() {
        let session = Session::parse(
            br#"{
                "cwd": "/home/me/project/src",
                "model": {"id": "claude-opus-5-5", "display_name": "Opus"},
                "workspace": {"current_dir": "/home/me/project/src", "project_dir": "/home/me/project"}
            }"#,
        );
        assert_eq!(session.model.unwrap().display_name.as_deref(), Some("Opus"));
        assert_eq!(
            session.workspace.unwrap().current_dir,
            Some(PathBuf::from("/home/me/project/src"))
        );
    }

    #[test]
    fn missing_null_and_mistyped_fields_read_as_none() {
        let session =
            Session::parse(br#"{"cwd": null, "model": "Opus", "workspace": {"current_dir": 7}}"#);
        assert!(session.cwd.is_none());
        assert!(session.model.is_none());
        assert!(session.workspace.unwrap().current_dir.is_none());

        let session = Session::parse(b"{}");
        assert!(session.cwd.is_none() && session.model.is_none() && session.workspace.is_none());
    }

    #[test]
    fn input_that_is_not_a_json_object_parses_as_empty() {
        for input in [
            &b""[..],
            b"not json",
            b"[1, 2]",
            b"\"text\"",
            b"{\"cwd\": \"/a\"",
            b"\xff\xfe",
        ] {
            let session = Session::parse(input);
            assert!(
                session.cwd.is_none() && session.model.is_none() && session.workspace.is_none()
            );
        }
    }

    #[test]
    fn current_dir_prefers_workspace_over_cwd() {
        let session = Session::parse(br#"{"cwd": "/a", "workspace": {"current_dir": "/b"}}"#);
        assert_eq!(session.current_dir(), Some(Path::new("/b")));

        let session = Session::parse(br#"{"cwd": "/a", "workspace": {"current_dir": null}}"#);
        assert_eq!(session.current_dir(), Some(Path::new("/a")));
    }
}

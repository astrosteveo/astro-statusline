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
    pub cwd: Option<PathBuf>,
    #[serde(default, deserialize_with = "lenient")]
    pub model: Option<Model>,
    #[serde(default, deserialize_with = "lenient")]
    pub workspace: Option<Workspace>,
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

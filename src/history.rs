//! A short history of each session's values, so graphs can show how they changed between
//! updates. Claude Code sends only the current values.
//!
//! Each session keeps its own small file, named by its session id. A run reads it, adds the
//! current values when they changed, and writes it back through a temporary file and a rename,
//! so a run killed partway (Claude Code cancels a run when a newer update arrives) leaves the old
//! file whole. There is no fsync: a file torn by a crash reads as corrupt, which reads as empty.
//! Two overlapping runs can lose one sample, since the last writer wins.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;

use serde::{Deserialize, Serialize};

use crate::dirs;
use crate::session::Session;

/// The most samples kept, and so the most a graph can show.
pub const MAX_SAMPLES: usize = 64;

/// A history file larger than this is not read, so a stray file can't slow every run.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// The values of one update, at `t` seconds since the Unix epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub t: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

impl Sample {
    /// The session's current values, as of `t`.
    pub fn new(session: &Session, t: i64) -> Self {
        let rate_limits = session.rate_limits.as_ref();
        Sample {
            t,
            context: session
                .context_window
                .as_ref()
                .and_then(|w| w.used_percentage),
            five_hour: rate_limits
                .and_then(|r| r.five_hour.as_ref())
                .and_then(|l| l.used_percentage),
            seven_day: rate_limits
                .and_then(|r| r.seven_day.as_ref())
                .and_then(|l| l.used_percentage),
            cost: session.cost.as_ref().and_then(|c| c.total_cost_usd),
        }
    }

    /// Whether the values match exactly. Values round-trip through JSON unchanged, so an update
    /// that changed nothing compares equal and writes nothing.
    fn same_values(&self, other: &Sample) -> bool {
        (self.context, self.five_hour, self.seven_day, self.cost)
            == (other.context, other.five_hour, other.seven_day, other.cost)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    samples: Vec<Sample>,
}

/// The session's samples, oldest first, ending with its current values. They're recorded in the
/// session's history file when the session has a usable id and the clock can be read. A problem
/// with the file comes back with the samples, for the caller to report; the samples are then
/// whatever could be read, plus the current values.
pub fn update(session: &Session, now: Option<i64>) -> (Vec<Sample>, Option<String>) {
    let current = Sample::new(session, now.unwrap_or_default());
    // Without the clock, a sample has no time to be stored under.
    match now.and_then(|_| path(session.session_id.as_deref())) {
        Some(path) => record(&path, current),
        None => (vec![current], None),
    }
}

/// The history file for session `id`, when `id` is safe to use as a file name.
fn path(id: Option<&str>) -> Option<PathBuf> {
    let id = id.filter(|id| valid_session_id(id))?;
    Some(dirs::state()?.join("sessions").join(format!("{id}.json")))
}

/// Session ids come from stdin, so only plain names can become file names: no dots, no
/// slashes, and nothing that could climb out of the directory.
fn valid_session_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn record(path: &Path, current: Sample) -> (Vec<Sample>, Option<String>) {
    let describe = |error: io::Error| format!("{}: {error}", path.display());
    let (mut samples, mut error) = match load(path) {
        Ok(samples) => (samples, None),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (Vec::new(), None),
        Err(e) => (Vec::new(), Some(describe(e))),
    };
    // A clock that stepped back would otherwise leave samples from the future in place.
    samples.retain(|sample| sample.t <= current.t);
    if samples
        .last()
        .is_some_and(|last| last.same_values(&current))
    {
        return (samples, error);
    }
    samples.push(current);
    let excess = samples.len().saturating_sub(MAX_SAMPLES);
    samples.drain(..excess);
    if let Err(e) = save(path, &samples) {
        error.get_or_insert(describe(e));
    }
    (samples, error)
}

fn load(path: &Path) -> io::Result<Vec<Sample>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too large"));
    }
    let stored: Stored = serde_json::from_slice(&bytes)?;
    Ok(stored.samples)
}

fn save(path: &Path, samples: &[Sample]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec(&Stored {
        samples: samples.to_vec(),
    })?;
    // The process id keeps two runs from writing the same temporary file.
    let tmp = path.with_extension(format!("json.{}.tmp", process::id()));
    let result = fs::write(&tmp, json).and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(t: i64, context: f64) -> Sample {
        Sample {
            t,
            context: Some(context),
            ..Sample::default()
        }
    }

    fn contexts(samples: &[Sample]) -> Vec<f64> {
        samples.iter().filter_map(|s| s.context).collect()
    }

    #[test]
    fn records_a_sample_only_when_a_value_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions/abc.json");

        let (samples, error) = record(&path, sample(100, 10.0));
        assert_eq!((contexts(&samples), error), (vec![10.0], None));
        let (samples, _) = record(&path, sample(101, 10.0));
        assert_eq!(
            samples,
            [sample(100, 10.0)],
            "an unchanged update writes nothing"
        );
        let (samples, _) = record(&path, sample(160, 60.0));
        assert_eq!(contexts(&samples), [10.0, 60.0]);
        assert_eq!(load(&path).unwrap(), samples);
        // Only the history file is left behind.
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["abc.json"]);
    }

    #[test]
    fn keeps_the_latest_samples() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.json");
        for t in 0..MAX_SAMPLES as i64 + 10 {
            record(&path, sample(t, t as f64));
        }
        let samples = load(&path).unwrap();
        assert_eq!(samples.len(), MAX_SAMPLES);
        assert_eq!(samples.first().unwrap().t, 10);
    }

    #[test]
    fn starts_over_from_a_corrupt_or_oversized_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.json");
        for contents in [b"{\"samples\": [".to_vec(), vec![b' '; 70 * 1024]] {
            fs::write(&path, contents).unwrap();
            let (samples, error) = record(&path, sample(100, 10.0));
            assert_eq!(contexts(&samples), [10.0]);
            assert!(error.unwrap().starts_with(&path.display().to_string()));
            // The bad file was replaced by a good one.
            assert_eq!(load(&path).unwrap(), samples);
        }
    }

    #[test]
    fn drops_samples_from_the_future() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.json");
        record(&path, sample(100, 10.0));
        record(&path, sample(200, 20.0));
        // The clock stepped back to between the two.
        let (samples, _) = record(&path, sample(150, 30.0));
        assert_eq!(samples, [sample(100, 10.0), sample(150, 30.0)]);
    }

    #[test]
    fn reports_a_file_it_cannot_write() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the sessions directory should be.
        fs::write(dir.path().join("sessions"), "").unwrap();
        let (samples, error) = record(&dir.path().join("sessions/abc.json"), sample(100, 10.0));
        assert_eq!(contexts(&samples), [10.0]);
        assert!(error.is_some());
    }

    #[test]
    fn accepts_only_plain_session_ids() {
        for id in ["abc", "3f2a-9c_X", &"a".repeat(128)] {
            assert!(valid_session_id(id), "{id}");
        }
        for id in [
            "",
            "../x",
            "a/b",
            "a.b",
            ".",
            "..",
            "a b",
            "ä",
            &"a".repeat(129),
        ] {
            assert!(!valid_session_id(id), "{id}");
        }
    }
}

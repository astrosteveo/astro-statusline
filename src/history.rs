//! A short history of each session's values, so graphs can show how they changed between
//! updates. Claude Code sends only the current values.
//!
//! Each session keeps its own small file, named by its session id. A run reads it, adds the
//! current values when they changed, and writes it back through a temporary file and a rename,
//! so a run killed partway (Claude Code cancels a run when a newer update arrives) leaves the old
//! file whole. There is no fsync: a file torn by a crash reads as corrupt, which reads as empty.
//! Two overlapping runs can lose one sample, since the last writer wins. When a new session
//! writes its first sample, files of sessions untouched for a week are deleted.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::dirs;
use crate::session::Session;

/// The most samples kept, and so the most a graph can show.
pub const MAX_SAMPLES: usize = 64;

/// A history file larger than this is not read, so a stray file can't slow every run.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Session files untouched for this long are deleted.
const MAX_AGE: Duration = Duration::from_secs(7 * 86_400);

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
    let mut new_session = false;
    let (mut samples, mut error) = match load(path) {
        Ok(samples) => (samples, None),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            new_session = true;
            (Vec::new(), None)
        }
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
    match save(path, &samples) {
        // Once per session is often enough, and a file that won't save is no time to clean up.
        Ok(()) if new_session => {
            if let Some(dir) = path.parent() {
                prune(dir, SystemTime::now());
            }
        }
        Ok(()) => {}
        Err(e) => {
            error.get_or_insert(describe(e));
        }
    }
    (samples, error)
}

/// Deletes session files in `dir` last changed more than `MAX_AGE` before `now`. `dir` is this
/// program's own sessions directory, and even there only regular files directly in it with names
/// this program writes are candidates, so a link, a subdirectory, or any other file is left alone.
fn prune(dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // `file_type` doesn't follow symlinks, so a link is never a regular file here.
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        if !is_file || !is_ours(&entry.file_name()) {
            continue;
        }
        let modified = entry.metadata().and_then(|meta| meta.modified());
        // A time after `now` is an error here, and counts as recent.
        let old =
            modified.is_ok_and(|time| now.duration_since(time).is_ok_and(|age| age > MAX_AGE));
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Whether `name` is one `save` writes: `<id>.json`, or `<id>.json.<pid>.tmp` left by a run that
/// was killed.
fn is_ours(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    if let Some(id) = name.strip_suffix(".json") {
        return valid_session_id(id);
    }
    name.strip_suffix(".tmp")
        .and_then(|rest| rest.rsplit_once(".json."))
        .is_some_and(|(id, pid)| {
            valid_session_id(id) && !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit())
        })
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

    /// Writes `name` in `dir`, last changed `days_ago` days ago (negative for the future).
    fn backdated(dir: &Path, name: &str, days_ago: i64) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "{}").unwrap();
        let offset = Duration::from_secs(days_ago.unsigned_abs() * 86_400);
        let time = if days_ago >= 0 {
            SystemTime::now() - offset
        } else {
            SystemTime::now() + offset
        };
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(time)
            .unwrap();
        path
    }

    // Unix only for the symlink.
    #[cfg(unix)]
    #[test]
    fn prunes_only_its_own_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let old = backdated(dir.path(), "old-session.json", 8);
        let killed = backdated(dir.path(), "old-session.json.4242.tmp", 8);
        let recent = backdated(dir.path(), "recent.json", 6);
        let future = backdated(dir.path(), "future.json", -3);
        let keep = [
            recent,
            future,
            backdated(dir.path(), "my notes.json", 30),
            backdated(dir.path(), "notes.json.bak", 30),
            backdated(dir.path(), "x.txt", 30),
            backdated(dir.path(), "a.b.json", 30),
            backdated(dir.path(), "abc.json.tmp", 30),
            backdated(dir.path(), "abc.json.12x.tmp", 30),
            backdated(dir.path(), ".json", 30),
        ];
        // A link with a session's name, to an old file elsewhere, and a directory with one.
        let elsewhere = tempfile::tempdir().unwrap();
        let target = backdated(elsewhere.path(), "target.json", 30);
        let link = dir.path().join("linked.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let subdir = dir.path().join("subdir.json");
        fs::create_dir(&subdir).unwrap();
        let nested = backdated(&subdir, "nested.json", 30);

        prune(dir.path(), SystemTime::now());

        assert!(!old.exists() && !killed.exists());
        for path in keep.iter().chain([&link, &target, &subdir, &nested]) {
            assert!(path.exists(), "{} was deleted", path.display());
        }
    }

    #[test]
    fn a_new_session_prunes_and_an_old_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let stale = backdated(dir.path(), "stale.json", 8);
        let ongoing = dir.path().join("ongoing.json");
        fs::write(&ongoing, r#"{"samples": []}"#).unwrap();
        record(&ongoing, sample(100, 10.0));
        assert!(stale.exists(), "an ongoing session pruned");
        record(&dir.path().join("new.json"), sample(100, 10.0));
        assert!(!stale.exists(), "a new session didn't prune");
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

//! Saving a file the user keeps: through a temporary file and a rename, so a crash halfway leaves
//! the old file whole, and through any symlink, so a file a dotfile manager links in stays linked.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

/// Writes `text` to `path`, first copying what was there to `backup`. A read-only file is an
/// error, not replaced.
pub fn write(path: &Path, text: &str, backup: &Path) -> io::Result<()> {
    let target = resolve(path)?;
    let existing = match fs::metadata(&target) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    match &existing {
        // The rename would replace it anyway, since that needs only the directory writable.
        Some(metadata) if metadata.permissions().readonly() => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the file is read-only",
            ));
        }
        Some(_) => {
            fs::copy(&target, backup)?;
        }
        None => {
            if let Some(dir) = target.parent() {
                fs::create_dir_all(dir)?;
            }
        }
    }

    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temporary = target.with_file_name(format!(".{name}.{}.tmp", process::id()));
    let result = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        if let Some(metadata) = &existing {
            file.set_permissions(metadata.permissions())?;
        }
        file.sync_all()?;
        fs::rename(&temporary, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Whether `error` means the file can't be written, as opposed to something going wrong.
pub fn is_read_only(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
    )
}

/// `path` with the symlinks at its end followed, even to a target that doesn't exist yet.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let mut path = path.to_path_buf();
    // Linux gives up after 40 links too.
    for _ in 0..40 {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link = fs::read_link(&path)?;
                // `join` keeps an absolute link as is and resolves a relative one against the
                // link's directory.
                path = match path.parent() {
                    Some(dir) => dir.join(link),
                    None => link,
                };
            }
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("too many levels of symlinks"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn writes_a_new_file_and_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/config.toml");
        let backup = dir.path().join("a/b/config.toml.bak");
        write(&path, "new", &backup).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert!(!backup.exists());
    }

    #[test]
    fn keeps_the_previous_file_and_its_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let (path, backup) = (
            dir.path().join("config.toml"),
            dir.path().join("config.toml.bak"),
        );
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write(&path, "new", &backup).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "old");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // No temporary file is left behind.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn writes_through_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let dotfiles = dir.path().join("dotfiles");
        fs::create_dir(&dotfiles).unwrap();
        fs::write(dotfiles.join("config.toml"), "old").unwrap();
        let config = dir.path().join("config");
        fs::create_dir(&config).unwrap();
        let path = config.join("config.toml");
        // A relative link, then a link to that link.
        symlink("../dotfiles/config.toml", config.join("real.toml")).unwrap();
        symlink("real.toml", &path).unwrap();

        write(&path, "new", &config.join("config.toml.bak")).unwrap();
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(dotfiles.join("config.toml")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(config.join("config.toml.bak")).unwrap(),
            "old"
        );

        // A link whose target doesn't exist yet makes the target.
        let dangling = config.join("dangling.toml");
        symlink("../dotfiles/later.toml", &dangling).unwrap();
        write(&dangling, "later", &config.join("dangling.bak")).unwrap();
        assert_eq!(
            fs::read_to_string(dotfiles.join("later.toml")).unwrap(),
            "later"
        );

        let loop_path = config.join("loop.toml");
        symlink("loop.toml", &loop_path).unwrap();
        assert!(write(&loop_path, "x", &config.join("loop.bak")).is_err());
    }

    #[test]
    fn refuses_a_read_only_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let error = write(&path, "new", &dir.path().join("config.toml.bak")).unwrap_err();
        assert!(is_read_only(&error), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
        assert!(!dir.path().join("config.toml.bak").exists());
    }
}

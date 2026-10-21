//! Reads the checked-out branch straight from the repository's `HEAD` file, so the status line
//! never waits on a `git` process or a repository lock.

use std::fs;
use std::path::{Path, PathBuf};

/// The branch checked out in the repository that contains `dir`, or the short commit hash when
/// `HEAD` is detached. `None` outside a repository, or when `HEAD` can't be read.
pub fn head(dir: &Path) -> Option<String> {
    let head = fs::read_to_string(git_dir(dir)?.join("HEAD")).ok()?;
    parse_head(&head)
}

/// The git directory of the nearest `.git` at or above `dir`. Linked worktrees and submodules
/// have a `.git` file that points at their real git directory.
fn git_dir(dir: &Path) -> Option<PathBuf> {
    let dot_git = dir
        .ancestors()
        .map(|ancestor| ancestor.join(".git"))
        .find(|dot_git| dot_git.exists())?;
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let pointer = fs::read_to_string(&dot_git).ok()?;
    let target = pointer.strip_prefix("gitdir:")?.trim();
    // `join` keeps an absolute target as is and resolves a relative one against the `.git` file.
    Some(dot_git.parent()?.join(target))
}

fn parse_head(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim();
        let branch = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        // Repositories that store refs in a reftable keep a stub `HEAD` naming `.invalid`, and
        // the real branch is only in the reftable.
        return (!branch.is_empty() && branch != ".invalid").then(|| branch.to_string());
    }
    let is_hash = head.len() >= 7 && head.bytes().all(|b| b.is_ascii_hexdigit());
    is_hash.then(|| head[..7].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn reads_the_branch_from_any_directory_in_the_repository() {
        let repo = tempfile::tempdir().unwrap();
        write(&repo.path().join(".git/HEAD"), "ref: refs/heads/feat/x\n");
        let deeper = repo.path().join("src/nested");
        fs::create_dir_all(&deeper).unwrap();

        assert_eq!(head(repo.path()).as_deref(), Some("feat/x"));
        assert_eq!(head(&deeper).as_deref(), Some("feat/x"));
    }

    #[test]
    fn follows_a_gitdir_pointer_absolute_or_relative() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("main/.git/worktrees/wt");
        write(&real.join("HEAD"), "ref: refs/heads/wt-branch\n");

        let absolute = root.path().join("absolute");
        write(
            &absolute.join(".git"),
            &format!("gitdir: {}\n", real.display()),
        );
        assert_eq!(head(&absolute).as_deref(), Some("wt-branch"));

        let relative = root.path().join("relative");
        write(
            &relative.join(".git"),
            "gitdir: ../main/.git/worktrees/wt\n",
        );
        assert_eq!(head(&relative).as_deref(), Some("wt-branch"));
    }

    #[test]
    fn stops_at_the_nearest_git_even_when_it_is_broken() {
        let outer = tempfile::tempdir().unwrap();
        write(&outer.path().join(".git/HEAD"), "ref: refs/heads/outer\n");
        let inner = outer.path().join("inner");
        write(&inner.join(".git"), "not a pointer\n");

        assert_eq!(head(&inner), None);
    }

    #[test]
    fn shortens_a_detached_head_to_seven_characters() {
        assert_eq!(
            parse_head("1400a73c2b9e4f0d8a6b5c3d2e1f0a9b8c7d6e5f\n").as_deref(),
            Some("1400a73")
        );
    }

    #[test]
    fn rejects_heads_it_cannot_name() {
        for head in [
            "",
            "ref:",
            "ref: refs/heads/.invalid\n",
            "abc",
            "not-a-hash-at-all",
        ] {
            assert_eq!(parse_head(head), None, "{head:?}");
        }
    }
}

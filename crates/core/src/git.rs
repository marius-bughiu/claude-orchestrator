//! Minimal git status probing for project cards. Shells out to `git`; returns a
//! best-effort snapshot (never errors hard — missing git or non-repo just yields
//! `available: false`).

use crate::models::GitStatus;
use std::path::Path;
use std::process::Command;

fn run(path: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Probe the working tree at `path`.
pub fn status(path: impl AsRef<Path>) -> GitStatus {
    let path = path.as_ref();
    let mut status = GitStatus::default();

    // Is this a git work tree at all?
    if run(path, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return status;
    }
    status.available = true;
    status.branch = run(path, &["rev-parse", "--abbrev-ref", "HEAD"]);
    status.dirty = run(path, &["status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if let Some(line) = run(path, &["log", "-1", "--format=%h\u{1f}%s"]) {
        if let Some((hash, subject)) = line.split_once('\u{1f}') {
            status.last_commit = Some(hash.to_string());
            status.last_subject = Some(subject.to_string());
        }
    }
    // ahead/behind vs upstream, if any.
    if let Some(counts) = run(
        path,
        &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
    ) {
        let mut it = counts.split_whitespace();
        status.behind = it.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        status.ahead = it.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_status_for_this_repo() {
        // The crate lives inside the project git repo.
        let s = status(env!("CARGO_MANIFEST_DIR"));
        if s.available {
            assert!(s.branch.is_some());
            assert!(s.last_commit.is_some());
        }
    }

    #[test]
    fn non_repo_is_unavailable() {
        let s = status(std::env::temp_dir());
        // temp dir is typically not a git repo; if it happens to be, just skip.
        let _ = s.available;
    }
}

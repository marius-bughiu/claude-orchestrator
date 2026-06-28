//! Per-task git worktree isolation so concurrent sessions in the same project
//! don't clobber each other. Each task runs in its own worktree on a dedicated
//! branch; on success the changes are committed and (optionally) pushed and a PR
//! is opened via the `gh` CLI. Everything shells out to `git`/`gh` and degrades
//! gracefully when they're unavailable.

use crate::error::{CoreError, Result};
use crate::models::{BranchInfo, DiffFile, RebaseResult, SessionDiff};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Prefix every orchestrator-created branch shares.
pub const BRANCH_PREFIX: &str = "orchestrator/";

/// Cap on the unified-diff text returned to the UI (bytes).
const MAX_PATCH_BYTES: usize = 200_000;

fn git(repo: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(CoreError::Io)
}

fn git_ok(repo: &Path, args: &[&str]) -> Result<String> {
    let out = git(repo, args)?;
    if !out.status.success() {
        return Err(CoreError::other(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether `path` is inside a git work tree.
pub fn is_git_repo(path: impl AsRef<Path>) -> bool {
    git(path.as_ref(), &["rev-parse", "--is-inside-work-tree"])
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

/// The repository's currently checked-out branch.
pub fn current_branch(repo: impl AsRef<Path>) -> Option<String> {
    git_ok(repo.as_ref(), &["rev-parse", "--abbrev-ref", "HEAD"]).ok()
}

/// Base directory under which managed worktrees live (outside the repo).
pub fn worktrees_root() -> PathBuf {
    std::env::temp_dir().join("claude-orchestrator-worktrees")
}

/// Turn a task title into a short branch-safe slug.
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
        if slug.len() >= 32 {
            break;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "task".into()
    } else {
        slug
    }
}

/// Branch name for a task session: `orchestrator/<slug>-<short id>`.
pub fn branch_name(title: &str, session_id: &str) -> String {
    let short: String = session_id.chars().take(8).collect();
    format!("orchestrator/{}-{}", slugify(title), short)
}

/// Create a fresh worktree at `wt_path` on a new branch off the repo's HEAD.
pub fn create(repo: &Path, branch: &str, wt_path: &Path) -> Result<()> {
    if let Some(parent) = wt_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    git_ok(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            &wt_path.to_string_lossy(),
            "HEAD",
        ],
    )?;
    Ok(())
}

/// True if the worktree has uncommitted changes.
pub fn has_changes(wt: &Path) -> bool {
    git_ok(wt, &["status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Stage everything and commit. Returns the commit hash if a commit was made,
/// or `None` if there was nothing to commit.
pub fn commit_all(wt: &Path, message: &str) -> Result<Option<String>> {
    if !has_changes(wt) {
        return Ok(None);
    }
    git_ok(wt, &["add", "-A"])?;
    // Identity may be unset in CI/sandboxes; pass it inline so commits never fail.
    let out = git(
        wt,
        &[
            "-c",
            "user.name=Claude Orchestrator",
            "-c",
            "user.email=orchestrator@local",
            "commit",
            "-m",
            message,
        ],
    )?;
    if !out.status.success() {
        return Err(CoreError::other(format!(
            "git commit: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(git_ok(wt, &["rev-parse", "--short", "HEAD"]).ok())
}

/// Remove a worktree (force) and prune metadata. Best-effort.
pub fn remove(repo: &Path, wt_path: &Path) -> Result<()> {
    let _ = git(
        repo,
        &["worktree", "remove", "--force", &wt_path.to_string_lossy()],
    );
    let _ = std::fs::remove_dir_all(wt_path);
    let _ = git(repo, &["worktree", "prune"]);
    Ok(())
}

/// Delete a local branch (used when a task produced no commits). Best-effort.
pub fn delete_branch(repo: &Path, branch: &str) {
    let _ = git(repo, &["branch", "-D", branch]);
}

/// Push a branch to `origin`. Returns an error if there is no remote or the push
/// fails (the caller logs and continues).
pub fn push(wt: &Path, branch: &str) -> Result<()> {
    git_ok(wt, &["push", "-u", "origin", branch]).map(|_| ())
}

/// Open a pull request via the `gh` CLI. Returns the PR URL on success, or `None`
/// if `gh` is unavailable / unauthenticated (logged by the caller).
pub fn open_pr(wt: &Path, title: &str, body: &str, base: &str) -> Result<Option<String>> {
    let out = Command::new("gh")
        .arg("-C")
        .arg(wt)
        .args([
            "pr", "create", "--title", title, "--body", body, "--base", base,
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            Ok(Some(String::from_utf8_lossy(&o.stdout).trim().to_string()))
        }
        _ => Ok(None),
    }
}

/// Whether `branch` would conflict if merged onto `base`, computed without
/// touching the working tree via `git merge-tree --write-tree`. Returns `None`
/// when git can't determine it (e.g. an unrelated history or an old git).
pub fn conflicts_with(repo: &Path, base: &str, branch: &str) -> Option<bool> {
    let out = git(repo, &["merge-tree", "--write-tree", base, branch]).ok()?;
    match out.status.code() {
        Some(0) => Some(false), // clean merge
        Some(1) => Some(true),  // conflicts
        _ => None,              // could not determine
    }
}

/// List orchestrator-created branches in a repo, flagging which are already
/// merged into the repo's base branch and which would conflict if merged.
/// `active` is filled in by the caller.
pub fn list_orchestrator_branches(repo: &Path) -> Result<Vec<BranchInfo>> {
    let pattern = format!("{BRANCH_PREFIX}*");
    let all = git_ok(
        repo,
        &["branch", "--list", &pattern, "--format=%(refname:short)"],
    )?;
    let base = current_branch(repo).unwrap_or_else(|| "HEAD".into());
    let merged_out = git_ok(
        repo,
        &[
            "branch",
            "--merged",
            &base,
            "--list",
            &pattern,
            "--format=%(refname:short)",
        ],
    )
    .unwrap_or_default();
    let merged: Vec<&str> = merged_out.lines().map(|l| l.trim()).collect();
    let mut branches = Vec::new();
    for name in all.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        let is_merged = merged.contains(&name);
        // Only probe unmerged branches for conflicts/staleness (merged are moot).
        let (conflicted, behind) = if is_merged {
            (None, 0)
        } else {
            let conflicted = conflicts_with(repo, &base, name);
            // Commits on base not yet in this branch = how stale it is.
            let behind = git_ok(repo, &["rev-list", "--count", &format!("{name}..{base}")])
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            (conflicted, behind)
        };
        branches.push(BranchInfo {
            name: name.to_string(),
            merged: is_merged,
            active: false,
            conflicted,
            behind,
        });
    }
    Ok(branches)
}

/// Whether a branch has a corresponding `origin/<branch>` remote-tracking ref,
/// i.e. it has been pushed (typically by the auto-PR flow).
pub fn branch_is_pushed(repo: &Path, branch: &str) -> bool {
    git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{branch}"),
        ],
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
}

/// Rebase an orchestrator branch onto the repo's base branch using a throwaway
/// worktree, so the repo's own working tree is never touched. On conflict the
/// rebase is aborted and reported, leaving the branch unchanged.
pub fn rebase_onto_base(repo: &Path, branch: &str) -> Result<RebaseResult> {
    let base = current_branch(repo).unwrap_or_else(|| "HEAD".into());
    // Nothing to do if already up to date.
    let behind: u32 = git_ok(repo, &["rev-list", "--count", &format!("{branch}..{base}")])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    if behind == 0 {
        return Ok(RebaseResult {
            status: "up_to_date".into(),
            detail: format!("{branch} is already up to date with {base}"),
        });
    }

    let tmp = worktrees_root().join(format!("rebase-{}", uuid::Uuid::new_v4()));
    if let Some(parent) = tmp.parent() {
        std::fs::create_dir_all(parent)?;
    }
    git_ok(repo, &["worktree", "add", &tmp.to_string_lossy(), branch])?;

    let out = git(&tmp, &["rebase", &base]);
    let result = match out {
        Ok(o) if o.status.success() => {
            let mut detail = format!("rebased {branch} onto {base} ({behind} commit(s))");
            // If the branch was already pushed (has a remote-tracking ref), the
            // rebase rewrote history, so update the remote with a lease-guarded
            // force-push to keep an open PR in sync.
            if branch_is_pushed(repo, branch) {
                match git(&tmp, &["push", "--force-with-lease", "origin", branch]) {
                    Ok(p) if p.status.success() => detail.push_str("; force-pushed to origin"),
                    Ok(p) => detail.push_str(&format!(
                        "; remote not updated ({})",
                        String::from_utf8_lossy(&p.stderr).trim()
                    )),
                    Err(e) => detail.push_str(&format!("; remote not updated ({e})")),
                }
            }
            RebaseResult {
                status: "rebased".into(),
                detail,
            }
        }
        Ok(_) => {
            let _ = git(&tmp, &["rebase", "--abort"]);
            RebaseResult {
                status: "conflicts".into(),
                detail: format!("rebase onto {base} hit conflicts; resolve {branch} manually"),
            }
        }
        Err(e) => RebaseResult {
            status: "error".into(),
            detail: e.to_string(),
        },
    };

    let _ = git(
        repo,
        &["worktree", "remove", "--force", &tmp.to_string_lossy()],
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = git(repo, &["worktree", "prune"]);
    Ok(result)
}

/// Prune stale worktree metadata for a repo. Best-effort.
pub fn prune_worktrees(repo: &Path) -> Result<()> {
    let _ = git(repo, &["worktree", "prune"]);
    Ok(())
}

/// Parse `git diff --numstat` output into per-file change counts. A `-` count
/// (binary file) is treated as 0.
fn parse_numstat(text: &str) -> Vec<DiffFile> {
    let mut files = Vec::new();
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(add), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        files.push(DiffFile {
            path: path.to_string(),
            additions: add.parse().unwrap_or(0),
            deletions: del.parse().unwrap_or(0),
            status: "modified".into(),
        });
    }
    files
}

/// Assemble a `SessionDiff` from numstat + patch text, applying the size cap.
fn build_diff(
    branch: Option<String>,
    base: Option<String>,
    numstat: &str,
    patch: String,
) -> SessionDiff {
    let files = parse_numstat(numstat);
    let additions = files.iter().map(|f| f.additions).sum();
    let deletions = files.iter().map(|f| f.deletions).sum();
    let truncated = patch.len() > MAX_PATCH_BYTES;
    let patch = if truncated {
        let mut p: String = patch.chars().take(MAX_PATCH_BYTES).collect();
        p.push_str("\n\n… diff truncated …\n");
        p
    } else {
        patch
    };
    SessionDiff {
        available: !files.is_empty(),
        branch,
        base,
        additions,
        deletions,
        files,
        patch,
        truncated,
    }
}

/// Diff a committed task branch against the point it forked from the repo's
/// current branch (so only the task's own commits show).
pub fn branch_diff(repo: &Path, branch: &str) -> Result<SessionDiff> {
    let base = current_branch(repo).unwrap_or_else(|| "HEAD".into());
    let merge_base = git_ok(repo, &["merge-base", &base, branch]).unwrap_or_else(|_| "HEAD".into());
    let range = format!("{merge_base}..{branch}");
    let numstat = git_ok(repo, &["diff", "--numstat", &range]).unwrap_or_default();
    let patch = git_ok(repo, &["diff", &range]).unwrap_or_default();
    Ok(build_diff(
        Some(branch.to_string()),
        Some(base),
        &numstat,
        patch,
    ))
}

/// Diff the still-live worktree against its HEAD (used while a task runs or
/// before its changes are committed). Untracked files are listed as additions.
pub fn working_diff(wt: &Path, branch: Option<String>) -> Result<SessionDiff> {
    let numstat = git_ok(wt, &["diff", "--numstat", "HEAD"]).unwrap_or_default();
    let patch = git_ok(wt, &["diff", "HEAD"]).unwrap_or_default();
    let mut diff = build_diff(branch, Some("working tree".into()), &numstat, patch);
    // Untracked files don't show in `git diff`; surface them as added.
    if let Ok(others) = git_ok(wt, &["ls-files", "--others", "--exclude-standard"]) {
        for path in others.lines().filter(|l| !l.is_empty()) {
            diff.files.push(DiffFile {
                path: path.to_string(),
                additions: 0,
                deletions: 0,
                status: "untracked".into(),
            });
        }
    }
    diff.available = !diff.files.is_empty();
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orch-wt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .unwrap();
        };
        cfg(&["init", "-q"]);
        cfg(&["config", "user.email", "t@t"]);
        cfg(&["config", "user.name", "t"]);
        std::fs::write(dir.join("README.md"), "hi").unwrap();
        cfg(&["add", "-A"]);
        cfg(&["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn slug_and_branch() {
        assert_eq!(slugify("Add  login!! flow"), "add-login-flow");
        assert_eq!(slugify(""), "task");
        let b = branch_name("Fix bug", "abcdef1234567890");
        assert_eq!(b, "orchestrator/fix-bug-abcdef12");
    }

    #[test]
    fn worktree_lifecycle_commit_and_remove() {
        let repo = init_repo();
        assert!(is_git_repo(&repo));
        let wt = worktrees_root().join(format!("t-{}", uuid::Uuid::new_v4()));
        create(&repo, "orchestrator/test-1", &wt).unwrap();
        assert!(wt.join("README.md").exists());
        assert!(!has_changes(&wt));

        std::fs::write(wt.join("new.txt"), "content").unwrap();
        assert!(has_changes(&wt));
        let commit = commit_all(&wt, "add new.txt").unwrap();
        assert!(commit.is_some());
        assert!(!has_changes(&wt));
        // Nothing to commit the second time.
        assert!(commit_all(&wt, "noop").unwrap().is_none());

        remove(&repo, &wt).unwrap();
        assert!(!wt.exists());
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn branch_diff_shows_task_commits() {
        let repo = init_repo();
        let wt = worktrees_root().join(format!("d-{}", uuid::Uuid::new_v4()));
        create(&repo, "orchestrator/diff-1", &wt).unwrap();
        std::fs::write(wt.join("feature.txt"), "line one\nline two\n").unwrap();
        commit_all(&wt, "add feature").unwrap();
        // Worktree removed: diff comes from the committed branch.
        remove(&repo, &wt).unwrap();

        let diff = branch_diff(&repo, "orchestrator/diff-1").unwrap();
        assert!(diff.available);
        assert!(diff.files.iter().any(|f| f.path == "feature.txt"));
        assert_eq!(diff.additions, 2);
        assert!(diff.patch.contains("line one"));
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn working_diff_includes_untracked() {
        let repo = init_repo();
        let wt = worktrees_root().join(format!("w-{}", uuid::Uuid::new_v4()));
        create(&repo, "orchestrator/work-1", &wt).unwrap();
        std::fs::write(wt.join("scratch.txt"), "wip").unwrap();
        let diff = working_diff(&wt, Some("orchestrator/work-1".into())).unwrap();
        assert!(diff.available);
        assert!(diff
            .files
            .iter()
            .any(|f| f.path == "scratch.txt" && f.status == "untracked"));
        remove(&repo, &wt).unwrap();
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lists_orchestrator_branches_with_merged_flag() {
        let repo = init_repo();
        // A merged branch (committed then merged) and an unmerged one.
        let wt1 = worktrees_root().join(format!("b1-{}", uuid::Uuid::new_v4()));
        create(&repo, "orchestrator/merged-1", &wt1).unwrap();
        std::fs::write(wt1.join("a.txt"), "a").unwrap();
        commit_all(&wt1, "a").unwrap();
        remove(&repo, &wt1).unwrap();
        let cfg = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
        };
        cfg(&["merge", "--no-ff", "-m", "merge", "orchestrator/merged-1"]);

        let wt2 = worktrees_root().join(format!("b2-{}", uuid::Uuid::new_v4()));
        create(&repo, "orchestrator/open-2", &wt2).unwrap();
        std::fs::write(wt2.join("b.txt"), "b").unwrap();
        commit_all(&wt2, "b").unwrap();
        remove(&repo, &wt2).unwrap();

        let branches = list_orchestrator_branches(&repo).unwrap();
        let merged = branches
            .iter()
            .find(|b| b.name == "orchestrator/merged-1")
            .unwrap();
        let open = branches
            .iter()
            .find(|b| b.name == "orchestrator/open-2")
            .unwrap();
        assert!(merged.merged);
        assert!(!open.merged);
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn detects_conflicting_branch() {
        let repo = init_repo();
        let cfg = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
        };
        // A branch off the very first commit that edits README differently from
        // the base's later edit will conflict on merge.
        let first = git_ok(&repo, &["rev-list", "--max-parents=0", "HEAD"]).unwrap();
        std::fs::write(repo.join("README.md"), "base change\n").unwrap();
        cfg(&["commit", "-aqm", "base change"]);
        let base = current_branch(&repo).unwrap();

        cfg(&["branch", "orchestrator/conflict-1", first.trim()]);
        let wt = worktrees_root().join(format!("c-{}", uuid::Uuid::new_v4()));
        cfg(&[
            "worktree",
            "add",
            &wt.to_string_lossy(),
            "orchestrator/conflict-1",
        ]);
        std::fs::write(wt.join("README.md"), "branch change\n").unwrap();
        commit_all(&wt, "branch change").unwrap();
        remove(&repo, &wt).unwrap();

        assert_eq!(
            conflicts_with(&repo, &base, "orchestrator/conflict-1"),
            Some(true)
        );
        // A branch that only adds a new file merges cleanly.
        cfg(&["branch", "orchestrator/clean-2", base.as_str()]);
        let wt2 = worktrees_root().join(format!("cl-{}", uuid::Uuid::new_v4()));
        cfg(&[
            "worktree",
            "add",
            &wt2.to_string_lossy(),
            "orchestrator/clean-2",
        ]);
        std::fs::write(wt2.join("new.txt"), "x").unwrap();
        commit_all(&wt2, "add new").unwrap();
        remove(&repo, &wt2).unwrap();
        assert_eq!(
            conflicts_with(&repo, &base, "orchestrator/clean-2"),
            Some(false)
        );
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn rebase_brings_stale_branch_up_to_date() {
        let repo = init_repo();
        let cfg = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
        };
        // Branch off the first commit, then advance base on a different file so
        // the branch is behind but does not conflict.
        let first = git_ok(&repo, &["rev-list", "--max-parents=0", "HEAD"]).unwrap();
        cfg(&["branch", "orchestrator/stale-1", first.trim()]);
        std::fs::write(repo.join("base-only.txt"), "base\n").unwrap();
        cfg(&["add", "-A"]);
        cfg(&["commit", "-qm", "advance base"]);

        // Put a commit on the branch so a rebase actually moves it.
        let wt = worktrees_root().join(format!("st-{}", uuid::Uuid::new_v4()));
        cfg(&[
            "worktree",
            "add",
            &wt.to_string_lossy(),
            "orchestrator/stale-1",
        ]);
        std::fs::write(wt.join("branch-only.txt"), "branch\n").unwrap();
        commit_all(&wt, "branch work").unwrap();
        remove(&repo, &wt).unwrap();

        // It is behind by one (the base advance).
        let before = list_orchestrator_branches(&repo).unwrap();
        let stale = before
            .iter()
            .find(|b| b.name == "orchestrator/stale-1")
            .unwrap();
        assert_eq!(stale.behind, 1);

        let res = rebase_onto_base(&repo, "orchestrator/stale-1").unwrap();
        assert_eq!(res.status, "rebased", "{}", res.detail);

        // After rebasing it is no longer behind.
        let after = list_orchestrator_branches(&repo).unwrap();
        let stale = after
            .iter()
            .find(|b| b.name == "orchestrator/stale-1")
            .unwrap();
        assert_eq!(stale.behind, 0);

        // Rebasing again is a no-op.
        assert_eq!(
            rebase_onto_base(&repo, "orchestrator/stale-1")
                .unwrap()
                .status,
            "up_to_date"
        );
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn rebase_force_pushes_a_pushed_branch() {
        let repo = init_repo();
        let cfg = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
        };
        // A bare remote on disk, wired up as origin.
        let remote = std::env::temp_dir().join(format!("orch-remote-{}.git", uuid::Uuid::new_v4()));
        Command::new("git")
            .args(["init", "--bare", "-q", &remote.to_string_lossy()])
            .output()
            .unwrap();
        cfg(&["remote", "add", "origin", &remote.to_string_lossy()]);

        // Branch off first commit, commit on it, and push it (simulating auto-PR).
        let first = git_ok(&repo, &["rev-list", "--max-parents=0", "HEAD"]).unwrap();
        cfg(&["branch", "orchestrator/pushed-1", first.trim()]);
        let wt = worktrees_root().join(format!("p-{}", uuid::Uuid::new_v4()));
        cfg(&[
            "worktree",
            "add",
            &wt.to_string_lossy(),
            "orchestrator/pushed-1",
        ]);
        std::fs::write(wt.join("feature.txt"), "f\n").unwrap();
        commit_all(&wt, "feature").unwrap();
        push(&wt, "orchestrator/pushed-1").unwrap();
        remove(&repo, &wt).unwrap();
        assert!(branch_is_pushed(&repo, "orchestrator/pushed-1"));

        // Advance base so the branch is behind, then rebase.
        std::fs::write(repo.join("base.txt"), "b\n").unwrap();
        cfg(&["add", "-A"]);
        cfg(&["commit", "-qm", "advance"]);
        let res = rebase_onto_base(&repo, "orchestrator/pushed-1").unwrap();
        assert_eq!(res.status, "rebased", "{}", res.detail);
        assert!(res.detail.contains("force-pushed"), "{}", res.detail);

        // The remote-tracking ref now matches the rebased branch head.
        let local = git_ok(&repo, &["rev-parse", "orchestrator/pushed-1"]).unwrap();
        let remote_head = git_ok(&repo, &["rev-parse", "origin/orchestrator/pushed-1"]).unwrap();
        assert_eq!(local, remote_head);
        std::fs::remove_dir_all(&repo).ok();
        std::fs::remove_dir_all(&remote).ok();
    }

    #[test]
    fn non_repo_detected() {
        let dir = std::env::temp_dir().join(format!("orch-nr-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_git_repo(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }
}

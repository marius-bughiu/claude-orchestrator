//! GitHub issue ingestion via the `gh` CLI.
//!
//! Shells out to `gh issue list --json ...` inside a project's repo and parses
//! the result. Never panics; a missing `gh`, no auth, or a non-GitHub repo just
//! yields an error the caller can surface. No webhooks — this is pull-only.

use crate::error::{CoreError, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// One open issue as returned by `gh issue list --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct GithubIssue {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Label {
    #[serde(default)]
    pub name: String,
}

impl GithubIssue {
    pub fn label_names(&self) -> Vec<String> {
        self.labels.iter().map(|l| l.name.clone()).collect()
    }
}

/// True if `gh` is on PATH.
pub fn available() -> bool {
    crate::util::binary_available("gh")
}

/// List open issues for the repository at `repo_path`. `limit` caps results.
pub fn list_open_issues(repo_path: impl AsRef<Path>, limit: u32) -> Result<Vec<GithubIssue>> {
    let limit = limit.clamp(1, 200).to_string();
    let out = Command::new("gh")
        .arg("-C")
        .arg(repo_path.as_ref())
        .args([
            "issue",
            "list",
            "--state",
            "open",
            "--limit",
            &limit,
            "--json",
            "number,title,body,url,labels",
        ])
        .output()
        .map_err(|e| CoreError::Other(format!("failed to run gh: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(CoreError::Other(format!(
            "gh issue list failed: {}",
            stderr.trim()
        )));
    }
    let issues: Vec<GithubIssue> = serde_json::from_slice(&out.stdout)?;
    Ok(issues)
}

/// The tag used to mark (and dedupe) a task imported from a given issue.
pub fn issue_tag(number: u64) -> String {
    format!("gh-issue-{number}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_tag_is_stable() {
        assert_eq!(issue_tag(42), "gh-issue-42");
    }

    #[test]
    fn parses_issue_json() {
        let json = r#"[{"number":7,"title":"Fix bug","body":"details","url":"https://x/7","labels":[{"name":"bug"}]}]"#;
        let issues: Vec<GithubIssue> = serde_json::from_str(json).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 7);
        assert_eq!(issues[0].label_names(), vec!["bug".to_string()]);
    }
}

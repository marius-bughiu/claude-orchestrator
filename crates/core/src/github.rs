//! GitHub issue ingestion via the `gh` CLI.
//!
//! Shells out to `gh issue list --json ...` inside a project's repo and parses
//! the result. Never panics; a missing `gh`, no auth, or a non-GitHub repo just
//! yields an error the caller can surface. No webhooks — this is pull-only.

use crate::error::{CoreError, Result};
use crate::models::PullRequest;
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

/// One pull request as returned by `gh pr list --json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPr {
    number: u64,
    title: String,
    url: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    head_ref_name: String,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    mergeable: Option<String>,
    #[serde(default)]
    status_check_rollup: Vec<serde_json::Value>,
}

/// Summarize a PR's status-check rollup into one of
/// "passing" | "failing" | "pending" | "none".
fn ci_summary(items: &[serde_json::Value]) -> String {
    if items.is_empty() {
        return "none".into();
    }
    let mut pending = false;
    let mut failing = false;
    for it in items {
        let field = |k: &str| {
            it.get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_uppercase()
        };
        let conclusion = field("conclusion");
        let state = field("state");
        let status = field("status");
        if matches!(
            conclusion.as_str(),
            "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
        ) || matches!(state.as_str(), "FAILURE" | "ERROR")
        {
            failing = true;
        } else if matches!(
            status.as_str(),
            "IN_PROGRESS" | "QUEUED" | "PENDING" | "WAITING" | "REQUESTED"
        ) || state == "PENDING"
            || (conclusion.is_empty() && state.is_empty() && status.is_empty())
        {
            pending = true;
        }
    }
    if failing {
        "failing".into()
    } else if pending {
        "pending".into()
    } else {
        "passing".into()
    }
}

impl From<GhPr> for PullRequest {
    fn from(p: GhPr) -> Self {
        let ci = ci_summary(&p.status_check_rollup);
        PullRequest {
            number: p.number,
            title: p.title,
            url: p.url,
            state: p.state,
            draft: p.is_draft,
            branch: p.head_ref_name,
            ci,
            review_decision: p.review_decision,
            mergeable: p.mergeable,
        }
    }
}

/// List open pull requests for the repository at `repo_path`, with CI and review
/// state summarized.
pub fn list_open_prs(repo_path: impl AsRef<Path>) -> Result<Vec<PullRequest>> {
    let out = Command::new("gh")
        .arg("-C")
        .arg(repo_path.as_ref())
        .args([
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            "50",
            "--json",
            "number,title,url,state,isDraft,headRefName,reviewDecision,mergeable,statusCheckRollup",
        ])
        .output()
        .map_err(|e| CoreError::Other(format!("failed to run gh: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Other(format!(
            "gh pr list failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let prs: Vec<GhPr> = serde_json::from_slice(&out.stdout)?;
    Ok(prs.into_iter().map(Into::into).collect())
}

/// Merge a pull request by number (squash merge, deleting the branch).
pub fn merge_pr(repo_path: impl AsRef<Path>, number: u64) -> Result<()> {
    let out = Command::new("gh")
        .arg("-C")
        .arg(repo_path.as_ref())
        .args([
            "pr",
            "merge",
            &number.to_string(),
            "--squash",
            "--delete-branch",
        ])
        .output()
        .map_err(|e| CoreError::Other(format!("failed to run gh: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Other(format!(
            "gh pr merge failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_tag_is_stable() {
        assert_eq!(issue_tag(42), "gh-issue-42");
    }

    #[test]
    fn ci_summary_buckets() {
        use serde_json::json;
        assert_eq!(ci_summary(&[]), "none");
        assert_eq!(ci_summary(&[json!({"conclusion": "SUCCESS"})]), "passing");
        assert_eq!(
            ci_summary(&[
                json!({"conclusion": "SUCCESS"}),
                json!({"conclusion": "FAILURE"})
            ]),
            "failing"
        );
        assert_eq!(ci_summary(&[json!({"status": "IN_PROGRESS"})]), "pending");
    }

    #[test]
    fn parses_pr_json() {
        let json = r#"[{"number":42,"title":"Add streaming","url":"https://x/42","state":"OPEN","isDraft":false,"headRefName":"orchestrator/add-streaming-1a2b","reviewDecision":"APPROVED","mergeable":"MERGEABLE","statusCheckRollup":[{"conclusion":"SUCCESS"}]}]"#;
        let prs: Vec<GhPr> = serde_json::from_str(json).unwrap();
        let pr: PullRequest = prs.into_iter().next().unwrap().into();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.ci, "passing");
        assert_eq!(pr.review_decision.as_deref(), Some("APPROVED"));
        assert_eq!(pr.branch, "orchestrator/add-streaming-1a2b");
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

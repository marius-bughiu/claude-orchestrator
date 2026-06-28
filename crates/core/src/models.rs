//! Core data model shared across the orchestrator.
//!
//! Every struct here is serialized to the frontend (camelCase JSON) and persisted
//! to SQLite. Keep these definitions free of any GUI/Tauri concerns.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which CLI agent executes a piece of work. Claude is the orchestrator and the
/// default executor; Gemini and Codex are sub-agents work can be delegated to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Gemini,
    Codex,
}

impl AgentKind {
    pub const ALL: [AgentKind; 3] = [AgentKind::Claude, AgentKind::Gemini, AgentKind::Codex];

    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Gemini => "gemini",
            AgentKind::Codex => "codex",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<AgentKind> {
        match s.to_ascii_lowercase().as_str() {
            "claude" => Some(AgentKind::Claude),
            "gemini" => Some(AgentKind::Gemini),
            "codex" => Some(AgentKind::Codex),
            _ => None,
        }
    }

    /// The default model alias used when nothing more specific is configured.
    /// Claude defaults to the latest Opus; the other CLIs default to their own
    /// latest model (represented as `None`, i.e. no `--model` flag).
    pub fn default_model(&self) -> Option<&'static str> {
        match self {
            AgentKind::Claude => Some("opus"),
            AgentKind::Gemini => None,
            AgentKind::Codex => None,
        }
    }
}

/// Lifecycle of a task as it moves through the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Newly created, eligible to be scheduled.
    Pending,
    /// Picked up by the scheduler, a session is starting.
    Queued,
    /// A session is actively executing this task.
    Running,
    /// Session finished but the verifier judged the goal incomplete; will be retried.
    NeedsReview,
    /// Verified complete.
    Completed,
    /// Exhausted retries or the agent reported an unrecoverable error.
    Failed,
    /// Manually cancelled.
    Cancelled,
    /// Waiting on a dependency (`depends_on`).
    Blocked,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::NeedsReview => "needs_review",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::Blocked => "blocked",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> TaskStatus {
        match s {
            "queued" => TaskStatus::Queued,
            "running" => TaskStatus::Running,
            "needs_review" => TaskStatus::NeedsReview,
            "completed" => TaskStatus::Completed,
            "failed" => TaskStatus::Failed,
            "cancelled" => TaskStatus::Cancelled,
            "blocked" => TaskStatus::Blocked,
            _ => TaskStatus::Pending,
        }
    }

    /// Terminal states are never rescheduled.
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Completed | TaskStatus::Cancelled)
    }

    /// States the scheduler may pick up and (re)run.
    pub fn is_schedulable(&self) -> bool {
        matches!(self, TaskStatus::Pending | TaskStatus::NeedsReview)
    }
}

/// What a session was spawned to do. Drives which convention file / system prompt
/// is used and how the result is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// Execute a task.
    Task,
    /// Generate new tasks for a project whose queue is empty (`roadmap.md`).
    Roadmap,
    /// Judge whether a finished task actually met its goal (`verify.md`).
    Verify,
}

impl SessionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionKind::Task => "task",
            SessionKind::Roadmap => "roadmap",
            SessionKind::Verify => "verify",
        }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> SessionKind {
        match s {
            "roadmap" => SessionKind::Roadmap,
            "verify" => SessionKind::Verify,
            _ => SessionKind::Task,
        }
    }
}

/// Lifecycle of a single agent session (one CLI invocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Pending => "pending",
            SessionStatus::Running => "running",
            SessionStatus::Completed => "completed",
            SessionStatus::Failed => "failed",
            SessionStatus::Cancelled => "cancelled",
            SessionStatus::TimedOut => "timed_out",
        }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> SessionStatus {
        match s {
            "running" => SessionStatus::Running,
            "completed" => SessionStatus::Completed,
            "failed" => SessionStatus::Failed,
            "cancelled" => SessionStatus::Cancelled,
            "timed_out" => SessionStatus::TimedOut,
            _ => SessionStatus::Pending,
        }
    }
    pub fn is_active(&self) -> bool {
        matches!(self, SessionStatus::Pending | SessionStatus::Running)
    }
}

/// A local git repository the orchestrator manages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Absolute path to the local git repo.
    pub path: String,
    pub description: Option<String>,
    /// When false the scheduler ignores this project entirely.
    pub enabled: bool,
    /// Default agent for tasks in this project that don't specify one.
    pub default_agent: AgentKind,
    /// The set of agents this project is permitted to use. Defaults to Claude
    /// only; the scheduler will never dispatch to an agent outside this set.
    pub allowed_agents: Vec<AgentKind>,
    /// Per-project cap on concurrent sessions (None = use global setting).
    pub max_concurrent: Option<u32>,
    /// Whether the roadmap loop may auto-generate tasks when the queue empties.
    pub roadmap_enabled: bool,
    /// Whether finished tasks are auto-verified by a verifier session.
    pub verify_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Project {
    /// The agents this project may use, guaranteed non-empty and always
    /// including the default agent (falls back to `[default_agent]`).
    pub fn effective_allowed_agents(&self) -> Vec<AgentKind> {
        let mut agents: Vec<AgentKind> = self.allowed_agents.clone();
        if !agents.contains(&self.default_agent) {
            agents.insert(0, self.default_agent);
        }
        if agents.is_empty() {
            agents.push(self.default_agent);
        }
        agents
    }

    pub fn allows(&self, agent: AgentKind) -> bool {
        self.effective_allowed_agents().contains(&agent)
    }
}

/// A unit of work for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    /// The prompt / instructions handed to the agent.
    pub description: String,
    pub status: TaskStatus,
    /// Higher runs first. Convention: 0 low, 50 normal, 100 high, 200 urgent.
    pub priority: i64,
    /// Preferred agent. When `auto_agent` is true this is only a fallback — the
    /// scheduler may pick a different allowed agent to balance load.
    pub agent: AgentKind,
    /// When true, the agent was not explicitly pinned and the scheduler is free
    /// to load-balance across the project's allowed agents.
    pub auto_agent: bool,
    /// Model override for sessions of this task. `None` = use the agent default
    /// (latest Opus for Claude; the CLI's own latest for others).
    pub model: Option<String>,
    /// Task that spawned this one (roadmap loop, decomposition).
    pub parent_id: Option<String>,
    /// Task ids that must be Completed before this is schedulable.
    pub depends_on: Vec<String>,
    /// How many sessions have run for this task.
    pub attempts: u32,
    /// Hard cap on attempts before the task is Failed.
    pub max_attempts: u32,
    /// Free-form labels for filtering.
    pub tags: Vec<String>,
    /// Set by the roadmap loop so generated tasks are distinguishable.
    pub auto_generated: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Aggregated token/cost usage for a single session or rolled up across many.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    pub num_turns: u32,
}

impl TokenUsage {
    pub fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.total_cost_usd += other.total_cost_usd;
        self.num_turns += other.num_turns;
    }
}

/// One CLI invocation against an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub task_id: Option<String>,
    pub project_id: String,
    pub agent: AgentKind,
    pub kind: SessionKind,
    pub status: SessionStatus,
    /// The agent's own session id (e.g. Claude's `session_id`) for resuming.
    pub agent_session_id: Option<String>,
    pub model: Option<String>,
    /// The prompt this session was launched with.
    pub prompt: String,
    /// Final assistant text / result summary.
    pub result_text: Option<String>,
    pub error: Option<String>,
    pub exit_code: Option<i32>,
    pub usage: TokenUsage,
    /// Git branch this session worked on (when worktree isolation is enabled).
    pub branch: Option<String>,
    /// URL of a pull request opened for this session's branch, if any.
    pub pr_url: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A persisted streaming event from a session (normalized from the agent's
/// stream-json output). The raw line is kept for fidelity/debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    pub id: i64,
    pub session_id: String,
    /// Normalized event kind: "init" | "assistant" | "thinking" | "tool_use"
    /// | "tool_result" | "result" | "error" | "raw".
    pub kind: String,
    /// Human-readable text content (assistant text, tool name, error message…).
    pub text: Option<String>,
    /// Structured payload (tool input, usage, etc.).
    pub data: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Usage within one rolling window (session or weekly), with optional configured
/// limits and the resulting fraction used.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowUsage {
    pub usage: TokenUsage,
    pub window_hours: u32,
    pub window_started_at: Option<DateTime<Utc>>,
    pub cost_limit_usd: Option<f64>,
    pub token_limit: Option<u64>,
    /// Fraction of the cost limit used (0.0–1.0+), if a limit is configured.
    pub cost_pct: Option<f64>,
    /// Fraction of the token limit used (0.0–1.0+), if a limit is configured.
    pub token_pct: Option<f64>,
}

/// Per-agent usage rollup with optional configured limits, surfaced in the top bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsage {
    pub agent: AgentKind,
    pub available: bool,
    pub active_sessions: u32,
    /// Usage within the short (session) window — e.g. the rolling 5h plan window.
    pub session: WindowUsage,
    /// Usage within the weekly window.
    pub weekly: WindowUsage,
    /// Usage since the app started tracking (all time).
    pub total: TokenUsage,
}

/// User-configured usage limits for an agent (session + weekly cost/token caps).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLimits {
    /// Cost ceiling (USD) within the session window.
    pub session_cost_usd: Option<f64>,
    /// Cost ceiling (USD) within the weekly window.
    pub weekly_cost_usd: Option<f64>,
    /// Token ceiling within the session window.
    pub session_token_limit: Option<u64>,
    /// Token ceiling within the weekly window.
    pub weekly_token_limit: Option<u64>,
}

/// One bucket of aggregated usage over a time period (day/month/year) for charts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePoint {
    /// Period key: "YYYY-MM-DD" (day), "YYYY-MM" (month), or "YYYY" (year).
    pub period: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub num_turns: u32,
    pub sessions: u32,
}

/// Aggregate performance comparison for one agent across its finished task
/// sessions (roadmap/verify sessions excluded). Surfaced in analytics so users
/// can compare agents by reliability, cost, and speed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStat {
    pub agent: AgentKind,
    /// Finished task sessions counted (completed + failed + timed out).
    pub sessions: u32,
    pub completed: u32,
    pub failed: u32,
    /// Fraction of counted sessions that completed successfully (0.0–1.0).
    pub success_rate: f64,
    /// Mean cost per counted session, USD.
    pub avg_cost_usd: f64,
    pub total_cost_usd: f64,
    /// Mean wall-clock duration (seconds) over sessions with both timestamps.
    pub avg_duration_secs: f64,
}

/// One changed file in a session's diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    /// "modified" | "added" | "deleted" | "renamed" | "untracked".
    pub status: String,
}

/// The set of changes a task session made on its worktree branch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDiff {
    /// False when the session was not isolated, produced no commit, or the
    /// branch/worktree is no longer available.
    pub available: bool,
    /// The branch the work lives on, if any.
    pub branch: Option<String>,
    /// The base the diff is computed against (e.g. the repo's main branch, or
    /// "working tree" while the task is still running).
    pub base: Option<String>,
    pub files: Vec<DiffFile>,
    pub additions: u32,
    pub deletions: u32,
    /// Unified diff text (truncated for very large diffs).
    pub patch: String,
    /// True if `patch` was truncated.
    pub truncated: bool,
}

/// An open pull request for a project, with CI / review state summarized.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// "OPEN" | "MERGED" | "CLOSED".
    pub state: String,
    pub draft: bool,
    pub branch: String,
    /// Summarized CI rollup: "passing" | "failing" | "pending" | "none".
    pub ci: String,
    /// "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED" | null.
    pub review_decision: Option<String>,
    /// "MERGEABLE" | "CONFLICTING" | "UNKNOWN" | null.
    pub mergeable: Option<String>,
}

/// Detection result for one agent CLI: whether its binary is on PATH and, if so,
/// the version string it reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHealth {
    pub agent: AgentKind,
    /// The binary name/path probed.
    pub binary: String,
    pub available: bool,
    /// First line of `<binary> --version`, if it ran.
    pub version: Option<String>,
}

/// A managed (orchestrator-created) branch in a project repo, for cleanup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    /// True if the branch is already merged into the repo's base branch.
    pub merged: bool,
    /// True if a currently-running session is working on this branch.
    pub active: bool,
    /// Whether the branch merges cleanly onto the base: `Some(true)` = conflicts,
    /// `Some(false)` = clean, `None` = could not determine (or already merged).
    pub conflicted: Option<bool>,
    /// How many commits the base branch is ahead of this branch (i.e. how stale
    /// it is). 0 when up to date or already merged.
    pub behind: u32,
}

/// Outcome of attempting to rebase a task branch onto its base.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseResult {
    /// "rebased" | "up_to_date" | "conflicts" | "error".
    pub status: String,
    /// Human-readable detail.
    pub detail: String,
}

/// A project's accumulated memory: auto-generated context and learned lessons.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMemory {
    /// Contents of `.orchestrator/context.md`, if present.
    pub context: Option<String>,
    /// Contents of `.orchestrator/lessons.md`, if present.
    pub lessons: Option<String>,
}

/// Snapshot of the whole orchestrator for the header / status views.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorStatus {
    pub running: bool,
    /// True while draining for an update: no new work is scheduled.
    pub draining: bool,
    pub active_sessions: u32,
    pub max_concurrent: u32,
    pub pending_tasks: u32,
    pub projects: u32,
    pub agents: Vec<AgentUsage>,
}

/// A single entry in the timeline view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItem {
    pub session_id: String,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub project_id: String,
    pub project_name: String,
    pub agent: AgentKind,
    pub kind: SessionKind,
    pub status: SessionStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub cost_usd: f64,
}

/// A scheduled task discovered from a markdown file in a project repo
/// (`.orchestrator/scheduled/*.md`). The schedule lives in the file's front
/// matter; the body is the task prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    /// Stable id derived from project id + relative file path.
    pub id: String,
    pub project_id: String,
    /// Absolute path to the markdown file.
    pub path: String,
    /// Path relative to the project root (for display).
    pub rel_path: String,
    pub title: String,
    /// Raw schedule expression as written in front matter.
    pub schedule: String,
    /// "cron" or "interval".
    pub schedule_kind: String,
    /// Human-readable schedule summary.
    pub schedule_desc: String,
    /// Agent override from front matter (None = project default / balanced).
    pub agent: Option<AgentKind>,
    /// Model override from front matter.
    pub model: Option<String>,
    pub priority: i64,
    pub enabled: bool,
    /// Whether the file parsed successfully; if false, `error` explains why.
    pub valid: bool,
    pub error: Option<String>,
    /// The task prompt (markdown body after front matter).
    pub body: String,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A best-effort git working-tree snapshot for a project.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub available: bool,
    pub branch: Option<String>,
    pub dirty: bool,
    pub ahead: u32,
    pub behind: u32,
    pub last_commit: Option<String>,
    pub last_subject: Option<String>,
}

/// A projected future firing of a scheduled task, for the "Upcoming" lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingTask {
    pub scheduled_id: String,
    pub project_id: String,
    pub project_name: String,
    pub title: String,
    pub agent: Option<AgentKind>,
    pub priority: i64,
    pub schedule_desc: String,
    pub run_at: DateTime<Utc>,
}

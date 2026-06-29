//! Global, app-wide settings (persisted as a single JSON row in the DB).

use crate::models::{AgentKind, AgentLimits};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Permission posture for spawned Claude sessions. Full autonomy requires
/// bypassing prompts; we make that an explicit, opt-in choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    /// `--permission-mode default` — agent asks; only safe for supervised runs.
    Default,
    /// `--permission-mode acceptEdits`
    AcceptEdits,
    /// `--permission-mode plan`
    Plan,
    /// `--dangerously-skip-permissions` — required for unattended autonomy.
    #[default]
    BypassPermissions,
}

/// Per-agent execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    /// Override the binary path/name (defaults to the agent's conventional name).
    pub binary: Option<String>,
    /// Model id to pass to the agent, if any.
    pub model: Option<String>,
    /// Extra CLI args appended to every invocation.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Configured usage limits for the header display.
    #[serde(default)]
    pub limits: AgentLimits,
    /// Short ("session") rolling window length in hours (Claude plans reset every
    /// 5h, hence 5).
    #[serde(default = "default_session_window_hours", alias = "windowHours")]
    pub session_window_hours: u32,
    /// Weekly rolling window length in hours (default 168 = 7 days).
    #[serde(default = "default_weekly_window_hours")]
    pub weekly_window_hours: u32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_session_window_hours() -> u32 {
    5
}
fn default_weekly_window_hours() -> u32 {
    168
}
fn default_true() -> bool {
    true
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            binary: None,
            model: None,
            extra_args: Vec::new(),
            limits: AgentLimits::default(),
            session_window_hours: 5,
            weekly_window_hours: 168,
            enabled: true,
        }
    }
}

/// An outbound notification webhook. Posts a JSON payload (shaped for the
/// target service) whenever a matching orchestrator event fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookConfig {
    /// Stable id (used by the UI for list keys / editing).
    pub id: String,
    /// Human label.
    #[serde(default)]
    pub name: String,
    /// Destination URL.
    pub url: String,
    /// Payload shape: "slack", "discord", or "generic".
    #[serde(default = "default_webhook_kind")]
    pub kind: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Fire when a task completes successfully.
    #[serde(default = "default_true")]
    pub on_task_complete: bool,
    /// Fire when a task fails (exhausted retries / unrecoverable error).
    #[serde(default = "default_true")]
    pub on_task_fail: bool,
    /// Restrict this webhook to specific projects. Empty = all projects.
    #[serde(default)]
    pub project_ids: Vec<String>,
    /// Optional message template with `{event}`, `{title}`, `{body}`, `{project}`,
    /// `{task}`, `{status}`, `{link}` placeholders. Empty = built-in format.
    #[serde(default)]
    pub template: String,
}

fn default_webhook_kind() -> String {
    "slack".to_string()
}
fn default_retry_base_secs() -> u64 {
    60
}
fn default_retry_max_secs() -> u64 {
    3600
}
fn default_activity_retention() -> u32 {
    2000
}
fn default_backup_interval_hours() -> u64 {
    24
}
fn default_quiet_hours_end() -> u8 {
    8
}

/// Global orchestrator settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Whether the scheduler loop is running.
    pub running: bool,
    /// Global cap on simultaneously active sessions across all projects.
    pub max_concurrent: u32,
    /// How often the scheduler wakes to look for work, in seconds.
    pub tick_interval_secs: u64,
    /// Default agent for new tasks/projects.
    pub default_agent: AgentKind,
    /// Permission posture for Claude sessions.
    pub permission_mode: PermissionMode,
    /// Hard wall-clock timeout for a single session, in seconds (0 = none).
    pub session_timeout_secs: u64,
    /// Global default for whether the roadmap loop may run.
    pub roadmap_enabled: bool,
    /// Global default for whether finished tasks are verified.
    pub verify_enabled: bool,
    /// When true, tasks that don't pin an agent are dispatched to the least-used
    /// available agent among the project's allowed set, to even out usage.
    pub balance_agents: bool,
    /// When true, task sessions run in live mode: token-by-token streaming and
    /// mid-run message injection (Claude only). Disable to fall back to one-shot
    /// sessions.
    #[serde(default = "default_true")]
    pub live_streaming: bool,
    /// When true, the app raises a desktop notification when a task completes or
    /// fails.
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    /// Run each task session in its own git worktree (on a dedicated branch) so
    /// concurrent sessions in the same repo don't clobber each other.
    #[serde(default = "default_true")]
    pub isolate_worktrees: bool,
    /// After a successful isolated task, commit any uncommitted changes on the
    /// task branch.
    #[serde(default = "default_true")]
    pub auto_commit: bool,
    /// After committing, push the branch and open a pull request via `gh`.
    #[serde(default)]
    pub auto_pr: bool,
    /// How often to re-scan projects for scheduled-task markdown files, seconds.
    pub schedule_refresh_secs: u64,
    /// When true, failed tasks wait an (exponentially growing) backoff before
    /// being retried instead of re-running immediately. Scheduled tasks are not
    /// retried regardless (they run on their own cadence).
    #[serde(default = "default_true")]
    pub retry_enabled: bool,
    /// Base backoff for the first retry, in seconds. Doubles each attempt.
    #[serde(default = "default_retry_base_secs")]
    pub retry_base_secs: u64,
    /// Cap on the retry backoff, in seconds.
    #[serde(default = "default_retry_max_secs")]
    pub retry_max_secs: u64,
    /// Maximum number of activity-log entries to retain; older ones are pruned.
    #[serde(default = "default_activity_retention")]
    pub activity_retention: u32,
    /// Anti-starvation: how much a waiting task's effective scheduling priority
    /// grows per hour it has waited (0 = off). Keeps low-priority tasks from
    /// being buried forever behind a stream of higher-priority work.
    #[serde(default)]
    pub priority_aging_per_hour: f64,
    /// Cap on a project's pending queue that the roadmap loop will fill to
    /// (0 = unlimited). Bounds the self-generated backlog.
    #[serde(default)]
    pub roadmap_max_pending: u32,
    /// Minimum minutes between roadmap runs for a project (0 = no cooldown).
    /// Stops the loop from regenerating immediately after a run.
    #[serde(default)]
    pub roadmap_min_interval_mins: u32,
    /// When true, desktop notifications are suppressed during the quiet-hours
    /// window (local time), so unattended overnight runs don't ping the user.
    #[serde(default)]
    pub quiet_hours_enabled: bool,
    /// Quiet-hours window start/end as local hours [0, 23]. The window wraps past
    /// midnight when start > end (e.g. 22 → 8).
    #[serde(default)]
    pub quiet_hours_start: u8,
    #[serde(default = "default_quiet_hours_end")]
    pub quiet_hours_end: u8,
    /// When true, the config (settings + projects) is auto-exported on a cadence.
    #[serde(default)]
    pub backup_enabled: bool,
    /// How often to write a config backup, in hours.
    #[serde(default = "default_backup_interval_hours")]
    pub backup_interval_hours: u64,
    /// Directory to write config backups into (empty = backups disabled).
    #[serde(default)]
    pub backup_dir: String,
    /// Outbound notification webhooks (Slack / Discord / generic).
    #[serde(default)]
    pub webhooks: Vec<WebhookConfig>,
    /// Reusable task presets for quick task creation.
    #[serde(default)]
    pub task_templates: Vec<TaskTemplate>,
    /// Per-agent configuration, keyed by agent name.
    pub agents: BTreeMap<String, AgentConfig>,
}

/// A reusable preset for creating tasks. Fields map onto the task-create form;
/// `agent` empty means "auto / balanced".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub agent: Option<AgentKind>,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let mut agents = BTreeMap::new();
        for kind in AgentKind::ALL {
            agents.insert(kind.as_str().to_string(), AgentConfig::default());
        }
        Settings {
            running: false,
            max_concurrent: 3,
            tick_interval_secs: 10,
            default_agent: AgentKind::Claude,
            permission_mode: PermissionMode::default(),
            session_timeout_secs: 1800,
            roadmap_enabled: true,
            verify_enabled: true,
            balance_agents: true,
            live_streaming: true,
            notifications_enabled: true,
            isolate_worktrees: true,
            auto_commit: true,
            auto_pr: false,
            schedule_refresh_secs: 300,
            retry_enabled: true,
            retry_base_secs: 60,
            retry_max_secs: 3600,
            activity_retention: 2000,
            priority_aging_per_hour: 0.0,
            roadmap_max_pending: 0,
            roadmap_min_interval_mins: 0,
            quiet_hours_enabled: false,
            quiet_hours_start: 22,
            quiet_hours_end: 8,
            backup_enabled: false,
            backup_interval_hours: 24,
            backup_dir: String::new(),
            webhooks: Vec::new(),
            task_templates: Vec::new(),
            agents,
        }
    }
}

impl Settings {
    pub fn agent_config(&self, kind: AgentKind) -> AgentConfig {
        self.agents.get(kind.as_str()).cloned().unwrap_or_default()
    }
}

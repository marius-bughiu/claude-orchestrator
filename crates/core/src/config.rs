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
    pub extra_args: Vec<String>,
    /// Configured usage limits for the header display.
    pub limits: AgentLimits,
    /// Rolling usage window length in hours (Claude plans reset every 5h, hence 5).
    pub window_hours: u32,
    pub enabled: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            binary: None,
            model: None,
            extra_args: Vec::new(),
            limits: AgentLimits::default(),
            window_hours: 5,
            enabled: true,
        }
    }
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
    /// How often to re-scan projects for scheduled-task markdown files, seconds.
    pub schedule_refresh_secs: u64,
    /// Per-agent configuration, keyed by agent name.
    pub agents: BTreeMap<String, AgentConfig>,
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
            schedule_refresh_secs: 300,
            agents,
        }
    }
}

impl Settings {
    pub fn agent_config(&self, kind: AgentKind) -> AgentConfig {
        self.agents.get(kind.as_str()).cloned().unwrap_or_default()
    }
}

//! Agent adapters: each knows how to build a CLI invocation and how to parse that
//! CLI's streaming output into a normalized [`AgentEvent`] stream.
//!
//! Adapters are intentionally pure (no process spawning, no IO) so the
//! command-building and parsing logic can be unit-tested deterministically. The
//! actual spawning lives in [`crate::runner`].

mod claude;
mod codex;
mod gemini;

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub use gemini::GeminiAdapter;

use crate::config::PermissionMode;
use crate::models::{AgentKind, TokenUsage};
use std::path::PathBuf;

/// Everything needed to launch one agent invocation.
#[derive(Debug, Clone)]
pub struct RunSpec {
    /// The prompt / instruction text.
    pub prompt: String,
    /// Working directory (the project's git repo).
    pub cwd: PathBuf,
    /// Model id, if overriding the agent default.
    pub model: Option<String>,
    /// Text appended to the agent's system prompt.
    pub system_prompt_append: Option<String>,
    /// The agent's own prior session id to resume (continues a conversation).
    pub resume_session_id: Option<String>,
    /// A desired session id to assign up-front (Claude `--session-id`).
    pub session_id: Option<String>,
    pub permission_mode: PermissionMode,
    /// Additional directories the agent may access.
    pub add_dirs: Vec<PathBuf>,
    /// Path to an MCP config file, if any.
    pub mcp_config: Option<PathBuf>,
    /// Extra args appended verbatim.
    pub extra_args: Vec<String>,
}

impl RunSpec {
    pub fn new(prompt: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        RunSpec {
            prompt: prompt.into(),
            cwd: cwd.into(),
            model: None,
            system_prompt_append: None,
            resume_session_id: None,
            session_id: None,
            permission_mode: PermissionMode::default(),
            add_dirs: Vec::new(),
            mcp_config: None,
            extra_args: Vec::new(),
        }
    }
}

/// A concrete process invocation produced by an adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    /// Payload to write to the child's stdin, if the adapter uses stdin input.
    pub stdin: Option<String>,
}

/// A normalized streaming event, agent-agnostic.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// Session initialized; carries the agent's own session id for resuming.
    Init {
        agent_session_id: Option<String>,
        model: Option<String>,
    },
    /// Assistant produced visible text.
    Assistant { text: String },
    /// Extended-thinking / reasoning text.
    Thinking { text: String },
    /// The agent invoked a tool.
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    /// A tool returned a result.
    ToolResult { content: String, is_error: bool },
    /// Final result of the invocation with usage accounting.
    Result {
        success: bool,
        result_text: Option<String>,
        usage: TokenUsage,
    },
    /// A non-fatal error reported in-band.
    Error { message: String },
    /// Anything we didn't specifically model, preserved verbatim.
    Raw { value: serde_json::Value },
}

impl AgentEvent {
    /// Short kind string used for persistence and the UI.
    pub fn kind(&self) -> &'static str {
        match self {
            AgentEvent::Init { .. } => "init",
            AgentEvent::Assistant { .. } => "assistant",
            AgentEvent::Thinking { .. } => "thinking",
            AgentEvent::ToolUse { .. } => "tool_use",
            AgentEvent::ToolResult { .. } => "tool_result",
            AgentEvent::Result { .. } => "result",
            AgentEvent::Error { .. } => "error",
            AgentEvent::Raw { .. } => "raw",
        }
    }
}

/// The behavior every agent adapter implements.
pub trait AgentAdapter: Send + Sync {
    fn kind(&self) -> AgentKind;

    /// Default binary name (overridable via settings).
    fn default_binary(&self) -> &'static str;

    /// Build the concrete process invocation for `spec`.
    fn build_invocation(&self, spec: &RunSpec, binary: &str) -> Invocation;

    /// Parse a single line of stdout into zero or more normalized events.
    fn parse_line(&self, line: &str) -> Vec<AgentEvent>;
}

/// Construct the adapter for a given agent kind.
pub fn adapter_for(kind: AgentKind) -> Box<dyn AgentAdapter> {
    match kind {
        AgentKind::Claude => Box::new(ClaudeAdapter),
        AgentKind::Gemini => Box::new(GeminiAdapter),
        AgentKind::Codex => Box::new(CodexAdapter),
    }
}

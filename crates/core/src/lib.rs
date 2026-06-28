//! # orchestrator-core
//!
//! Platform-independent engine behind Claude Orchestrator. It owns the data
//! model, SQLite persistence, the agent CLI adapters (Claude / Gemini / Codex),
//! the process runner that streams their JSON output, and the scheduling engine
//! that allocates pending tasks to autonomous sessions, runs the roadmap loop,
//! and verifies finished work.
//!
//! It deliberately contains no GUI or Tauri dependency, so it can be unit-tested
//! in isolation and reused from any host.

pub mod agents;
pub mod config;
pub mod conventions;
pub mod db;
pub mod engine;
pub mod error;
pub mod event;
pub mod git;
pub mod github;
pub mod models;
pub mod parse;
pub mod runner;
pub mod scheduled;
pub mod service;
pub mod util;
pub mod webhook;
pub mod worktree;

pub use config::{AgentConfig, PermissionMode, Settings};
pub use db::Db;
pub use engine::Engine;
pub use error::{CoreError, Result};
pub use event::{EventSink, NullSink, OrchestratorEvent};
pub use models::*;

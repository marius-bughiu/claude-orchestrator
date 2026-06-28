//! Outbound events from the engine to whatever is driving the UI.
//!
//! The core stays GUI-agnostic: it emits [`OrchestratorEvent`] through an
//! [`EventSink`]. The Tauri layer implements `EventSink` by forwarding to the
//! webview; tests can implement it with a channel.

use crate::models::{Session, SessionEvent, Task};
use serde::Serialize;

/// A structured event the front-end can react to. Serialized as
/// `{ "type": "...", ... }` for the webview.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OrchestratorEvent {
    /// A new streamed event was persisted for a session.
    SessionEvent {
        session_id: String,
        project_id: String,
        task_id: Option<String>,
        event: SessionEvent,
    },
    /// A session's status or fields changed.
    SessionUpdated { session: Session },
    /// A task's status or fields changed.
    TaskUpdated { task: Task },
    /// Top-level orchestrator status changed; the UI should refetch status.
    StatusChanged,
    /// Usage totals changed; the UI should refresh the header.
    UsageUpdated,
    /// Free-form log line for the activity feed.
    Log { level: String, message: String },
}

/// Sink the engine emits events into. Implemented by the host (Tauri / tests).
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: OrchestratorEvent);
}

/// An `EventSink` that drops everything — useful for tests and headless runs.
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: OrchestratorEvent) {}
}

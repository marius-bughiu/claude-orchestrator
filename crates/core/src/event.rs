//! Outbound events from the engine to whatever is driving the UI.
//!
//! The core stays GUI-agnostic: it emits [`OrchestratorEvent`] through an
//! [`EventSink`]. The Tauri layer implements `EventSink` by forwarding to the
//! webview; tests can implement it with a channel.

use crate::models::{ActivityEntry, Session, SessionEvent, Task};
use serde::Serialize;

/// A structured event the front-end can react to. Serialized as
/// `{ "type": "...", ... }` for the webview.
#[derive(Debug, Clone, Serialize)]
// `rename_all` only camelCases the variant names (the `type` tag); without
// `rename_all_fields` the struct-variant fields stay snake_case (`session_id`),
// so the webview's `sessionId` filter never matches and live updates are dropped.
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum OrchestratorEvent {
    /// A new streamed event was persisted for a session.
    SessionEvent {
        session_id: String,
        project_id: String,
        task_id: Option<String>,
        event: SessionEvent,
    },
    /// An ephemeral token delta for the live view (not persisted). `kind` is
    /// "assistant" or "thinking".
    SessionDelta {
        session_id: String,
        kind: String,
        text: String,
    },
    /// A session's status or fields changed.
    SessionUpdated { session: Session },
    /// A task's status or fields changed.
    TaskUpdated { task: Task },
    /// Top-level orchestrator status changed; the UI should refetch status.
    StatusChanged,
    /// The set of scheduled tasks changed (discovery or a firing); refetch them.
    ScheduledChanged,
    /// Usage totals changed; the UI should refresh the header.
    UsageUpdated,
    /// A significant event was recorded in the persisted activity history.
    Activity { entry: ActivityEntry },
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

#[cfg(test)]
mod tests {
    use super::*;

    // The webview filters live events on camelCase `sessionId`; if the struct
    // variant fields regress to snake_case the live session view stops updating.
    #[test]
    fn session_event_fields_are_camel_case() {
        let ev = OrchestratorEvent::SessionDelta {
            session_id: "s1".into(),
            kind: "assistant".into(),
            text: "hi".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"sessionDelta\""), "{json}");
        assert!(json.contains("\"sessionId\":\"s1\""), "{json}");
        assert!(!json.contains("session_id"), "{json}");
    }
}

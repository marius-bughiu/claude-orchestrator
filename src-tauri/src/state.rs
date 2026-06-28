//! Application state and the Tauri-backed event sink.

use orchestrator_core::event::{EventSink, OrchestratorEvent};
use orchestrator_core::Engine;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// The single event channel the front-end subscribes to. Payload is an
/// [`OrchestratorEvent`] serialized as `{ "type": "...", ... }`.
pub const EVENT_CHANNEL: &str = "orchestrator://event";

/// Forwards engine events to the webview.
pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        TauriSink { app }
    }
}

impl EventSink for TauriSink {
    fn emit(&self, event: OrchestratorEvent) {
        // Best-effort: a failed emit (e.g. window closing) must not crash the engine.
        let _ = self.app.emit(EVENT_CHANNEL, event);
    }
}

/// Managed Tauri state.
pub struct AppState {
    pub engine: Arc<Engine>,
}

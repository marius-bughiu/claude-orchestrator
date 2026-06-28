//! Tauri host: constructs the engine, bridges its events to the webview, and
//! exposes the command surface.

mod commands;
mod state;

use orchestrator_core::{Db, Engine};
use state::{AppState, TauriSink};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;

/// Resolve a writable per-user data directory for the SQLite database.
fn data_dir(app: &tauri::AppHandle) -> PathBuf {
    // Prefer Tauri's resolved app-data dir; fall back to a platform default.
    if let Ok(dir) = app.path().app_data_dir() {
        return dir;
    }
    directories::ProjectDirs::from("com", "ClaudeOrchestrator", "ClaudeOrchestrator")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,orchestrator=debug".into()),
        )
        .init();

    // A multi-thread tokio runtime backs the engine's spawned session jobs. We
    // enter it for the app's lifetime so `tokio::spawn` works from setup and from
    // (synchronous) command handlers running on the main thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let _guard = runtime.enter();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let dir = data_dir(&handle);
            std::fs::create_dir_all(&dir).ok();
            let db = Db::open(dir.join("orchestrator.sqlite"))
                .expect("failed to open orchestrator database");
            let sink = Arc::new(TauriSink::new(handle.clone()));
            let engine = Engine::new(db, sink);
            engine.start();
            app.manage(AppState { engine });
            tracing::info!("Claude Orchestrator started; data dir: {}", dir.display());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::get_project,
            commands::add_project,
            commands::update_project,
            commands::remove_project,
            commands::scaffold_project,
            commands::project_conventions,
            commands::list_tasks,
            commands::get_task,
            commands::create_task,
            commands::update_task,
            commands::delete_task,
            commands::run_task_now,
            commands::list_sessions,
            commands::get_session,
            commands::get_session_events,
            commands::send_message,
            commands::stop_session,
            commands::get_status,
            commands::set_running,
            commands::get_settings,
            commands::update_settings,
            commands::trigger_roadmap,
            commands::get_timeline,
            commands::list_scheduled,
            commands::refresh_scheduled,
            commands::set_scheduled_enabled,
            commands::upcoming_tasks,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Claude Orchestrator");

    // Keep the runtime alive until the app exits.
    drop(_guard);
    drop(runtime);
}

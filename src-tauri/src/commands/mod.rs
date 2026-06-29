//! Tauri command handlers — the IPC surface the React front-end calls via
//! `invoke`. Each returns `Result<T, String>` so errors surface cleanly in JS.

use crate::state::AppState;
use orchestrator_core::config::{Settings, WebhookConfig};
use orchestrator_core::models::*;
use orchestrator_core::service::{
    self, AddProjectInput, BulkTaskInput, ConfigBundle, CreateTaskInput, ImportResult,
};
use orchestrator_core::{conventions, SessionEvent};
use tauri::State;

type CmdResult<T> = std::result::Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---- Projects --------------------------------------------------------------

#[tauri::command]
pub fn list_projects(state: State<AppState>) -> CmdResult<Vec<Project>> {
    state.engine.db().list_projects().map_err(err)
}

#[tauri::command]
pub fn get_project(state: State<AppState>, id: String) -> CmdResult<Project> {
    state.engine.db().get_project(&id).map_err(err)
}

#[tauri::command]
pub fn add_project(state: State<AppState>, input: AddProjectInput) -> CmdResult<Project> {
    let project = service::add_project(state.engine.db(), input).map_err(err)?;
    state.engine.request_tick();
    Ok(project)
}

#[tauri::command]
pub fn update_project(state: State<AppState>, project: Project) -> CmdResult<()> {
    let mut project = project;
    project.updated_at = chrono::Utc::now();
    state.engine.db().upsert_project(&project).map_err(err)?;
    state.engine.request_tick();
    Ok(())
}

#[tauri::command]
pub fn remove_project(state: State<AppState>, id: String) -> CmdResult<()> {
    state.engine.db().delete_project(&id).map_err(err)
}

#[tauri::command]
pub fn scaffold_project(state: State<AppState>, id: String) -> CmdResult<Vec<String>> {
    let project = state.engine.db().get_project(&id).map_err(err)?;
    conventions::scaffold(&project.path).map_err(err)
}

#[tauri::command]
pub fn project_conventions(state: State<AppState>, id: String) -> CmdResult<bool> {
    let project = state.engine.db().get_project(&id).map_err(err)?;
    Ok(conventions::is_initialized(&project.path))
}

#[tauri::command]
pub fn project_git_status(state: State<AppState>, id: String) -> CmdResult<GitStatus> {
    let project = state.engine.db().get_project(&id).map_err(err)?;
    Ok(orchestrator_core::git::status(&project.path))
}

// ---- Tasks -----------------------------------------------------------------

#[tauri::command]
pub fn list_tasks(state: State<AppState>, project_id: Option<String>) -> CmdResult<Vec<Task>> {
    state
        .engine
        .db()
        .list_tasks(project_id.as_deref())
        .map_err(err)
}

#[tauri::command]
pub fn get_task(state: State<AppState>, id: String) -> CmdResult<Task> {
    state.engine.db().get_task(&id).map_err(err)
}

#[tauri::command]
pub fn create_task(state: State<AppState>, input: CreateTaskInput) -> CmdResult<Task> {
    let task = service::create_task(state.engine.db(), input).map_err(err)?;
    state.engine.request_tick();
    Ok(task)
}

#[tauri::command]
pub fn update_task(state: State<AppState>, task: Task) -> CmdResult<()> {
    let mut task = task;
    // Reject self-deps, missing prerequisites, and dependency cycles.
    state.engine.validate_task_deps(&task).map_err(err)?;
    task.updated_at = chrono::Utc::now();
    state.engine.db().upsert_task(&task).map_err(err)?;
    state.engine.request_tick();
    Ok(())
}

#[tauri::command]
pub fn delete_task(state: State<AppState>, id: String) -> CmdResult<()> {
    state.engine.db().delete_task(&id).map_err(err)
}

/// Bulk-delete tasks in the given statuses (e.g. completed/cancelled), optionally
/// scoped to one project. Returns how many were removed.
#[tauri::command]
pub fn purge_tasks(
    state: State<AppState>,
    statuses: Vec<String>,
    project_id: Option<String>,
) -> CmdResult<u32> {
    let statuses: Vec<TaskStatus> = statuses.iter().map(|s| TaskStatus::from_str(s)).collect();
    state
        .engine
        .db()
        .purge_tasks(project_id.as_deref(), &statuses)
        .map_err(err)
}

/// Force a task back into the queue so the scheduler picks it up promptly.
#[tauri::command]
pub fn run_task_now(state: State<AppState>, id: String) -> CmdResult<()> {
    let mut task = state.engine.db().get_task(&id).map_err(err)?;
    task.status = TaskStatus::Pending;
    task.retry_at = None; // bypass any retry backoff
    task.updated_at = chrono::Utc::now();
    state.engine.db().upsert_task(&task).map_err(err)?;
    state.engine.request_tick();
    Ok(())
}

/// Reset a task to pending and clear its attempt counter so it runs again even
/// after exhausting its retries (used for failed/needs-review tasks).
#[tauri::command]
pub fn retry_task(state: State<AppState>, id: String) -> CmdResult<()> {
    let mut task = state.engine.db().get_task(&id).map_err(err)?;
    task.status = TaskStatus::Pending;
    task.attempts = 0;
    task.retry_at = None; // bypass any retry backoff
    task.updated_at = chrono::Utc::now();
    state.engine.db().upsert_task(&task).map_err(err)?;
    state.engine.request_tick();
    Ok(())
}

/// Create many tasks at once from a pasted markdown/checklist block.
#[tauri::command]
pub fn create_tasks_bulk(state: State<AppState>, input: BulkTaskInput) -> CmdResult<Vec<Task>> {
    let tasks = service::create_tasks_bulk(state.engine.db(), input).map_err(err)?;
    state.engine.request_tick();
    Ok(tasks)
}

/// Duplicate a task as a fresh pending task in the same project.
#[tauri::command]
pub fn clone_task(state: State<AppState>, id: String) -> CmdResult<Task> {
    let t = state.engine.db().get_task(&id).map_err(err)?;
    let input = CreateTaskInput {
        project_id: t.project_id,
        title: format!("{} (copy)", t.title),
        description: t.description,
        priority: Some(t.priority),
        agent: if t.auto_agent { None } else { Some(t.agent) },
        model: t.model,
        depends_on: vec![],
        tags: t.tags,
        max_attempts: Some(t.max_attempts),
    };
    let task = service::create_task(state.engine.db(), input).map_err(err)?;
    state.engine.request_tick();
    Ok(task)
}

// ---- Sessions --------------------------------------------------------------

#[tauri::command]
pub fn list_sessions(
    state: State<AppState>,
    task_id: Option<String>,
    project_id: Option<String>,
) -> CmdResult<Vec<Session>> {
    state
        .engine
        .db()
        .list_sessions(task_id.as_deref(), project_id.as_deref())
        .map_err(err)
}

#[tauri::command]
pub fn get_session(state: State<AppState>, id: String) -> CmdResult<Session> {
    state.engine.db().get_session(&id).map_err(err)
}

#[tauri::command]
pub fn get_session_events(state: State<AppState>, id: String) -> CmdResult<Vec<SessionEvent>> {
    state.engine.db().list_events(&id).map_err(err)
}

#[tauri::command]
pub fn send_message(
    state: State<AppState>,
    session_id: String,
    message: String,
    model: Option<String>,
) -> CmdResult<String> {
    state
        .engine
        .send_message(&session_id, &message, model.as_deref())
        .map_err(err)
}

/// Inject a message into a live session (or resume if it has finished). Returns
/// the session id the UI should display.
#[tauri::command]
pub fn inject_message(
    state: State<AppState>,
    session_id: String,
    message: String,
    model: Option<String>,
) -> CmdResult<String> {
    state
        .engine
        .inject_message(&session_id, &message, model.as_deref())
        .map_err(err)
}

#[tauri::command]
pub fn stop_session(state: State<AppState>, id: String) -> CmdResult<()> {
    state.engine.stop_session(&id).map_err(err)
}

// ---- Orchestrator ----------------------------------------------------------

#[tauri::command]
pub fn get_status(state: State<AppState>) -> CmdResult<OrchestratorStatus> {
    state.engine.status().map_err(err)
}

#[tauri::command]
pub fn set_running(state: State<AppState>, running: bool) -> CmdResult<()> {
    state.engine.set_running(running).map_err(err)
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> CmdResult<Settings> {
    state.engine.db().get_settings().map_err(err)
}

#[tauri::command]
pub fn update_settings(state: State<AppState>, settings: Settings) -> CmdResult<()> {
    state.engine.db().save_settings(&settings).map_err(err)?;
    state.engine.request_tick();
    Ok(())
}

#[tauri::command]
pub fn test_webhook(state: State<AppState>, config: WebhookConfig) -> CmdResult<()> {
    state.engine.test_webhook(&config).map_err(err)
}

#[tauri::command]
pub fn trigger_roadmap(state: State<AppState>, project_id: String) -> CmdResult<()> {
    state.engine.trigger_roadmap(&project_id).map_err(err)
}

#[tauri::command]
pub fn get_timeline(state: State<AppState>, limit: Option<u32>) -> CmdResult<Vec<TimelineItem>> {
    state
        .engine
        .db()
        .timeline(limit.unwrap_or(200))
        .map_err(err)
}

#[tauri::command]
pub fn get_activity(
    state: State<AppState>,
    limit: Option<u32>,
    project_id: Option<String>,
) -> CmdResult<Vec<ActivityEntry>> {
    state
        .engine
        .activity(limit.unwrap_or(200), project_id.as_deref())
        .map_err(err)
}

#[tauri::command]
pub fn task_rollup(state: State<AppState>, id: String) -> CmdResult<TaskRollup> {
    state.engine.task_rollup(&id).map_err(err)
}

#[tauri::command]
pub fn stuck_tasks(state: State<AppState>) -> CmdResult<Vec<StuckTask>> {
    state.engine.stuck_tasks().map_err(err)
}

// ---- Config import/export --------------------------------------------------

#[tauri::command]
pub fn export_config(state: State<AppState>) -> CmdResult<ConfigBundle> {
    service::export_config(state.engine.db()).map_err(err)
}

#[tauri::command]
pub fn import_config(state: State<AppState>, bundle: ConfigBundle) -> CmdResult<ImportResult> {
    let res = service::import_config(state.engine.db(), bundle).map_err(err)?;
    state.engine.request_tick();
    Ok(res)
}

#[tauri::command]
pub fn backup_config_now(state: State<AppState>) -> CmdResult<String> {
    state
        .engine
        .backup_now()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(err)
}

// ---- Scheduled tasks -------------------------------------------------------

#[tauri::command]
pub fn list_scheduled(
    state: State<AppState>,
    project_id: Option<String>,
) -> CmdResult<Vec<ScheduledTask>> {
    state
        .engine
        .db()
        .list_scheduled(project_id.as_deref())
        .map_err(err)
}

#[tauri::command]
pub fn refresh_scheduled(state: State<AppState>) -> CmdResult<u32> {
    state.engine.refresh_scheduled().map_err(err)
}

#[tauri::command]
pub fn set_scheduled_enabled(state: State<AppState>, id: String, enabled: bool) -> CmdResult<()> {
    state
        .engine
        .db()
        .set_scheduled_enabled(&id, enabled)
        .map_err(err)?;
    state.engine.request_tick();
    Ok(())
}

#[tauri::command]
pub fn upcoming_tasks(
    state: State<AppState>,
    project_id: Option<String>,
    limit: Option<u32>,
) -> CmdResult<Vec<UpcomingTask>> {
    state
        .engine
        .upcoming(project_id.as_deref(), limit.unwrap_or(10) as usize)
        .map_err(err)
}

// ---- Dashboards ------------------------------------------------------------

#[tauri::command]
pub fn usage_series(
    state: State<AppState>,
    granularity: String,
    agent: Option<AgentKind>,
    limit: Option<u32>,
) -> CmdResult<Vec<UsagePoint>> {
    state
        .engine
        .db()
        .usage_series(&granularity, agent, limit.unwrap_or(30))
        .map_err(err)
}

#[tauri::command]
pub fn agent_stats(state: State<AppState>) -> CmdResult<Vec<AgentStat>> {
    state.engine.db().agent_stats().map_err(err)
}

// ---- Project memory --------------------------------------------------------

#[tauri::command]
pub fn project_memory(state: State<AppState>, id: String) -> CmdResult<ProjectMemory> {
    state.engine.project_memory(&id).map_err(err)
}

#[tauri::command]
pub fn generate_project_context(state: State<AppState>, id: String) -> CmdResult<String> {
    state.engine.generate_project_context(&id).map_err(err)
}

// ---- GitHub ----------------------------------------------------------------

#[tauri::command]
pub fn import_github_issues(state: State<AppState>, project_id: String) -> CmdResult<u32> {
    state.engine.import_github_issues(&project_id).map_err(err)
}

#[tauri::command]
pub fn list_pull_requests(
    state: State<AppState>,
    project_id: String,
) -> CmdResult<Vec<PullRequest>> {
    state.engine.list_pull_requests(&project_id).map_err(err)
}

#[tauri::command]
pub fn merge_pull_request(
    state: State<AppState>,
    project_id: String,
    number: u64,
) -> CmdResult<()> {
    state
        .engine
        .merge_pull_request(&project_id, number)
        .map_err(err)
}

// ---- Diffs -----------------------------------------------------------------

#[tauri::command]
pub fn session_diff(state: State<AppState>, id: String) -> CmdResult<SessionDiff> {
    state.engine.session_diff(&id).map_err(err)
}

// ---- Agent health & maintenance --------------------------------------------

#[tauri::command]
pub fn agent_health(state: State<AppState>) -> CmdResult<Vec<AgentHealth>> {
    state.engine.agent_health().map_err(err)
}

#[tauri::command]
pub fn diagnostics(state: State<AppState>) -> CmdResult<Vec<Diagnostic>> {
    state.engine.diagnostics().map_err(err)
}

#[tauri::command]
pub fn search_sessions(
    state: State<AppState>,
    query: String,
    project_id: Option<String>,
) -> CmdResult<Vec<SessionMatch>> {
    state
        .engine
        .search_sessions(&query, project_id.as_deref())
        .map_err(err)
}

#[tauri::command]
pub fn upcoming_queue(state: State<AppState>, limit: usize) -> CmdResult<Vec<QueuedTask>> {
    state.engine.upcoming_queue(limit).map_err(err)
}

#[tauri::command]
pub fn session_throughput(state: State<AppState>, days: u32) -> CmdResult<Vec<ThroughputPoint>> {
    state.engine.session_throughput(days).map_err(err)
}

#[tauri::command]
pub fn export_task_transcript(state: State<AppState>, task_id: String) -> CmdResult<String> {
    state.engine.export_task_transcript(&task_id).map_err(err)
}

#[tauri::command]
pub fn export_project_transcript(state: State<AppState>, project_id: String) -> CmdResult<String> {
    state
        .engine
        .export_project_transcript(&project_id)
        .map_err(err)
}

#[tauri::command]
pub fn list_branches(state: State<AppState>, project_id: String) -> CmdResult<Vec<BranchInfo>> {
    state.engine.list_branches(&project_id).map_err(err)
}

#[tauri::command]
pub fn delete_branch(state: State<AppState>, project_id: String, branch: String) -> CmdResult<()> {
    state
        .engine
        .delete_branch(&project_id, &branch)
        .map_err(err)
}

#[tauri::command]
pub fn prune_worktrees(state: State<AppState>, project_id: String) -> CmdResult<()> {
    state.engine.prune_worktrees(&project_id).map_err(err)
}

#[tauri::command]
pub fn rebase_branch(
    state: State<AppState>,
    project_id: String,
    branch: String,
) -> CmdResult<RebaseResult> {
    state
        .engine
        .rebase_branch(&project_id, &branch)
        .map_err(err)
}

// ---- Updates ---------------------------------------------------------------

/// Begin draining for an update: stop scheduling new work. The UI then polls
/// status until `activeSessions` is zero before installing the update.
#[tauri::command]
pub fn begin_drain(state: State<AppState>) -> CmdResult<()> {
    state.engine.begin_drain();
    Ok(())
}

#[tauri::command]
pub fn cancel_drain(state: State<AppState>) -> CmdResult<()> {
    state.engine.cancel_drain();
    Ok(())
}

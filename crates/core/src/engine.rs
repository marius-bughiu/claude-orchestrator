//! The orchestration engine: the scheduling loop that allocates pending tasks to
//! agent sessions, runs the roadmap loop when a project's queue empties, and
//! verifies finished work.

use crate::agents::{self, RunSpec};
use crate::config::Settings;
use crate::conventions;
use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::event::{EventSink, OrchestratorEvent};
use crate::models::*;
use crate::parse;
use crate::runner::{self, CancelToken, RunOutcome};
use crate::util;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use uuid::Uuid;

/// The orchestrator engine. Cheap to clone via `Arc`.
pub struct Engine {
    db: Db,
    sink: Arc<dyn EventSink>,
    /// session id -> cancel token for currently running sessions.
    running: Arc<Mutex<HashMap<String, CancelToken>>>,
    wake: Arc<Notify>,
}

impl Engine {
    pub fn new(db: Db, sink: Arc<dyn EventSink>) -> Arc<Engine> {
        Arc::new(Engine {
            db,
            sink,
            running: Arc::new(Mutex::new(HashMap::new())),
            wake: Arc::new(Notify::new()),
        })
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Nudge the scheduler to run a tick immediately.
    pub fn request_tick(&self) {
        self.wake.notify_one();
    }

    fn emit(&self, event: OrchestratorEvent) {
        self.sink.emit(event);
    }

    fn log(&self, level: &str, message: impl Into<String>) {
        let message = message.into();
        tracing::debug!(target: "orchestrator", "{level}: {message}");
        self.emit(OrchestratorEvent::Log {
            level: level.to_string(),
            message,
        });
    }

    /// Launch the background scheduling loop. Returns immediately.
    pub fn start(self: &Arc<Self>) {
        let engine = self.clone();
        tokio::spawn(async move {
            let recovered = engine.db.reconcile_orphan_sessions().unwrap_or(0);
            if recovered > 0 {
                engine.log("warn", format!("recovered {recovered} orphaned session(s)"));
            }
            loop {
                if let Err(e) = engine.tick().await {
                    engine.log("error", format!("scheduler tick failed: {e}"));
                }
                let interval = engine
                    .db
                    .get_settings()
                    .map(|s| s.tick_interval_secs)
                    .unwrap_or(10)
                    .max(1);
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
                    _ = engine.wake.notified() => {}
                }
            }
        });
    }

    // ---- Status -------------------------------------------------------------

    pub fn status(&self) -> Result<OrchestratorStatus> {
        let settings = self.db.get_settings()?;
        Ok(OrchestratorStatus {
            running: settings.running,
            active_sessions: self.db.count_active_sessions()?,
            max_concurrent: settings.max_concurrent,
            pending_tasks: self.db.count_pending_tasks()?,
            projects: self.db.count_projects()?,
            agents: self.agent_usage(&settings)?,
        })
    }

    fn agent_usage(&self, settings: &Settings) -> Result<Vec<AgentUsage>> {
        let now = Utc::now();
        let mut out = Vec::new();
        for kind in AgentKind::ALL {
            let cfg = settings.agent_config(kind);
            let window_hours = cfg.window_hours.max(1);
            let window_start = now - chrono::Duration::hours(window_hours as i64);
            let binary = cfg
                .binary
                .clone()
                .unwrap_or_else(|| agents::adapter_for(kind).default_binary().to_string());
            out.push(AgentUsage {
                agent: kind,
                available: util::binary_available(&binary),
                window: self.db.usage_for_agent(kind, Some(window_start))?,
                total: self.db.usage_for_agent(kind, None)?,
                active_sessions: self.db.count_active_sessions_for_agent(kind)?,
                limits: cfg.limits.clone(),
                window_started_at: window_start,
                window_hours,
            });
        }
        Ok(out)
    }

    // ---- Controls -----------------------------------------------------------

    pub fn set_running(&self, running: bool) -> Result<()> {
        let mut settings = self.db.get_settings()?;
        settings.running = running;
        self.db.save_settings(&settings)?;
        self.emit(OrchestratorEvent::StatusChanged);
        self.request_tick();
        Ok(())
    }

    pub fn stop_session(&self, session_id: &str) -> Result<()> {
        if let Some(token) = self.running.lock().unwrap().get(session_id).cloned() {
            token.cancel();
            Ok(())
        } else {
            Err(CoreError::NotFound(format!("running session {session_id}")))
        }
    }

    /// Manually enqueue a roadmap session for a project (ignores queue depth).
    pub fn trigger_roadmap(self: &Arc<Self>, project_id: &str) -> Result<()> {
        let project = self.db.get_project(project_id)?;
        self.spawn_roadmap_session(&project)?;
        Ok(())
    }

    /// Continue a session's conversation with a new user message. Spawns a
    /// follow-up session resuming the agent's prior context.
    pub fn send_message(self: &Arc<Self>, session_id: &str, message: &str) -> Result<String> {
        let prior = self.db.get_session(session_id)?;
        let project = self.db.get_project(&prior.project_id)?;
        let new_id = Uuid::new_v4().to_string();
        let session = Session {
            id: new_id.clone(),
            task_id: prior.task_id.clone(),
            project_id: prior.project_id.clone(),
            agent: prior.agent,
            kind: SessionKind::Task,
            status: SessionStatus::Pending,
            agent_session_id: None,
            model: prior.model.clone(),
            prompt: message.to_string(),
            result_text: None,
            error: None,
            exit_code: None,
            usage: TokenUsage::default(),
            started_at: None,
            ended_at: None,
            created_at: Utc::now(),
        };
        self.db.upsert_session(&session)?;
        let mut spec = self.run_spec(&project, prior.agent, message);
        spec.resume_session_id = prior.agent_session_id.clone();
        self.clone().spawn_session_job(session, spec, None);
        Ok(new_id)
    }

    // ---- Scheduling ---------------------------------------------------------

    async fn tick(self: &Arc<Self>) -> Result<()> {
        let settings = self.db.get_settings()?;
        if !settings.running {
            return Ok(());
        }
        let global_max = settings.max_concurrent.max(1);
        let mut active = self.db.count_active_sessions()?;
        if active >= global_max {
            return Ok(());
        }

        let active_sessions = self.db.active_sessions()?;
        let projects = self.db.list_projects()?;

        for project in projects.iter().filter(|p| p.enabled) {
            let proj_max = project.max_concurrent.unwrap_or(global_max).max(1);
            loop {
                if active >= global_max {
                    return Ok(());
                }
                let proj_active = self.db.count_active_sessions_for_project(&project.id)?;
                if proj_active >= proj_max {
                    break;
                }

                match self.pick_task(&project.id)? {
                    Some(task) => {
                        self.start_task(project, task)?;
                        active += 1;
                    }
                    None => {
                        // Queue is empty: run the roadmap loop if enabled and not
                        // already running for this project.
                        let roadmap_active = active_sessions.iter().any(|s| {
                            s.project_id == project.id && s.kind == SessionKind::Roadmap
                        });
                        if settings.roadmap_enabled
                            && project.roadmap_enabled
                            && !roadmap_active
                            && !self.has_running_roadmap(&project.id)?
                        {
                            self.spawn_roadmap_session(project)?;
                            active += 1;
                        }
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn has_running_roadmap(&self, project_id: &str) -> Result<bool> {
        Ok(self
            .db
            .active_sessions()?
            .iter()
            .any(|s| s.project_id == project_id && s.kind == SessionKind::Roadmap))
    }

    /// Pick the highest-priority schedulable task whose dependencies are met and
    /// whose attempts are not exhausted.
    fn pick_task(&self, project_id: &str) -> Result<Option<Task>> {
        let candidates = self.db.schedulable_tasks(project_id)?;
        for task in candidates {
            if task.attempts >= task.max_attempts {
                continue;
            }
            if self.deps_satisfied(&task)? {
                return Ok(Some(task));
            }
        }
        Ok(None)
    }

    fn deps_satisfied(&self, task: &Task) -> Result<bool> {
        for dep in &task.depends_on {
            match self.db.get_task(dep) {
                Ok(d) if d.status == TaskStatus::Completed => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    /// Synchronously mark a task as running and spawn its execution job.
    fn start_task(self: &Arc<Self>, project: &Project, mut task: Task) -> Result<()> {
        task.status = TaskStatus::Running;
        task.updated_at = Utc::now();
        self.db.upsert_task(&task)?;
        self.emit(OrchestratorEvent::TaskUpdated { task: task.clone() });

        let prompt = self.task_prompt(project, &task);
        let session = self.new_session(project, Some(&task), task.agent, SessionKind::Task, &prompt);
        self.db.upsert_session(&session)?;

        let spec = self.run_spec(project, task.agent, &prompt);
        self.clone()
            .spawn_session_job(session, spec, Some(task));
        Ok(())
    }

    fn spawn_roadmap_session(self: &Arc<Self>, project: &Project) -> Result<()> {
        let prompt = conventions::roadmap_prompt(&project.path);
        let agent = project.default_agent;
        let session = self.new_session(project, None, agent, SessionKind::Roadmap, &prompt);
        self.db.upsert_session(&session)?;
        self.log("info", format!("roadmap loop starting for {}", project.name));
        let spec = self.run_spec(project, agent, &prompt);
        self.clone().spawn_session_job(session, spec, None);
        Ok(())
    }

    // ---- Session execution --------------------------------------------------

    fn new_session(
        &self,
        project: &Project,
        task: Option<&Task>,
        agent: AgentKind,
        kind: SessionKind,
        prompt: &str,
    ) -> Session {
        Session {
            id: Uuid::new_v4().to_string(),
            task_id: task.map(|t| t.id.clone()),
            project_id: project.id.clone(),
            agent,
            kind,
            status: SessionStatus::Pending,
            agent_session_id: None,
            model: None,
            prompt: prompt.to_string(),
            result_text: None,
            error: None,
            exit_code: None,
            usage: TokenUsage::default(),
            started_at: None,
            ended_at: None,
            created_at: Utc::now(),
        }
    }

    fn run_spec(&self, project: &Project, agent: AgentKind, prompt: &str) -> RunSpec {
        let settings = self.db.get_settings().unwrap_or_default();
        let cfg = settings.agent_config(agent);
        let mut spec = RunSpec::new(prompt, &project.path);
        spec.model = cfg.model.clone();
        spec.permission_mode = settings.permission_mode;
        spec.extra_args = cfg.extra_args.clone();
        spec
    }

    fn task_prompt(&self, project: &Project, task: &Task) -> String {
        let mut prompt = String::new();
        if let Some(preamble) = conventions::task_preamble(&project.path) {
            prompt.push_str(preamble.trim_end());
            prompt.push_str("\n\n---\n\n");
        }
        prompt.push_str(&format!("# Task: {}\n\n", task.title));
        prompt.push_str(&task.description);
        prompt
    }

    /// Spawn the async job that runs a session to completion, persists/streams
    /// its events, records usage, and (for task sessions) verifies the result.
    fn spawn_session_job(self: Arc<Self>, session: Session, spec: RunSpec, task: Option<Task>) {
        tokio::spawn(async move {
            let outcome = self.run_session(&session, &spec).await;
            match outcome {
                Ok(outcome) => {
                    if let Some(task) = task {
                        if let Err(e) = self.handle_task_outcome(task, &session, &outcome).await {
                            self.log("error", format!("post-task handling failed: {e}"));
                        }
                    } else if session.kind == SessionKind::Roadmap {
                        if let Err(e) = self.handle_roadmap_outcome(&session, &outcome) {
                            self.log("error", format!("roadmap import failed: {e}"));
                        }
                    }
                }
                Err(e) => {
                    self.log("error", format!("session {} failed to run: {e}", session.id));
                    self.finalize_session_error(&session, &e.to_string());
                }
            }
            self.emit(OrchestratorEvent::StatusChanged);
            self.emit(OrchestratorEvent::UsageUpdated);
            self.request_tick();
        });
    }

    /// Drive a single agent invocation: stream + persist events, finalize the
    /// session row, and record usage. Returns the run outcome.
    async fn run_session(&self, session: &Session, spec: &RunSpec) -> Result<RunOutcome> {
        let cancel = CancelToken::new();
        self.running
            .lock()
            .unwrap()
            .insert(session.id.clone(), cancel.clone());

        // Mark running.
        let mut row = session.clone();
        row.status = SessionStatus::Running;
        row.started_at = Some(Utc::now());
        self.db.upsert_session(&row)?;
        self.emit(OrchestratorEvent::SessionUpdated { session: row.clone() });
        self.emit(OrchestratorEvent::StatusChanged);

        let settings = self.db.get_settings().unwrap_or_default();
        let cfg = settings.agent_config(session.agent);
        let adapter = agents::adapter_for(session.agent);
        let binary = cfg
            .binary
            .clone()
            .unwrap_or_else(|| adapter.default_binary().to_string());
        let timeout = match settings.session_timeout_secs {
            0 => None,
            secs => Some(Duration::from_secs(secs)),
        };

        let db = self.db.clone();
        let sink = self.sink.clone();
        let sid = session.id.clone();
        let project_id = session.project_id.clone();
        let task_id = session.task_id.clone();

        let result = runner::run_agent(
            adapter.as_ref(),
            &binary,
            spec,
            cancel,
            timeout,
            |event| {
                let (text, data) = describe_event(event);
                if let Ok(stored) =
                    db.insert_event(&sid, event.kind(), text.as_deref(), data.as_ref(), Utc::now())
                {
                    sink.emit(OrchestratorEvent::SessionEvent {
                        session_id: sid.clone(),
                        project_id: project_id.clone(),
                        task_id: task_id.clone(),
                        event: stored,
                    });
                }
            },
        )
        .await;

        self.running.lock().unwrap().remove(&session.id);

        let outcome = result?;
        self.finalize_session(session, &outcome)?;
        Ok(outcome)
    }

    fn finalize_session(&self, session: &Session, outcome: &RunOutcome) -> Result<()> {
        let mut row = self.db.get_session(&session.id)?;
        row.status = if outcome.cancelled {
            SessionStatus::Cancelled
        } else if outcome.timed_out {
            SessionStatus::TimedOut
        } else if outcome.success {
            SessionStatus::Completed
        } else {
            SessionStatus::Failed
        };
        row.agent_session_id = outcome.agent_session_id.clone().or(row.agent_session_id);
        row.model = outcome.model.clone().or(row.model);
        row.result_text = outcome.result_text.clone();
        row.error = outcome.error.clone();
        row.exit_code = outcome.exit_code;
        row.usage = outcome.usage.clone();
        row.ended_at = Some(Utc::now());
        self.db.upsert_session(&row)?;
        self.emit(OrchestratorEvent::SessionUpdated { session: row.clone() });

        // Record usage for the agent.
        if outcome.usage != TokenUsage::default() {
            self.db.insert_usage(
                &Uuid::new_v4().to_string(),
                session.agent,
                Some(&session.id),
                &outcome.usage,
                Utc::now(),
            )?;
        }
        Ok(())
    }

    fn finalize_session_error(&self, session: &Session, error: &str) {
        if let Ok(mut row) = self.db.get_session(&session.id) {
            row.status = SessionStatus::Failed;
            row.error = Some(error.to_string());
            row.ended_at = Some(Utc::now());
            let _ = self.db.upsert_session(&row);
            self.emit(OrchestratorEvent::SessionUpdated { session: row });
        }
    }

    // ---- Outcome handling ---------------------------------------------------

    async fn handle_task_outcome(
        self: &Arc<Self>,
        mut task: Task,
        session: &Session,
        outcome: &RunOutcome,
    ) -> Result<()> {
        task.attempts += 1;
        task.updated_at = Utc::now();

        if outcome.cancelled {
            task.status = TaskStatus::Pending;
            task.attempts = task.attempts.saturating_sub(1);
            self.db.upsert_task(&task)?;
            self.emit(OrchestratorEvent::TaskUpdated { task });
            return Ok(());
        }

        if !outcome.success {
            task.status = if task.attempts >= task.max_attempts {
                TaskStatus::Failed
            } else {
                TaskStatus::NeedsReview
            };
            if let Some(err) = &outcome.error {
                task.description = append_feedback(&task.description, &format!("Previous attempt failed: {err}"));
            }
            self.db.upsert_task(&task)?;
            self.emit(OrchestratorEvent::TaskUpdated { task });
            return Ok(());
        }

        // Successful run. Verify if enabled.
        let settings = self.db.get_settings()?;
        let project = self.db.get_project(&task.project_id)?;
        if !(settings.verify_enabled && project.verify_enabled) {
            task.status = TaskStatus::Completed;
            self.db.upsert_task(&task)?;
            self.emit(OrchestratorEvent::TaskUpdated { task });
            return Ok(());
        }

        let verdict = self.run_verification(&project, &task, outcome).await?;
        match verdict {
            Some(v) if v.complete => {
                task.status = TaskStatus::Completed;
            }
            Some(v) => {
                let feedback = if v.follow_up.trim().is_empty() {
                    v.reason
                } else {
                    v.follow_up
                };
                task.description = append_feedback(&task.description, &feedback);
                task.status = if task.attempts >= task.max_attempts {
                    TaskStatus::Failed
                } else {
                    TaskStatus::NeedsReview
                };
            }
            None => {
                // No parseable verdict: trust the executor but flag it.
                self.log(
                    "warn",
                    format!("verifier produced no verdict for task {}; marking complete", task.id),
                );
                task.status = TaskStatus::Completed;
            }
        }
        let _ = session;
        self.db.upsert_task(&task)?;
        self.emit(OrchestratorEvent::TaskUpdated { task });
        Ok(())
    }

    async fn run_verification(
        &self,
        project: &Project,
        task: &Task,
        task_outcome: &RunOutcome,
    ) -> Result<Option<parse::VerifyVerdict>> {
        let base = conventions::verify_prompt(&project.path);
        let result_text = task_outcome.result_text.as_deref().unwrap_or("(no result text)");
        let prompt = format!(
            "{base}\n\n---\n## Task under review\nTitle: {}\n\nAcceptance criteria / description:\n{}\n\n## Executing session result\n{}\n",
            task.title, task.description, result_text
        );
        let session = self.new_session(project, Some(task), project.default_agent, SessionKind::Verify, &prompt);
        self.db.upsert_session(&session)?;
        let spec = self.run_spec(project, project.default_agent, &prompt);
        let outcome = self.run_session(&session, &spec).await?;
        Ok(outcome
            .result_text
            .as_deref()
            .and_then(parse::parse_verdict))
    }

    fn handle_roadmap_outcome(&self, session: &Session, outcome: &RunOutcome) -> Result<()> {
        let Some(text) = &outcome.result_text else {
            self.log("info", "roadmap produced no output");
            return Ok(());
        };
        let specs = parse::parse_roadmap_tasks(text);
        if specs.is_empty() {
            self.log("info", "roadmap generated no new tasks");
            return Ok(());
        }
        let project = self.db.get_project(&session.project_id)?;
        let now = Utc::now();
        let mut created = 0;
        for spec in specs {
            let task = Task {
                id: Uuid::new_v4().to_string(),
                project_id: project.id.clone(),
                title: spec.title.clone(),
                description: spec.description.clone(),
                status: TaskStatus::Pending,
                priority: spec.priority_or_default(),
                agent: spec.agent_kind(project.default_agent),
                parent_id: None,
                depends_on: vec![],
                attempts: 0,
                max_attempts: 3,
                tags: spec.tags.clone(),
                auto_generated: true,
                created_at: now,
                updated_at: now,
            };
            self.db.upsert_task(&task)?;
            self.emit(OrchestratorEvent::TaskUpdated { task });
            created += 1;
        }
        self.log("info", format!("roadmap created {created} task(s) for {}", project.name));
        Ok(())
    }
}

/// Append reviewer/error feedback as a bounded, clearly-delimited section.
fn append_feedback(description: &str, feedback: &str) -> String {
    format!(
        "{description}\n\n---\n## Reviewer feedback (address this)\n{}",
        feedback.trim()
    )
}

/// Produce a human-readable text + structured payload for persisting an event.
fn describe_event(event: &crate::agents::AgentEvent) -> (Option<String>, Option<serde_json::Value>) {
    use crate::agents::AgentEvent::*;
    match event {
        Init { agent_session_id, model } => (
            Some(format!(
                "session initialized ({})",
                model.clone().unwrap_or_else(|| "model unknown".into())
            )),
            Some(serde_json::json!({ "agentSessionId": agent_session_id, "model": model })),
        ),
        Assistant { text } => (Some(text.clone()), None),
        Thinking { text } => (Some(text.clone()), None),
        ToolUse { name, input } => (
            Some(name.clone()),
            Some(serde_json::json!({ "name": name, "input": input })),
        ),
        ToolResult { content, is_error } => (
            Some(content.clone()),
            Some(serde_json::json!({ "isError": is_error })),
        ),
        Result { success, result_text, usage } => (
            result_text.clone(),
            Some(serde_json::json!({ "success": success, "usage": usage })),
        ),
        Error { message } => (Some(message.clone()), None),
        Raw { value } => (None, Some(value.clone())),
    }
}

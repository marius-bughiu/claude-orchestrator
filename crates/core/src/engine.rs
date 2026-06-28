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
use crate::scheduled;
use crate::util;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::Path;
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
    /// session id -> stdin sender for live sessions accepting injected messages.
    inputs: Arc<Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<String>>>>,
    wake: Arc<Notify>,
    /// When set, the scheduler stops allocating new work (draining for an update).
    draining: Arc<std::sync::atomic::AtomicBool>,
}

impl Engine {
    pub fn new(db: Db, sink: Arc<dyn EventSink>) -> Arc<Engine> {
        Arc::new(Engine {
            db,
            sink,
            running: Arc::new(Mutex::new(HashMap::new())),
            inputs: Arc::new(Mutex::new(HashMap::new())),
            wake: Arc::new(Notify::new()),
            draining: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    pub fn is_draining(&self) -> bool {
        self.draining.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Begin draining for an update: stop scheduling new work. Existing sessions
    /// run to completion. The caller polls `status().active_sessions` until zero.
    pub fn begin_drain(&self) {
        self.draining
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.log("info", "draining: no new work will be scheduled");
        self.emit(OrchestratorEvent::StatusChanged);
    }

    /// Cancel a drain (e.g. the user dismissed the update).
    pub fn cancel_drain(&self) {
        self.draining
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.emit(OrchestratorEvent::StatusChanged);
        self.request_tick();
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

        // Scheduled-task discovery loop: scan on launch, then on an interval.
        let discoverer = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = discoverer.discover_all_scheduled() {
                    discoverer.log("error", format!("scheduled discovery failed: {e}"));
                }
                discoverer.request_tick();
                let secs = discoverer
                    .db
                    .get_settings()
                    .map(|s| s.schedule_refresh_secs)
                    .unwrap_or(300)
                    .max(30);
                tokio::time::sleep(Duration::from_secs(secs)).await;
            }
        });
    }

    // ---- Status -------------------------------------------------------------

    pub fn status(&self) -> Result<OrchestratorStatus> {
        let settings = self.db.get_settings()?;
        Ok(OrchestratorStatus {
            running: settings.running,
            draining: self.is_draining(),
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
            let binary = cfg
                .binary
                .clone()
                .unwrap_or_else(|| agents::adapter_for(kind).default_binary().to_string());
            let session = self.window_usage(
                kind,
                cfg.session_window_hours,
                cfg.limits.session_cost_usd,
                cfg.limits.session_token_limit,
                now,
            )?;
            let weekly = self.window_usage(
                kind,
                cfg.weekly_window_hours,
                cfg.limits.weekly_cost_usd,
                cfg.limits.weekly_token_limit,
                now,
            )?;
            out.push(AgentUsage {
                agent: kind,
                available: util::binary_available(&binary),
                active_sessions: self.db.count_active_sessions_for_agent(kind)?,
                session,
                weekly,
                total: self.db.usage_for_agent(kind, None)?,
            });
        }
        Ok(out)
    }

    fn window_usage(
        &self,
        agent: AgentKind,
        hours: u32,
        cost_limit: Option<f64>,
        token_limit: Option<u64>,
        now: DateTime<Utc>,
    ) -> Result<WindowUsage> {
        let hours = hours.max(1);
        let start = now - chrono::Duration::hours(hours as i64);
        let usage = self.db.usage_for_agent(agent, Some(start))?;
        let tokens = usage.input_tokens
            + usage.output_tokens
            + usage.cache_read_tokens
            + usage.cache_creation_tokens;
        let cost_pct = cost_limit.map(|l| {
            if l > 0.0 {
                usage.total_cost_usd / l
            } else {
                0.0
            }
        });
        let token_pct = token_limit.map(|l| if l > 0 { tokens as f64 / l as f64 } else { 0.0 });
        Ok(WindowUsage {
            usage,
            window_hours: hours,
            window_started_at: Some(start),
            cost_limit_usd: cost_limit,
            token_limit,
            cost_pct,
            token_pct,
        })
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
    pub fn send_message(
        self: &Arc<Self>,
        session_id: &str,
        message: &str,
        model: Option<&str>,
    ) -> Result<String> {
        let prior = self.db.get_session(session_id)?;
        let project = self.db.get_project(&prior.project_id)?;
        // A new model can be selected for the continued session; else reuse prior.
        let model = model
            .filter(|m| !m.trim().is_empty())
            .map(String::from)
            .or_else(|| prior.model.clone());
        let new_id = Uuid::new_v4().to_string();
        let session = Session {
            id: new_id.clone(),
            task_id: prior.task_id.clone(),
            project_id: prior.project_id.clone(),
            agent: prior.agent,
            kind: SessionKind::Task,
            status: SessionStatus::Pending,
            agent_session_id: None,
            model: model.clone(),
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
        let mut spec = self.run_spec(&project, prior.agent, message, model.as_deref());
        spec.resume_session_id = prior.agent_session_id.clone();
        self.clone().spawn_session_job(session, spec, None, false);
        Ok(new_id)
    }

    /// Send a message to a session. If the session is live (running with an open
    /// input channel), the message is injected mid-run and the same session id is
    /// returned. Otherwise the conversation is resumed in a new session whose id
    /// is returned. The UI navigates to the returned id.
    pub fn inject_message(
        self: &Arc<Self>,
        session_id: &str,
        message: &str,
        model: Option<&str>,
    ) -> Result<String> {
        let sender = self.inputs.lock().unwrap().get(session_id).cloned();
        if let Some(tx) = sender {
            if tx.send(message.to_string()).is_ok() {
                // Record the injected user message so it shows in the transcript.
                if let Ok(stored) = self.db.insert_event(
                    session_id,
                    "user_message",
                    Some(message),
                    None,
                    Utc::now(),
                ) {
                    if let Ok(s) = self.db.get_session(session_id) {
                        self.emit(OrchestratorEvent::SessionEvent {
                            session_id: session_id.to_string(),
                            project_id: s.project_id,
                            task_id: s.task_id,
                            event: stored,
                        });
                    }
                }
                return Ok(session_id.to_string());
            }
        }
        // Session has finished: resume as a new follow-up session.
        self.send_message(session_id, message, model)
    }

    // ---- Scheduling ---------------------------------------------------------

    async fn tick(self: &Arc<Self>) -> Result<()> {
        let settings = self.db.get_settings()?;
        if !settings.running || self.is_draining() {
            return Ok(());
        }

        // Fire any scheduled jobs that are due (creates pending tasks).
        if let Err(e) = self.fire_due_scheduled() {
            self.log("error", format!("scheduled firing failed: {e}"));
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
                        let roadmap_active = active_sessions
                            .iter()
                            .any(|s| s.project_id == project.id && s.kind == SessionKind::Roadmap);
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

    // ---- Scheduled tasks ----------------------------------------------------

    /// Re-scan every known project for scheduled-task markdown files and sync the
    /// database. Returns the total number of scheduled tasks discovered.
    pub fn discover_all_scheduled(&self) -> Result<u32> {
        let projects = self.db.list_projects()?;
        let mut total = 0usize;
        for project in projects {
            match self.discover_scheduled_for_project(&project) {
                Ok(n) => total += n,
                Err(e) => self.log(
                    "warn",
                    format!("scheduled discovery failed for {}: {e}", project.name),
                ),
            }
        }
        self.emit(OrchestratorEvent::ScheduledChanged);
        Ok(total as u32)
    }

    /// Manually refresh scheduled tasks and immediately evaluate firing.
    pub fn refresh_scheduled(self: &Arc<Self>) -> Result<u32> {
        let n = self.discover_all_scheduled()?;
        self.request_tick();
        Ok(n)
    }

    /// The soonest `limit` future firings projected from enabled, valid scheduled
    /// tasks. Optionally scoped to one project. A frequent job contributes several
    /// occurrences; results are merged and sorted by time.
    pub fn upcoming(&self, project_id: Option<&str>, limit: usize) -> Result<Vec<UpcomingTask>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let now = Utc::now();
        let scheduled = self.db.list_scheduled(project_id)?;
        let projects = self.db.list_projects()?;
        let name_of = |id: &str| {
            projects
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone())
                .unwrap_or_default()
        };

        let mut all = Vec::new();
        for st in scheduled.iter().filter(|s| s.enabled && s.valid) {
            // Seed from the stored next run if it's still in the future, else
            // recompute from now.
            let seed = st
                .next_run
                .filter(|t| *t > now)
                .or_else(|| scheduled::next_run_after(&st.schedule_kind, &st.schedule, now));
            let Some(seed) = seed else {
                continue;
            };
            for run_at in scheduled::occurrences(&st.schedule_kind, &st.schedule, seed, limit) {
                if run_at <= now {
                    continue;
                }
                all.push(UpcomingTask {
                    scheduled_id: st.id.clone(),
                    project_id: st.project_id.clone(),
                    project_name: name_of(&st.project_id),
                    title: st.title.clone(),
                    agent: st.agent,
                    priority: st.priority,
                    schedule_desc: st.schedule_desc.clone(),
                    run_at,
                });
            }
        }
        all.sort_by_key(|u| u.run_at);
        all.truncate(limit);
        Ok(all)
    }

    fn discover_scheduled_for_project(&self, project: &Project) -> Result<usize> {
        let dir = Path::new(&project.path).join(scheduled::SCHEDULED_DIR);
        let now = Utc::now();
        let mut keep = Vec::new();
        if dir.is_dir() {
            for entry in std::fs::read_dir(&dir)?.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                let rel = format!("{}/{}", scheduled::SCHEDULED_DIR, file_name);
                let mut st = scheduled::parse_scheduled(&project.id, &path, &rel, &content, now);
                // Preserve run history when the schedule itself is unchanged.
                if let Some(existing) = self.db.get_scheduled(&st.id)? {
                    st.created_at = existing.created_at;
                    if existing.schedule == st.schedule
                        && existing.schedule_kind == st.schedule_kind
                    {
                        st.last_run = existing.last_run;
                        st.next_run = existing.next_run.or(st.next_run);
                    }
                }
                self.db.upsert_scheduled(&st)?;
                keep.push(st.id.clone());
            }
        }
        self.db.prune_scheduled(&project.id, &keep)?;
        Ok(keep.len())
    }

    /// Create tasks for any scheduled jobs whose time has come, advancing each to
    /// its next slot. Missed slots collapse to a single firing.
    fn fire_due_scheduled(&self) -> Result<u32> {
        let now = Utc::now();
        let mut fired = 0;
        for st in self.db.due_scheduled(now)? {
            // Advance the schedule first so we never refire the same slot.
            let next = scheduled::next_run_after(&st.schedule_kind, &st.schedule, now);
            self.db.set_scheduled_run(&st.id, now, next)?;

            let Ok(project) = self.db.get_project(&st.project_id) else {
                continue;
            };
            if !project.enabled {
                continue;
            }

            let requested = st.agent.filter(|a| project.allows(*a));
            let (agent, auto_agent) = match requested {
                Some(a) => (a, false),
                None => (project.default_agent, true),
            };
            let task = Task {
                id: Uuid::new_v4().to_string(),
                project_id: project.id.clone(),
                title: st.title.clone(),
                description: st.body.clone(),
                status: TaskStatus::Pending,
                priority: st.priority,
                agent,
                auto_agent,
                model: st.model.clone(),
                parent_id: None,
                depends_on: vec![],
                attempts: 0,
                max_attempts: 3,
                tags: vec!["scheduled".into()],
                auto_generated: true,
                created_at: now,
                updated_at: now,
            };
            self.db.upsert_task(&task)?;
            self.emit(OrchestratorEvent::TaskUpdated { task });
            self.log(
                "info",
                format!("scheduled task '{}' fired for {}", st.title, project.name),
            );
            fired += 1;
        }
        if fired > 0 {
            self.emit(OrchestratorEvent::ScheduledChanged);
        }
        Ok(fired)
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
        let settings = self.db.get_settings()?;
        let agent = self.choose_agent(project, &task, &settings);

        task.status = TaskStatus::Running;
        task.updated_at = Utc::now();
        self.db.upsert_task(&task)?;
        self.emit(OrchestratorEvent::TaskUpdated { task: task.clone() });

        let prompt = self.task_prompt(project, &task);
        let session = self.new_session(project, Some(&task), agent, SessionKind::Task, &prompt);
        self.db.upsert_session(&session)?;

        let spec = self.run_spec(project, agent, &prompt, task.model.as_deref());
        self.clone()
            .spawn_session_job(session, spec, Some(task), settings.live_streaming);
        Ok(())
    }

    fn spawn_roadmap_session(self: &Arc<Self>, project: &Project) -> Result<()> {
        let prompt = conventions::roadmap_prompt(&project.path);
        let agent = project.default_agent;
        let session = self.new_session(project, None, agent, SessionKind::Roadmap, &prompt);
        self.db.upsert_session(&session)?;
        self.log(
            "info",
            format!("roadmap loop starting for {}", project.name),
        );
        let spec = self.run_spec(project, agent, &prompt, None);
        self.clone().spawn_session_job(session, spec, None, false);
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

    fn run_spec(
        &self,
        project: &Project,
        agent: AgentKind,
        prompt: &str,
        model_override: Option<&str>,
    ) -> RunSpec {
        let settings = self.db.get_settings().unwrap_or_default();
        let cfg = settings.agent_config(agent);
        let mut spec = RunSpec::new(prompt, &project.path);
        // Model precedence: explicit override > per-agent config > agent default.
        spec.model = model_override
            .map(|m| m.to_string())
            .or_else(|| cfg.model.clone())
            .or_else(|| agent.default_model().map(String::from));
        spec.permission_mode = settings.permission_mode;
        spec.extra_args = cfg.extra_args.clone();
        spec
    }

    /// Decide which agent actually runs a task. Honors an explicit pin, otherwise
    /// (when balancing is on) picks the least-used *available* agent among the
    /// project's allowed set so usage stays even across Claude/Gemini/Codex.
    fn choose_agent(&self, project: &Project, task: &Task, settings: &Settings) -> AgentKind {
        let allowed = project.effective_allowed_agents();

        // Explicit pin: use it if allowed, else fall through to balancing/fallback.
        if !task.auto_agent && allowed.contains(&task.agent) {
            return task.agent;
        }

        let now = Utc::now();
        let candidates: Vec<AgentKind> = allowed
            .iter()
            .copied()
            .filter(|a| {
                let cfg = settings.agent_config(*a);
                if !cfg.enabled {
                    return false;
                }
                let binary = cfg
                    .binary
                    .clone()
                    .unwrap_or_else(|| agents::adapter_for(*a).default_binary().to_string());
                util::binary_available(&binary)
            })
            .collect();

        let pool = if candidates.is_empty() {
            allowed.clone()
        } else {
            candidates
        };

        if !settings.balance_agents || pool.len() == 1 {
            // No balancing: prefer the task's agent if in pool, else the default.
            if pool.contains(&task.agent) {
                return task.agent;
            }
            return pool.first().copied().unwrap_or(project.default_agent);
        }

        // Pick the agent with the lowest windowed cost, tie-broken by active sessions.
        pool.into_iter()
            .min_by(|a, b| {
                let load_a = self.agent_load(*a, settings, now);
                let load_b = self.agent_load(*b, settings, now);
                load_a
                    .partial_cmp(&load_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(project.default_agent)
    }

    /// A scalar "load" for an agent within its window: windowed cost plus a small
    /// nudge per active session so concurrent work spreads out too.
    fn agent_load(&self, agent: AgentKind, settings: &Settings, now: DateTime<Utc>) -> f64 {
        let cfg = settings.agent_config(agent);
        let window_start = now - chrono::Duration::hours(cfg.session_window_hours.max(1) as i64);
        let cost = self
            .db
            .usage_for_agent(agent, Some(window_start))
            .map(|u| u.total_cost_usd)
            .unwrap_or(0.0);
        let active = self.db.count_active_sessions_for_agent(agent).unwrap_or(0) as f64;
        cost + active * 0.001
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
    fn spawn_session_job(
        self: Arc<Self>,
        session: Session,
        spec: RunSpec,
        task: Option<Task>,
        live: bool,
    ) {
        tokio::spawn(async move {
            let outcome = self.run_session(&session, &spec, live).await;
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
                    self.log(
                        "error",
                        format!("session {} failed to run: {e}", session.id),
                    );
                    self.finalize_session_error(&session, &e.to_string());
                }
            }
            self.emit(OrchestratorEvent::StatusChanged);
            self.emit(OrchestratorEvent::UsageUpdated);
            self.request_tick();
        });
    }

    /// Drive a single agent invocation: stream + persist events, finalize the
    /// session row, and record usage. In `live` mode, partial token deltas stream
    /// to the UI and a stdin channel is registered for mid-run message injection.
    async fn run_session(
        &self,
        session: &Session,
        spec: &RunSpec,
        live: bool,
    ) -> Result<RunOutcome> {
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
        self.emit(OrchestratorEvent::SessionUpdated {
            session: row.clone(),
        });
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

        // Build the (possibly live) spec + input channel.
        let mut spec = spec.clone();
        let input_rx = if live {
            spec.live = true;
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            // Deliver the initial prompt as the first stdin message.
            let _ = tx.send(spec.prompt.clone());
            self.inputs.lock().unwrap().insert(session.id.clone(), tx);
            Some(rx)
        } else {
            None
        };

        let db = self.db.clone();
        let sink = self.sink.clone();
        let inputs = self.inputs.clone();
        let sid = session.id.clone();
        let project_id = session.project_id.clone();
        let task_id = session.task_id.clone();

        let result = runner::run_agent(
            adapter.as_ref(),
            &binary,
            &spec,
            cancel,
            timeout,
            input_rx,
            |event| {
                // Ephemeral token deltas: stream live, do not persist.
                if event.is_delta() {
                    let (kind, text) = match event {
                        agents::AgentEvent::TextDelta { text } => ("assistant", text.clone()),
                        agents::AgentEvent::ThinkingDelta { text } => ("thinking", text.clone()),
                        _ => return,
                    };
                    sink.emit(OrchestratorEvent::SessionDelta {
                        session_id: sid.clone(),
                        kind: kind.to_string(),
                        text,
                    });
                    return;
                }

                // A final result means the turn is done: close the input channel
                // so the agent receives EOF and exits.
                if matches!(event, agents::AgentEvent::Result { .. }) {
                    inputs.lock().unwrap().remove(&sid);
                }

                let (text, data) = describe_event(event);
                if let Ok(stored) = db.insert_event(
                    &sid,
                    event.kind(),
                    text.as_deref(),
                    data.as_ref(),
                    Utc::now(),
                ) {
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
        self.inputs.lock().unwrap().remove(&session.id);

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
        self.emit(OrchestratorEvent::SessionUpdated {
            session: row.clone(),
        });

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
                task.description = append_feedback(
                    &task.description,
                    &format!("Previous attempt failed: {err}"),
                );
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
                    format!(
                        "verifier produced no verdict for task {}; marking complete",
                        task.id
                    ),
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
        let result_text = task_outcome
            .result_text
            .as_deref()
            .unwrap_or("(no result text)");
        let prompt = format!(
            "{base}\n\n---\n## Task under review\nTitle: {}\n\nAcceptance criteria / description:\n{}\n\n## Executing session result\n{}\n",
            task.title, task.description, result_text
        );
        let session = self.new_session(
            project,
            Some(task),
            project.default_agent,
            SessionKind::Verify,
            &prompt,
        );
        self.db.upsert_session(&session)?;
        let spec = self.run_spec(project, project.default_agent, &prompt, None);
        let outcome = self.run_session(&session, &spec, false).await?;
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
            // Respect an explicitly-requested agent only if the project allows it;
            // otherwise leave it auto so the scheduler load-balances.
            let requested = spec.agent.as_deref().and_then(AgentKind::from_str);
            let (agent, auto_agent) = match requested {
                Some(a) if project.allows(a) => (a, false),
                _ => (project.default_agent, true),
            };
            let task = Task {
                id: Uuid::new_v4().to_string(),
                project_id: project.id.clone(),
                title: spec.title.clone(),
                description: spec.description.clone(),
                status: TaskStatus::Pending,
                priority: spec.priority_or_default(),
                agent,
                auto_agent,
                model: None,
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
        self.log(
            "info",
            format!("roadmap created {created} task(s) for {}", project.name),
        );
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
fn describe_event(
    event: &crate::agents::AgentEvent,
) -> (Option<String>, Option<serde_json::Value>) {
    use crate::agents::AgentEvent::*;
    match event {
        Init {
            agent_session_id,
            model,
        } => (
            Some(format!(
                "session initialized ({})",
                model.clone().unwrap_or_else(|| "model unknown".into())
            )),
            Some(serde_json::json!({ "agentSessionId": agent_session_id, "model": model })),
        ),
        Assistant { text } => (Some(text.clone()), None),
        Thinking { text } => (Some(text.clone()), None),
        // Deltas are streamed live and never persisted, but keep the match total.
        TextDelta { text } | ThinkingDelta { text } => (Some(text.clone()), None),
        ToolUse { name, input } => (
            Some(name.clone()),
            Some(serde_json::json!({ "name": name, "input": input })),
        ),
        ToolResult { content, is_error } => (
            Some(content.clone()),
            Some(serde_json::json!({ "isError": is_error })),
        ),
        Result {
            success,
            result_text,
            usage,
        } => (
            result_text.clone(),
            Some(serde_json::json!({ "success": success, "usage": usage })),
        ),
        Error { message } => (Some(message.clone()), None),
        Raw { value } => (None, Some(value.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::NullSink;

    fn engine() -> Arc<Engine> {
        Engine::new(Db::open_in_memory().unwrap(), Arc::new(NullSink))
    }

    fn mk_project(eng: &Engine, path: &str, allowed: Vec<AgentKind>) -> Project {
        let now = Utc::now();
        let p = Project {
            id: Uuid::new_v4().to_string(),
            name: "p".into(),
            path: path.to_string(),
            description: None,
            enabled: true,
            default_agent: AgentKind::Claude,
            allowed_agents: allowed,
            max_concurrent: None,
            roadmap_enabled: false,
            verify_enabled: false,
            created_at: now,
            updated_at: now,
        };
        eng.db.upsert_project(&p).unwrap();
        p
    }

    fn mk_task(project_id: &str, agent: AgentKind, auto_agent: bool) -> Task {
        let now = Utc::now();
        Task {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.into(),
            title: "t".into(),
            description: "do".into(),
            status: TaskStatus::Pending,
            priority: 50,
            agent,
            auto_agent,
            model: None,
            parent_id: None,
            depends_on: vec![],
            attempts: 0,
            max_attempts: 3,
            tags: vec![],
            auto_generated: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn explicit_pin_is_respected_when_allowed() {
        let eng = engine();
        let tmp = std::env::temp_dir();
        let p = mk_project(
            &eng,
            &tmp.to_string_lossy(),
            vec![AgentKind::Claude, AgentKind::Gemini],
        );
        let settings = eng.db.get_settings().unwrap();
        let task = mk_task(&p.id, AgentKind::Gemini, false);
        assert_eq!(eng.choose_agent(&p, &task, &settings), AgentKind::Gemini);
    }

    #[test]
    fn disallowed_pin_falls_back_into_allowed_set() {
        let eng = engine();
        let tmp = std::env::temp_dir();
        let p = mk_project(&eng, &tmp.to_string_lossy(), vec![AgentKind::Claude]);
        let settings = eng.db.get_settings().unwrap();
        // Task pins Gemini, but the project only allows Claude.
        let task = mk_task(&p.id, AgentKind::Gemini, false);
        let chosen = eng.choose_agent(&p, &task, &settings);
        assert!(p.allows(chosen));
        assert_ne!(chosen, AgentKind::Gemini);
    }

    #[test]
    fn scheduled_discovery_and_firing() {
        let eng = engine();
        let dir = std::env::temp_dir().join(format!("orch-sched-{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".orchestrator/scheduled")).unwrap();
        std::fs::write(
            dir.join(".orchestrator/scheduled/job.md"),
            "---\nevery: 1h\ntitle: Nightly job\n---\nDo the scheduled thing.",
        )
        .unwrap();

        let p = mk_project(&eng, &dir.to_string_lossy(), vec![AgentKind::Claude]);
        assert_eq!(eng.discover_all_scheduled().unwrap(), 1);

        let sched = eng.db.list_scheduled(Some(&p.id)).unwrap();
        assert_eq!(sched.len(), 1);
        assert!(sched[0].valid);
        assert_eq!(sched[0].title, "Nightly job");
        assert!(sched[0].next_run.is_some());

        // Force it due, then fire.
        let past = Utc::now() - chrono::Duration::minutes(1);
        eng.db
            .set_scheduled_run(&sched[0].id, past, Some(past))
            .unwrap();
        assert_eq!(eng.fire_due_scheduled().unwrap(), 1);

        let tasks = eng.db.list_tasks(Some(&p.id)).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].description.trim(), "Do the scheduled thing.");
        assert!(tasks[0].tags.contains(&"scheduled".to_string()));

        // Next run advanced into the future; no longer due.
        let after = eng.db.get_scheduled(&sched[0].id).unwrap().unwrap();
        assert!(after.next_run.unwrap() > Utc::now());
        assert_eq!(eng.fire_due_scheduled().unwrap(), 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn upcoming_projects_future_firings_sorted() {
        let eng = engine();
        let dir = std::env::temp_dir().join(format!("orch-up-{}", Uuid::new_v4()));
        let sdir = dir.join(".orchestrator/scheduled");
        std::fs::create_dir_all(&sdir).unwrap();
        // A frequent interval job and a disabled one (should be excluded).
        std::fs::write(
            sdir.join("often.md"),
            "---\nevery: 15m\ntitle: Often\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            sdir.join("off.md"),
            "---\nevery: 5m\nenabled: false\ntitle: Off\n---\nbody",
        )
        .unwrap();
        mk_project(&eng, &dir.to_string_lossy(), vec![AgentKind::Claude]);
        eng.discover_all_scheduled().unwrap();

        let up = eng.upcoming(None, 10).unwrap();
        assert_eq!(up.len(), 10); // the 15m job alone fills 10 slots
        assert!(up.iter().all(|u| u.title == "Often"));
        // Sorted ascending and all in the future.
        let now = Utc::now();
        for w in up.windows(2) {
            assert!(w[0].run_at <= w[1].run_at);
        }
        assert!(up.iter().all(|u| u.run_at > now));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn removed_files_are_pruned() {
        let eng = engine();
        let dir = std::env::temp_dir().join(format!("orch-sched-{}", Uuid::new_v4()));
        let sdir = dir.join(".orchestrator/scheduled");
        std::fs::create_dir_all(&sdir).unwrap();
        std::fs::write(sdir.join("a.md"), "---\nevery: 1h\n---\nbody").unwrap();
        let p = mk_project(&eng, &dir.to_string_lossy(), vec![AgentKind::Claude]);
        assert_eq!(eng.discover_all_scheduled().unwrap(), 1);
        std::fs::remove_file(sdir.join("a.md")).unwrap();
        assert_eq!(eng.discover_all_scheduled().unwrap(), 0);
        assert_eq!(eng.db.list_scheduled(Some(&p.id)).unwrap().len(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}

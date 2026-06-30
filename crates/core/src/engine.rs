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
use crate::worktree;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
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

        // Config backup loop: write a backup every `backup_interval_hours` when
        // enabled and a directory is configured.
        let backupper = self.clone();
        tokio::spawn(async move {
            loop {
                let hours = backupper
                    .db
                    .get_settings()
                    .map(|s| s.backup_interval_hours)
                    .unwrap_or(24)
                    .max(1);
                tokio::time::sleep(Duration::from_secs(hours * 3600)).await;
                let settings = backupper.db.get_settings().unwrap_or_default();
                if settings.backup_enabled && !settings.backup_dir.trim().is_empty() {
                    if let Err(e) = backupper.backup_now() {
                        backupper.log("warn", format!("config backup failed: {e}"));
                    }
                }
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
            branch: None,
            pr_url: None,
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
        let aging = settings.priority_aging_per_hour;

        // --- Phase 1: fair task allocation. ---
        // Collect each enabled project's startable tasks (capped by its own free
        // slots), then hand out the free global slots round-robin so one busy
        // project can't monopolize the fleet just because it sorts first.
        let mut pools: HashMap<String, VecDeque<Task>> = HashMap::new();
        for project in projects.iter().filter(|p| p.enabled) {
            let proj_max = project.max_concurrent.unwrap_or(global_max).max(1);
            let proj_active = self.db.count_active_sessions_for_project(&project.id)?;
            let free = proj_max.saturating_sub(proj_active) as usize;
            if free == 0 {
                continue;
            }
            let mut eligible = self.eligible_tasks(&project.id, aging)?;
            eligible.truncate(free);
            if !eligible.is_empty() {
                pools.insert(project.id.clone(), eligible.into_iter().collect());
            }
        }
        let caps: Vec<(String, u32)> = pools
            .iter()
            .map(|(id, q)| (id.clone(), q.len() as u32))
            .collect();
        let proj_by_id: HashMap<&str, &Project> =
            projects.iter().map(|p| (p.id.as_str(), p)).collect();
        for pid in round_robin_allocation(&caps, global_max.saturating_sub(active)) {
            if active >= global_max {
                break;
            }
            if let (Some(project), Some(queue)) =
                (proj_by_id.get(pid.as_str()), pools.get_mut(&pid))
            {
                if let Some(task) = queue.pop_front() {
                    self.start_task(project, task)?;
                    active += 1;
                }
            }
        }

        // --- Phase 2: roadmap loop for projects with a genuinely empty queue. ---
        let now = Utc::now();
        for project in projects.iter().filter(|p| p.enabled) {
            if active >= global_max {
                break;
            }
            if !settings.roadmap_enabled || !project.roadmap_enabled {
                continue;
            }
            let proj_max = project.max_concurrent.unwrap_or(global_max).max(1);
            if self.db.count_active_sessions_for_project(&project.id)? >= proj_max {
                continue;
            }
            // Only when there is no schedulable work and nothing merely backing off.
            if !self.eligible_tasks(&project.id, aging)?.is_empty()
                || self.has_backing_off_tasks(&project.id)?
            {
                continue;
            }
            // Cooldown: don't regenerate within the configured interval of the
            // last roadmap run for this project.
            if settings.roadmap_min_interval_mins > 0 {
                if let Some(last) = self.db.last_roadmap_at(&project.id)? {
                    let mins = (now - last).num_minutes();
                    if mins >= 0 && (mins as u32) < settings.roadmap_min_interval_mins {
                        continue;
                    }
                }
            }
            let roadmap_active = active_sessions
                .iter()
                .any(|s| s.project_id == project.id && s.kind == SessionKind::Roadmap);
            if !roadmap_active && !self.has_running_roadmap(&project.id)? {
                self.spawn_roadmap_session(project)?;
                active += 1;
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
                // Scheduled tasks are not retried by default — they run again on
                // their own cadence rather than via the backoff retry loop.
                max_attempts: 1,
                tags: vec!["scheduled".into()],
                auto_generated: true,
                retry_at: None,
                notes: None,
                created_at: now,
                updated_at: now,
            };
            self.db.upsert_task(&task)?;
            let task_id = task.id.clone();
            self.emit(OrchestratorEvent::TaskUpdated { task });
            self.log(
                "info",
                format!("scheduled task '{}' fired for {}", st.title, project.name),
            );
            self.record_activity(
                "scheduled",
                "info",
                format!("Scheduled task fired: {}", st.title),
                Some(&project.id),
                Some(&task_id),
                None,
            );
            fired += 1;
        }
        if fired > 0 {
            self.emit(OrchestratorEvent::ScheduledChanged);
        }
        Ok(fired)
    }

    /// Pick the highest-priority schedulable task whose dependencies are met,
    /// whose attempts are not exhausted, and whose retry backoff has elapsed.
    /// All schedulable tasks for a project whose attempts aren't exhausted, whose
    /// retry backoff has elapsed, and whose dependencies are met — sorted best
    /// first by effective (aged) priority.
    fn eligible_tasks(&self, project_id: &str, aging: f64) -> Result<Vec<Task>> {
        let now = Utc::now();
        let mut eligible: Vec<Task> = Vec::new();
        for task in self.db.schedulable_tasks(project_id)? {
            if task.attempts >= task.max_attempts {
                continue;
            }
            if task.retry_at.map(|r| r > now).unwrap_or(false) {
                continue; // still backing off
            }
            if self.deps_satisfied(&task)? {
                eligible.push(task);
            }
        }
        // Highest effective (aged) priority wins; older task breaks ties.
        eligible.sort_by(|a, b| {
            effective_priority(b, now, aging)
                .partial_cmp(&effective_priority(a, now, aging))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.created_at.cmp(&b.created_at))
        });
        Ok(eligible)
    }

    /// True if a project has schedulable tasks that are only waiting out their
    /// retry backoff (so the queue isn't really empty — don't run the roadmap).
    fn has_backing_off_tasks(&self, project_id: &str) -> Result<bool> {
        let now = Utc::now();
        Ok(self
            .db
            .schedulable_tasks(project_id)?
            .iter()
            .any(|t| t.attempts < t.max_attempts && t.retry_at.map(|r| r > now).unwrap_or(false)))
    }

    /// Backoff before the next retry: `base * 2^(attempts-1)`, capped at the
    /// configured maximum. `None` when retries are disabled.
    fn retry_delay(&self, settings: &Settings, attempts: u32) -> Option<chrono::Duration> {
        if !settings.retry_enabled {
            return None;
        }
        let exp = attempts.saturating_sub(1).min(20);
        let secs = settings
            .retry_base_secs
            .saturating_mul(1u64 << exp)
            .min(settings.retry_max_secs.max(1));
        Some(chrono::Duration::seconds(secs as i64))
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
        let mut session = self.new_session(project, Some(&task), agent, SessionKind::Task, &prompt);
        let mut spec = self.run_spec(project, agent, &prompt, task.model.as_deref());

        // Isolate the session in its own git worktree + branch when enabled and
        // the project is a git repo, so concurrent sessions don't collide.
        if settings.isolate_worktrees && worktree::is_git_repo(&project.path) {
            let branch = worktree::branch_name(&task.title, &session.id);
            let wt = worktree::worktrees_root().join(&session.id);
            match worktree::create(Path::new(&project.path), &branch, &wt) {
                Ok(()) => {
                    self.log("info", format!("isolated task on {branch}"));
                    session.branch = Some(branch);
                    spec.cwd = wt;
                }
                Err(e) => self.log(
                    "warn",
                    format!("worktree setup failed, running in-place: {e}"),
                ),
            }
        }

        self.db.upsert_session(&session)?;
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
            branch: None,
            pr_url: None,
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
        // Per-project MCP servers (passed to the agent's `--mcp-config`).
        spec.mcp_config = project
            .mcp_config
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
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
        // Project memory: auto-generated context and learned lessons steer the
        // agent before the task itself.
        if let Some(context) = conventions::project_context(&project.path) {
            let context = context.trim();
            if !context.is_empty() {
                prompt.push_str("## Project context\n\n");
                prompt.push_str(context);
                prompt.push_str("\n\n---\n\n");
            }
        }
        if let Some(lessons) = conventions::lessons(&project.path) {
            let lessons = lessons.trim();
            if !lessons.is_empty() {
                prompt.push_str("## Lessons from previous work (apply these)\n\n");
                prompt.push_str(lessons);
                prompt.push_str("\n\n---\n\n");
            }
        }
        prompt.push_str(&format!("# Task: {}\n\n", task.title));
        prompt.push_str(&task.description);
        prompt
    }

    /// Regenerate `.orchestrator/context.md` for a project from its repo
    /// contents. Returns the generated markdown.
    pub fn generate_project_context(&self, project_id: &str) -> Result<String> {
        let project = self.db.get_project(project_id)?;
        let content = conventions::generate_context(&project.path)?;
        conventions::write_context(&project.path, &content)?;
        self.log(
            "info",
            format!("regenerated project context for {}", project.name),
        );
        Ok(content)
    }

    /// Read a project's accumulated memory (context + lessons) for display.
    pub fn project_memory(&self, project_id: &str) -> Result<ProjectMemory> {
        let project = self.db.get_project(project_id)?;
        Ok(conventions::memory(&project.path))
    }

    /// Import open GitHub issues for a project as pending tasks via the `gh`
    /// CLI. Issues already imported (tagged `gh-issue-<n>`) are skipped. Returns
    /// the number of new tasks created.
    pub fn import_github_issues(self: &Arc<Self>, project_id: &str) -> Result<u32> {
        let project = self.db.get_project(project_id)?;
        if !crate::github::available() {
            return Err(CoreError::Other(
                "the GitHub CLI `gh` is not installed or not on PATH".into(),
            ));
        }
        let issues = crate::github::list_open_issues(&project.path, 100)?;
        let existing = self.db.list_tasks(Some(project_id))?;
        let now = Utc::now();
        let mut created = 0u32;
        for issue in issues {
            let tag = crate::github::issue_tag(issue.number);
            if existing.iter().any(|t| t.tags.contains(&tag)) {
                continue;
            }
            let mut description = issue.body.trim().to_string();
            if description.is_empty() {
                description = "(no description provided in the issue)".into();
            }
            if !issue.url.is_empty() {
                description.push_str(&format!("\n\n---\nGitHub issue: {}", issue.url));
            }
            let mut tags = vec!["github".to_string(), tag];
            tags.extend(issue.label_names());
            let task = Task {
                id: Uuid::new_v4().to_string(),
                project_id: project.id.clone(),
                title: format!("#{} {}", issue.number, issue.title),
                description,
                status: TaskStatus::Pending,
                priority: 50,
                agent: project.default_agent,
                auto_agent: true,
                model: None,
                parent_id: None,
                depends_on: vec![],
                attempts: 0,
                max_attempts: project.effective_max_attempts(),
                tags,
                auto_generated: true,
                retry_at: None,
                notes: None,
                created_at: now,
                updated_at: now,
            };
            self.db.upsert_task(&task)?;
            self.emit(OrchestratorEvent::TaskUpdated { task });
            created += 1;
        }
        if created > 0 {
            self.log(
                "info",
                format!("imported {created} GitHub issue(s) for {}", project.name),
            );
            self.record_activity(
                "github",
                "info",
                format!("Imported {created} GitHub issue(s)"),
                Some(&project.id),
                None,
                None,
            );
            self.request_tick();
        }
        Ok(created)
    }

    /// Detect each agent CLI: whether its configured binary is on PATH and, if
    /// so, the version it reports. Used by the settings panel and onboarding.
    pub fn agent_health(&self) -> Result<Vec<AgentHealth>> {
        let settings = self.db.get_settings()?;
        let mut out = Vec::new();
        for agent in AgentKind::ALL {
            let cfg = settings.agent_config(agent);
            let binary = cfg
                .binary
                .clone()
                .unwrap_or_else(|| agents::adapter_for(agent).default_binary().to_string());
            let available = util::binary_available(&binary);
            let version = if available {
                std::process::Command::new(&binary)
                    .arg("--version")
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| {
                        String::from_utf8_lossy(&o.stdout)
                            .lines()
                            .next()
                            .map(|l| l.trim().to_string())
                            .filter(|l| !l.is_empty())
                    })
            } else {
                None
            };
            out.push(AgentHealth {
                agent,
                binary,
                available,
                version,
            });
        }
        Ok(out)
    }

    /// Run a system self-check across agents, git, the database, and project
    /// configuration. Returns one line item per finding so the UI can render a
    /// severity-coded list.
    pub fn diagnostics(&self) -> Result<Vec<Diagnostic>> {
        let mut out = Vec::new();

        // Agents — reuse the health probe and remember availability for the
        // per-project check below.
        let health = self.agent_health()?;
        let mut available: std::collections::HashMap<AgentKind, bool> =
            std::collections::HashMap::new();
        for h in &health {
            available.insert(h.agent, h.available);
            out.push(Diagnostic {
                category: "agent".into(),
                name: format!("{} CLI", h.agent.as_str()),
                level: if h.available { "ok" } else { "warn" }.into(),
                detail: if h.available {
                    h.version.clone().unwrap_or_else(|| "installed".into())
                } else {
                    format!("`{}` not found on PATH", h.binary)
                },
            });
        }

        // git — required for branch/PR/worktree features.
        let git_ok = util::binary_available("git");
        out.push(Diagnostic {
            category: "git".into(),
            name: "git".into(),
            level: if git_ok { "ok" } else { "error" }.into(),
            detail: if git_ok {
                "available on PATH".into()
            } else {
                "`git` not found on PATH — branches, PRs and worktrees will fail".into()
            },
        });

        // Database — confirm the file is writable, not just present.
        match self.db.writable() {
            Ok(()) => out.push(Diagnostic {
                category: "database".into(),
                name: "database".into(),
                level: "ok".into(),
                detail: "writable".into(),
            }),
            Err(e) => out.push(Diagnostic {
                category: "database".into(),
                name: "database".into(),
                level: "error".into(),
                detail: format!("not writable: {e}"),
            }),
        }

        // Projects — flag missing paths and allowed-but-uninstalled agents.
        let settings = self.db.get_settings()?;
        for p in self.db.list_projects()? {
            if !p.enabled {
                continue;
            }
            if !Path::new(&p.path).is_dir() {
                out.push(Diagnostic {
                    category: "project".into(),
                    name: p.name.clone(),
                    level: "error".into(),
                    detail: format!("path no longer exists: {}", p.path),
                });
                continue;
            }
            for a in p.effective_allowed_agents() {
                let installed = available.get(&a).copied().unwrap_or_else(|| {
                    let cfg = settings.agent_config(a);
                    let binary = cfg
                        .binary
                        .clone()
                        .unwrap_or_else(|| agents::adapter_for(a).default_binary().to_string());
                    util::binary_available(&binary)
                });
                if !installed {
                    out.push(Diagnostic {
                        category: "project".into(),
                        name: p.name.clone(),
                        level: "warn".into(),
                        detail: format!("allows {} but its CLI isn't installed", a.as_str()),
                    });
                }
            }
        }

        Ok(out)
    }

    /// The next tasks the scheduler would run across all enabled projects,
    /// ordered by effective (aged) priority — a preview of the live queue.
    pub fn upcoming_queue(&self, limit: usize) -> Result<Vec<QueuedTask>> {
        let now = Utc::now();
        let aging = self.db.get_settings()?.priority_aging_per_hour;
        let mut out: Vec<QueuedTask> = Vec::new();
        for project in self.db.list_projects()? {
            if !project.enabled {
                continue;
            }
            for task in self.db.schedulable_tasks(&project.id)? {
                if task.attempts >= task.max_attempts {
                    continue;
                }
                if task.retry_at.map(|r| r > now).unwrap_or(false) {
                    continue;
                }
                if !self.deps_satisfied(&task)? {
                    continue;
                }
                let effective_priority = effective_priority(&task, now, aging);
                out.push(QueuedTask {
                    project_name: project.name.clone(),
                    effective_priority,
                    task,
                });
            }
        }
        out.sort_by(|a, b| {
            b.effective_priority
                .partial_cmp(&a.effective_priority)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.task.created_at.cmp(&b.task.created_at))
        });
        out.truncate(limit);
        Ok(out)
    }

    /// Sessions completed vs failed per day over the last `days` days.
    pub fn session_throughput(&self, days: u32) -> Result<Vec<ThroughputPoint>> {
        self.db.session_throughput(days)
    }

    /// Per-project analytics: headline totals, a per-agent breakdown, and a daily
    /// completed/failed series — all derived from the project's task sessions.
    pub fn project_analytics(&self, project_id: &str, days: u32) -> Result<ProjectAnalytics> {
        let sessions = self.db.list_sessions(None, Some(project_id))?;
        // Finished task sessions are the basis for stats and the agent breakdown.
        let finished: Vec<&Session> = sessions
            .iter()
            .filter(|s| {
                s.kind == SessionKind::Task
                    && matches!(
                        s.status,
                        SessionStatus::Completed | SessionStatus::Failed | SessionStatus::TimedOut
                    )
            })
            .collect();

        let mut stats = ProjectStats::default();
        let mut dur_sum = 0.0;
        let mut dur_n = 0u32;
        // Per-agent accumulators.
        let mut per_agent: HashMap<AgentKind, (u32, u32, u32, f64, f64, u32)> = HashMap::new();
        for s in &finished {
            stats.sessions += 1;
            stats.total_cost_usd += s.usage.total_cost_usd;
            stats.total_tokens += s.usage.input_tokens + s.usage.output_tokens;
            let ok = s.status == SessionStatus::Completed;
            if ok {
                stats.completed += 1;
            } else {
                stats.failed += 1;
            }
            let dur = match (s.started_at, s.ended_at) {
                (Some(a), Some(b)) => Some((b - a).num_milliseconds() as f64 / 1000.0),
                _ => None,
            };
            if let Some(d) = dur {
                dur_sum += d;
                dur_n += 1;
            }
            let e = per_agent.entry(s.agent).or_insert((0, 0, 0, 0.0, 0.0, 0));
            e.0 += 1;
            if ok {
                e.1 += 1;
            } else {
                e.2 += 1;
            }
            e.3 += s.usage.total_cost_usd;
            if let Some(d) = dur {
                e.4 += d;
                e.5 += 1;
            }
        }
        stats.success_rate = if stats.sessions > 0 {
            stats.completed as f64 / stats.sessions as f64
        } else {
            0.0
        };
        stats.avg_duration_secs = if dur_n > 0 {
            dur_sum / dur_n as f64
        } else {
            0.0
        };

        let mut by_agent: Vec<AgentStat> = per_agent
            .into_iter()
            .map(|(agent, (sess, comp, fail, cost, dsum, dn))| AgentStat {
                agent,
                sessions: sess,
                completed: comp,
                failed: fail,
                success_rate: if sess > 0 {
                    comp as f64 / sess as f64
                } else {
                    0.0
                },
                avg_cost_usd: if sess > 0 { cost / sess as f64 } else { 0.0 },
                total_cost_usd: cost,
                avg_duration_secs: if dn > 0 { dsum / dn as f64 } else { 0.0 },
            })
            .collect();
        by_agent.sort_by_key(|a| std::cmp::Reverse(a.sessions));

        // Throughput: completed vs failed sessions per day over the window.
        let now = Utc::now();
        let cutoff = now - chrono::Duration::days(days.max(1) as i64 - 1);
        let mut buckets: std::collections::BTreeMap<String, (u32, u32)> = Default::default();
        for s in &sessions {
            let Some(ended) = s.ended_at else { continue };
            if ended < cutoff {
                continue;
            }
            let day = ended.format("%Y-%m-%d").to_string();
            let e = buckets.entry(day).or_insert((0, 0));
            match s.status {
                SessionStatus::Completed => e.0 += 1,
                SessionStatus::Failed | SessionStatus::TimedOut => e.1 += 1,
                _ => {}
            }
        }
        let throughput = buckets
            .into_iter()
            .map(|(date, (completed, failed))| ThroughputPoint {
                date,
                completed,
                failed,
            })
            .collect();

        Ok(ProjectAnalytics {
            stats,
            by_agent,
            throughput,
        })
    }

    /// Full-text search over session content (prompts, results, errors, and
    /// event transcripts). Returns matches with a context snippet.
    pub fn search_sessions(
        &self,
        query: &str,
        project_id: Option<&str>,
    ) -> Result<Vec<SessionMatch>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let sessions = self.db.search_sessions(q, project_id, 100)?;
        // Resolve task titles in one pass.
        let titles: std::collections::HashMap<String, String> = self
            .db
            .list_tasks(None)?
            .into_iter()
            .map(|t| (t.id, t.title))
            .collect();
        let needle = q.to_lowercase();
        let mut out = Vec::with_capacity(sessions.len());
        for s in sessions {
            // Prefer a snippet from the session's own fields; fall back to events.
            let (matched_in, snippet) =
                if let Some(snip) = snippet_of(s.result_text.as_deref(), &needle) {
                    ("result", snip)
                } else if let Some(snip) = snippet_of(Some(&s.prompt), &needle) {
                    ("prompt", snip)
                } else if let Some(snip) = snippet_of(s.error.as_deref(), &needle) {
                    ("error", snip)
                } else {
                    let ev =
                        self.db.list_events(&s.id)?.into_iter().find_map(|e| {
                            e.text.as_deref().and_then(|t| snippet_of(Some(t), &needle))
                        });
                    ("transcript", ev.unwrap_or_default())
                };
            let task_title = s.task_id.as_ref().and_then(|id| titles.get(id).cloned());
            out.push(SessionMatch {
                task_title,
                snippet,
                matched_in: matched_in.to_string(),
                session: s,
            });
        }
        Ok(out)
    }

    /// Render all sessions for a task as a single Markdown transcript archive.
    pub fn export_task_transcript(&self, task_id: &str) -> Result<String> {
        let task = self.db.get_task(task_id)?;
        let sessions = self.db.list_sessions(Some(task_id), None)?;
        let mut md = format!(
            "# Transcript: {}\n\n_{} session(s)_\n",
            task.title,
            sessions.len()
        );
        for s in &sessions {
            md.push('\n');
            md.push_str(&self.session_markdown(s)?);
        }
        Ok(md)
    }

    /// Render every session in a project as a single Markdown transcript archive.
    pub fn export_project_transcript(&self, project_id: &str) -> Result<String> {
        let project = self.db.get_project(project_id)?;
        let sessions = self.db.list_sessions(None, Some(project_id))?;
        let mut md = format!(
            "# Transcript: {}\n\n_{} session(s)_\n",
            project.name,
            sessions.len()
        );
        for s in &sessions {
            md.push('\n');
            md.push_str(&self.session_markdown(s)?);
        }
        Ok(md)
    }

    /// Markdown for a single session: metadata header, prompt, and transcript.
    fn session_markdown(&self, s: &Session) -> Result<String> {
        let mut md = format!("## {} · {}\n\n", s.kind.as_str(), s.agent.as_str());
        md.push_str(&format!("- **Session:** {}\n", s.id));
        md.push_str(&format!("- **Status:** {}\n", s.status.as_str()));
        if let Some(b) = &s.branch {
            md.push_str(&format!("- **Branch:** {b}\n"));
        }
        if let Some(pr) = &s.pr_url {
            md.push_str(&format!("- **Pull request:** {pr}\n"));
        }
        md.push_str(&format!(
            "- **Tokens:** {} · **Cost:** ${:.4} · **Turns:** {}\n",
            s.usage.input_tokens + s.usage.output_tokens,
            s.usage.total_cost_usd,
            s.usage.num_turns,
        ));
        md.push_str("\n### Prompt\n\n");
        md.push_str(&s.prompt);
        md.push_str("\n\n### Transcript\n\n");
        for e in self.db.list_events(&s.id)? {
            let label = e.kind.replace('_', " ");
            md.push_str(&format!("**{label}**\n\n"));
            if let Some(t) = &e.text {
                if e.kind == "tool_use" {
                    md.push_str(&format!("```\n{t}\n```\n\n"));
                } else {
                    md.push_str(t);
                    md.push_str("\n\n");
                }
            }
        }
        Ok(md)
    }

    /// List orchestrator-created branches in a project repo, flagging which are
    /// merged and which a running session is still using.
    pub fn list_branches(&self, project_id: &str) -> Result<Vec<BranchInfo>> {
        let project = self.db.get_project(project_id)?;
        let repo = PathBuf::from(&project.path);
        if !worktree::is_git_repo(&repo) {
            return Ok(Vec::new());
        }
        let mut branches = worktree::list_orchestrator_branches(&repo)?;
        let active: Vec<String> = self
            .db
            .active_sessions()?
            .into_iter()
            .filter_map(|s| s.branch)
            .collect();
        for b in &mut branches {
            b.active = active.contains(&b.name);
        }
        Ok(branches)
    }

    /// Delete a local orchestrator branch from a project repo. Refuses to delete
    /// a branch a running session is using.
    pub fn delete_branch(&self, project_id: &str, branch: &str) -> Result<()> {
        if !branch.starts_with(worktree::BRANCH_PREFIX) {
            return Err(CoreError::Invalid(format!(
                "refusing to delete non-orchestrator branch {branch}"
            )));
        }
        let in_use = self
            .db
            .active_sessions()?
            .into_iter()
            .any(|s| s.branch.as_deref() == Some(branch));
        if in_use {
            return Err(CoreError::Invalid(format!(
                "branch {branch} is in use by a running session"
            )));
        }
        let project = self.db.get_project(project_id)?;
        worktree::delete_branch(Path::new(&project.path), branch);
        self.log("info", format!("deleted branch {branch}"));
        Ok(())
    }

    /// Rebase an orchestrator branch onto its base branch. Refuses a branch a
    /// running session is using (its worktree is checked out).
    pub fn rebase_branch(&self, project_id: &str, branch: &str) -> Result<RebaseResult> {
        if !branch.starts_with(worktree::BRANCH_PREFIX) {
            return Err(CoreError::Invalid(format!(
                "refusing to rebase non-orchestrator branch {branch}"
            )));
        }
        let in_use = self
            .db
            .active_sessions()?
            .into_iter()
            .any(|s| s.branch.as_deref() == Some(branch));
        if in_use {
            return Err(CoreError::Invalid(format!(
                "branch {branch} is in use by a running session"
            )));
        }
        let project = self.db.get_project(project_id)?;
        let result = worktree::rebase_onto_base(Path::new(&project.path), branch)?;
        self.log("info", format!("rebase {branch}: {}", result.detail));
        let level = if result.status == "conflicts" || result.status == "error" {
            "warn"
        } else {
            "info"
        };
        self.record_activity(
            "branch",
            level,
            format!("Rebase {branch}: {}", result.detail),
            Some(&project.id),
            None,
            None,
        );
        Ok(result)
    }

    /// Prune stale git worktree metadata for a project.
    pub fn prune_worktrees(&self, project_id: &str) -> Result<()> {
        let project = self.db.get_project(project_id)?;
        worktree::prune_worktrees(Path::new(&project.path))?;
        Ok(())
    }

    /// Compute the code changes a task session made on its worktree branch. The
    /// diff is read from the live worktree while the task runs (or before its
    /// changes are committed), and from the committed branch afterward.
    pub fn session_diff(&self, session_id: &str) -> Result<SessionDiff> {
        let session = self.db.get_session(session_id)?;
        let Some(branch) = session.branch.clone() else {
            return Ok(SessionDiff::default());
        };
        let project = self.db.get_project(&session.project_id)?;
        let repo = PathBuf::from(&project.path);
        let wt = worktree::worktrees_root().join(&session.id);
        if wt.exists() {
            worktree::working_diff(&wt, Some(branch))
        } else {
            worktree::branch_diff(&repo, &branch)
        }
    }

    /// Open pull requests for a project, with CI / review status summarized.
    pub fn list_pull_requests(&self, project_id: &str) -> Result<Vec<PullRequest>> {
        let project = self.db.get_project(project_id)?;
        if !crate::github::available() {
            return Err(CoreError::Other(
                "the GitHub CLI `gh` is not installed or not on PATH".into(),
            ));
        }
        crate::github::list_open_prs(&project.path)
    }

    /// Merge a pull request for a project by number (squash + delete branch).
    pub fn merge_pull_request(&self, project_id: &str, number: u64) -> Result<()> {
        let project = self.db.get_project(project_id)?;
        crate::github::merge_pr(&project.path, number)?;
        self.log("info", format!("merged PR #{number} in {}", project.name));
        self.record_activity(
            "pr",
            "info",
            format!("Merged PR #{number}"),
            Some(&project.id),
            None,
            None,
        );
        Ok(())
    }

    /// Record a significant event in the persisted activity history and emit it
    /// so live views update. Best-effort — never fails the caller.
    fn record_activity(
        &self,
        kind: &str,
        level: &str,
        message: impl Into<String>,
        project_id: Option<&str>,
        task_id: Option<&str>,
        session_id: Option<&str>,
    ) {
        let message = message.into();
        match self.db.insert_activity(
            kind,
            level,
            &message,
            project_id,
            task_id,
            session_id,
            Utc::now(),
        ) {
            Ok(mut entry) => {
                if let Some(pid) = &entry.project_id {
                    entry.project_name = self.db.get_project(pid).ok().map(|p| p.name);
                }
                // Bound the log occasionally rather than on every insert.
                if entry.id % 200 == 0 {
                    let keep = self
                        .db
                        .get_settings()
                        .map(|s| s.activity_retention)
                        .unwrap_or(2000);
                    let _ = self.db.prune_activity(keep);
                }
                self.emit(OrchestratorEvent::Activity { entry });
            }
            Err(e) => tracing::debug!(target: "orchestrator", "activity insert failed: {e}"),
        }
    }

    /// Read the activity/audit history, optionally scoped to a project.
    pub fn activity(&self, limit: u32, project_id: Option<&str>) -> Result<Vec<ActivityEntry>> {
        self.db.list_activity(limit, project_id)
    }

    /// Aggregated cost/time for a task across its sessions.
    pub fn task_rollup(&self, task_id: &str) -> Result<TaskRollup> {
        self.db.task_rollup(task_id)
    }

    /// Write a timestamped config backup (settings + projects) into the
    /// configured directory, pruning to the most recent 10. Returns the path.
    pub fn backup_now(&self) -> Result<PathBuf> {
        let settings = self.db.get_settings()?;
        let dir = settings.backup_dir.trim();
        if dir.is_empty() {
            return Err(CoreError::Invalid("no backup directory configured".into()));
        }
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        let bundle = crate::service::export_config(&self.db)?;
        let json = serde_json::to_string_pretty(&bundle)?;
        let ts = Utc::now().format("%Y%m%d-%H%M%S");
        let path = dir.join(format!("orchestrator-config-{ts}.json"));
        std::fs::write(&path, json)?;
        prune_backups(&dir, 10);
        self.log(
            "info",
            format!("config backup written to {}", path.display()),
        );
        self.record_activity(
            "backup",
            "info",
            format!("Config backup written to {}", path.display()),
            None,
            None,
            None,
        );
        Ok(path)
    }

    /// Tasks that may need attention: a session running unusually long, or a
    /// task the verifier keeps bouncing toward its attempt limit.
    pub fn stuck_tasks(&self) -> Result<Vec<StuckTask>> {
        let now = Utc::now();
        const RUNNING_THRESHOLD_SECS: i64 = 900; // 15 minutes
        let mut out: Vec<StuckTask> = Vec::new();

        // Long-running active task sessions.
        for s in self.db.active_sessions()? {
            if s.kind != SessionKind::Task {
                continue;
            }
            if let (Some(tid), Some(start)) = (s.task_id.clone(), s.started_at) {
                let secs = (now - start).num_seconds();
                if secs >= RUNNING_THRESHOLD_SECS {
                    if let Ok(task) = self.db.get_task(&tid) {
                        out.push(StuckTask {
                            task,
                            reason: "running_long".into(),
                            detail: format!("running for {} min", secs / 60),
                        });
                    }
                }
            }
        }

        let all_tasks = self.db.list_tasks(None)?;

        // Tasks repeatedly bounced by the verifier, one attempt from failing.
        for t in &all_tasks {
            if t.status == TaskStatus::NeedsReview
                && t.max_attempts > 1
                && t.attempts >= t.max_attempts - 1
                && !out.iter().any(|s| s.task.id == t.id)
            {
                let detail = format!("{} of {} attempts used", t.attempts, t.max_attempts);
                out.push(StuckTask {
                    task: t.clone(),
                    reason: "many_retries".into(),
                    detail,
                });
            }
        }

        // Dependency problems: a waiting task whose prerequisite was deleted, or
        // one trapped in a dependency cycle, will never become schedulable and is
        // otherwise invisible — surface both.
        let ids: std::collections::HashSet<&str> =
            all_tasks.iter().map(|t| t.id.as_str()).collect();
        let dep_map: HashMap<&str, &Vec<String>> = all_tasks
            .iter()
            .map(|t| (t.id.as_str(), &t.depends_on))
            .collect();
        for t in &all_tasks {
            // Only tasks still waiting to run can be stuck this way.
            if matches!(
                t.status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
            ) || out.iter().any(|s| s.task.id == t.id)
            {
                continue;
            }
            let missing: Vec<&str> = t
                .depends_on
                .iter()
                .map(|d| d.as_str())
                .filter(|d| !ids.contains(d))
                .collect();
            if !missing.is_empty() {
                out.push(StuckTask {
                    task: t.clone(),
                    reason: "missing_dependency".into(),
                    detail: format!("{} prerequisite(s) no longer exist", missing.len()),
                });
            } else if dep_reaches(&t.id, &t.id, &dep_map) {
                out.push(StuckTask {
                    task: t.clone(),
                    reason: "dependency_cycle".into(),
                    detail: "part of a dependency cycle — it can never start".into(),
                });
            }
        }
        Ok(out)
    }

    /// Validate a task's dependency list before persisting a user edit: each
    /// prerequisite must exist, the task may not depend on itself, and the edit
    /// may not introduce a cycle.
    pub fn validate_task_deps(&self, task: &Task) -> Result<()> {
        if task.depends_on.is_empty() {
            return Ok(());
        }
        let all = self.db.list_tasks(None)?;
        let ids: std::collections::HashSet<&str> = all.iter().map(|t| t.id.as_str()).collect();
        for dep in &task.depends_on {
            if dep == &task.id {
                return Err(CoreError::invalid("a task cannot depend on itself"));
            }
            if !ids.contains(dep.as_str()) {
                return Err(CoreError::invalid(
                    "that prerequisite task no longer exists",
                ));
            }
        }
        // Build the graph with this task's *proposed* deps applied, then check
        // whether the task is reachable from itself (i.e. a cycle).
        let proposed = task.depends_on.clone();
        let mut dep_map: HashMap<&str, &Vec<String>> =
            all.iter().map(|t| (t.id.as_str(), &t.depends_on)).collect();
        dep_map.insert(task.id.as_str(), &proposed);
        if dep_reaches(&task.id, &task.id, &dep_map) {
            return Err(CoreError::invalid("that dependency would create a cycle"));
        }
        Ok(())
    }

    /// Send a sample notification to a webhook to verify it is reachable. Runs
    /// the delivery synchronously and surfaces any failure to the caller.
    pub fn test_webhook(&self, cfg: &crate::config::WebhookConfig) -> Result<()> {
        let mut n = crate::webhook::Notification::new(
            "test",
            "🔔 Test notification",
            "This is a test from Claude Orchestrator.",
        );
        n.project = "example-project".into();
        n.task = "Example task".into();
        n.status = "completed".into();
        crate::webhook::deliver(cfg, &n).map_err(CoreError::Other)
    }

    /// Fire any configured webhooks that want this event for the given project.
    /// Best-effort, async, fire-and-forget (failures are logged).
    fn notify_webhooks(&self, project_id: &str, n: crate::webhook::Notification) {
        let event = n.event.clone();
        let settings = self.db.get_settings().unwrap_or_default();
        let targets: Vec<_> = settings
            .webhooks
            .iter()
            .filter(|w| crate::webhook::wants(w, &event))
            // Empty project list = all projects; otherwise must include this one.
            .filter(|w| w.project_ids.is_empty() || w.project_ids.iter().any(|p| p == project_id))
            .cloned()
            .collect();
        if targets.is_empty() {
            return;
        }
        for cfg in targets {
            let cfg = cfg.clone();
            let n = n.clone();
            let sink = self.sink.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = crate::webhook::deliver(&cfg, &n) {
                    sink.emit(OrchestratorEvent::Log {
                        level: "warn".into(),
                        message: format!("webhook '{}' failed: {e}", cfg.name),
                    });
                }
            });
        }
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
        let project = self.db.get_project(&task.project_id)?;
        let settings = self.db.get_settings()?;
        // Session ran here (its worktree when isolated, else the project root).
        let cwd: PathBuf = if session.branch.is_some() {
            worktree::worktrees_root().join(&session.id)
        } else {
            PathBuf::from(&project.path)
        };

        task.attempts += 1;
        task.updated_at = Utc::now();
        let mut completed = false;

        if outcome.cancelled {
            task.status = TaskStatus::Pending;
            task.attempts = task.attempts.saturating_sub(1);
        } else if !outcome.success {
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
        } else if !(settings.verify_enabled && project.verify_enabled) {
            task.status = TaskStatus::Completed;
            completed = true;
        } else {
            match self
                .run_verification(&project, &task, outcome, &cwd)
                .await?
            {
                Some(v) if v.complete => {
                    task.status = TaskStatus::Completed;
                    completed = true;
                }
                Some(v) => {
                    let feedback = if v.follow_up.trim().is_empty() {
                        v.reason.clone()
                    } else {
                        v.follow_up.clone()
                    };
                    // Record the reviewer's reason as a durable lesson so future
                    // tasks in this project benefit from it.
                    if !v.reason.trim().is_empty() {
                        if let Err(e) = conventions::append_lesson(&project.path, &v.reason) {
                            self.log("warn", format!("could not record lesson: {e}"));
                        }
                    }
                    task.description = append_feedback(&task.description, &feedback);
                    task.status = if task.attempts >= task.max_attempts {
                        TaskStatus::Failed
                    } else {
                        TaskStatus::NeedsReview
                    };
                }
                None => {
                    self.log(
                        "warn",
                        format!(
                            "verifier produced no verdict for task {}; marking complete",
                            task.id
                        ),
                    );
                    task.status = TaskStatus::Completed;
                    completed = true;
                }
            }
        }

        // Apply retry backoff: a task awaiting another attempt waits out an
        // exponential delay before it's eligible again; anything else is cleared.
        if task.status == TaskStatus::NeedsReview {
            task.retry_at = self
                .retry_delay(&settings, task.attempts)
                .map(|d| Utc::now() + d);
            if let Some(at) = task.retry_at {
                self.log(
                    "info",
                    format!(
                        "task '{}' will retry after backoff (attempt {}/{}) at {}",
                        task.title,
                        task.attempts,
                        task.max_attempts,
                        at.to_rfc3339()
                    ),
                );
            }
        } else {
            task.retry_at = None;
        }

        // Commit / PR / clean up the worktree (no-op when not isolated).
        self.finalize_worktree(&project, session, &task, completed);

        // Outbound notifications + activity for terminal outcomes.
        match task.status {
            TaskStatus::Completed => {
                let mut n = crate::webhook::Notification::new(
                    "task_complete",
                    format!("✅ Task completed: {}", task.title),
                    format!("Project {} — {} attempt(s)", project.name, task.attempts),
                );
                n.link = self.db.get_session(&session.id).ok().and_then(|s| s.pr_url);
                n.project = project.name.clone();
                n.task = task.title.clone();
                n.status = "completed".into();
                self.notify_webhooks(&project.id, n);
                self.record_activity(
                    "task",
                    "info",
                    format!("Completed: {}", task.title),
                    Some(&project.id),
                    Some(&task.id),
                    Some(&session.id),
                );
            }
            TaskStatus::Failed => {
                let mut n = crate::webhook::Notification::new(
                    "task_fail",
                    format!("❌ Task failed: {}", task.title),
                    format!(
                        "Project {} — gave up after {} attempt(s)",
                        project.name, task.attempts
                    ),
                );
                n.project = project.name.clone();
                n.task = task.title.clone();
                n.status = "failed".into();
                self.notify_webhooks(&project.id, n);
                self.record_activity(
                    "task",
                    "error",
                    format!("Failed after {} attempt(s): {}", task.attempts, task.title),
                    Some(&project.id),
                    Some(&task.id),
                    Some(&session.id),
                );
            }
            _ => {}
        }

        self.db.upsert_task(&task)?;
        self.emit(OrchestratorEvent::TaskUpdated { task });
        Ok(())
    }

    /// Finalize an isolated task's worktree: on completion, optionally commit and
    /// open a PR; always remove the worktree and drop empty branches.
    fn finalize_worktree(
        &self,
        project: &Project,
        session: &Session,
        task: &Task,
        completed: bool,
    ) {
        let Some(branch) = session.branch.clone() else {
            return; // not isolated
        };
        let repo = PathBuf::from(&project.path);
        let wt = worktree::worktrees_root().join(&session.id);
        let settings = self.db.get_settings().unwrap_or_default();
        let mut committed = false;
        let mut pr_url = None;

        if completed && settings.auto_commit {
            match worktree::commit_all(&wt, &format!("{}\n\nBy Claude Orchestrator.", task.title)) {
                Ok(Some(hash)) => {
                    committed = true;
                    self.log("info", format!("committed {hash} on {branch}"));
                }
                Ok(None) => {}
                Err(e) => self.log("warn", format!("commit failed: {e}")),
            }
            if committed && settings.auto_pr {
                if let Err(e) = worktree::push(&wt, &branch) {
                    self.log("warn", format!("push failed: {e}"));
                } else {
                    let base = worktree::current_branch(&repo).unwrap_or_else(|| "main".into());
                    let body = format!(
                        "Automated PR for task: {}\n\nOpened by Claude Orchestrator.",
                        task.title
                    );
                    match worktree::open_pr(&wt, &task.title, &body, &base) {
                        Ok(Some(url)) => {
                            self.log("info", format!("opened PR {url}"));
                            pr_url = Some(url);
                        }
                        _ => self.log("info", "PR not opened (gh unavailable or no remote)"),
                    }
                }
            }
        }

        // Persist branch/PR (or clear the branch if nothing was committed).
        if let Ok(mut row) = self.db.get_session(&session.id) {
            if pr_url.is_some() {
                row.pr_url = pr_url;
            }
            if !committed {
                row.branch = None;
            }
            let _ = self.db.upsert_session(&row);
            self.emit(OrchestratorEvent::SessionUpdated { session: row });
        }

        let _ = worktree::remove(&repo, &wt);
        if !committed {
            worktree::delete_branch(&repo, &branch);
        }
    }

    async fn run_verification(
        &self,
        project: &Project,
        task: &Task,
        task_outcome: &RunOutcome,
        cwd: &Path,
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
        let mut spec = self.run_spec(project, project.default_agent, &prompt, None);
        spec.cwd = cwd.to_path_buf(); // verify in the same worktree to see changes
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
        let settings = self.db.get_settings()?;
        let now = Utc::now();

        // Dedup against the project's open (non-terminal) tasks so the roadmap
        // never re-queues work already in flight. Normalize titles for matching.
        let norm = |s: &str| s.trim().to_lowercase();
        let mut open_titles: std::collections::HashSet<String> = self
            .db
            .list_tasks(Some(&project.id))?
            .iter()
            .filter(|t| {
                !matches!(
                    t.status,
                    TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
                )
            })
            .map(|t| norm(&t.title))
            .collect();

        // Cap the pending backlog the roadmap is allowed to fill (0 = unlimited).
        let mut budget = if settings.roadmap_max_pending > 0 {
            settings
                .roadmap_max_pending
                .saturating_sub(self.db.count_pending_tasks_for_project(&project.id)?)
        } else {
            u32::MAX
        };

        let mut created = 0;
        let mut skipped = 0;
        for spec in specs {
            if budget == 0 {
                skipped += 1;
                continue;
            }
            // Skip duplicates of work already open (or already created this batch).
            if !open_titles.insert(norm(&spec.title)) {
                skipped += 1;
                continue;
            }
            // Respect an explicitly-requested agent only if the project allows it;
            // otherwise leave it auto so the scheduler load-balances.
            let requested = spec.agent.as_deref().and_then(AgentKind::from_str);
            let (agent, auto_agent) = match requested {
                Some(a) if project.allows(a) => (a, false),
                _ => (project.default_agent, true),
            };
            budget -= 1;
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
                max_attempts: project.effective_max_attempts(),
                tags: spec.tags.clone(),
                auto_generated: true,
                retry_at: None,
                notes: None,
                created_at: now,
                updated_at: now,
            };
            self.db.upsert_task(&task)?;
            self.emit(OrchestratorEvent::TaskUpdated { task });
            created += 1;
        }
        let skip_note = if skipped > 0 {
            format!(" (skipped {skipped} duplicate/over-cap)")
        } else {
            String::new()
        };
        self.log(
            "info",
            format!(
                "roadmap created {created} task(s) for {}{skip_note}",
                project.name
            ),
        );
        if created > 0 {
            self.record_activity(
                "roadmap",
                "info",
                format!("Roadmap generated {created} task(s)"),
                Some(&project.id),
                None,
                Some(&session.id),
            );
        }
        Ok(())
    }
}

/// Keep only the newest `keep` `orchestrator-config-*.json` backups in `dir`.
fn prune_backups(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("orchestrator-config-") && n.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();
    // Timestamped names sort lexicographically by time; newest last.
    files.sort();
    if files.len() > keep {
        for old in &files[..files.len() - keep] {
            let _ = std::fs::remove_file(old);
        }
    }
}

/// True if `target` is reachable by following dependency edges out of `start`.
/// With `start == target` this answers "is `start` part of a cycle?". The graph
/// is `task_id -> depends_on`; unknown ids are simply leaves.
fn dep_reaches(start: &str, target: &str, dep_map: &HashMap<&str, &Vec<String>>) -> bool {
    let mut stack: Vec<&str> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    if let Some(deps) = dep_map.get(start) {
        stack.extend(deps.iter().map(|d| d.as_str()));
    }
    while let Some(cur) = stack.pop() {
        if cur == target {
            return true;
        }
        if !seen.insert(cur) {
            continue;
        }
        if let Some(deps) = dep_map.get(cur) {
            stack.extend(deps.iter().map(|d| d.as_str()));
        }
    }
    false
}

/// Round-robin slot allocation: given each project's available capacity and the
/// number of free global slots, return the order in which projects should each
/// start one task at a time. Slots are shared one-per-project-per-round so no
/// single project drains the fleet just because it sorts first. Pure (no I/O)
/// so the fairness policy is unit-testable.
fn round_robin_allocation(caps: &[(String, u32)], global_slots: u32) -> Vec<String> {
    let mut remaining: Vec<(String, u32)> = caps.iter().filter(|(_, c)| *c > 0).cloned().collect();
    let mut out = Vec::new();
    let mut slots = global_slots;
    while slots > 0 && !remaining.is_empty() {
        for (id, cap) in remaining.iter_mut() {
            if slots == 0 {
                break;
            }
            out.push(id.clone());
            *cap -= 1;
            slots -= 1;
        }
        remaining.retain(|(_, c)| *c > 0);
    }
    out
}

/// A task's effective scheduling priority: its base priority plus an
/// anti-starvation bonus that grows with how long it has waited. With aging off
/// (`aging_per_hour <= 0`) this is just the base priority.
fn effective_priority(task: &Task, now: DateTime<Utc>, aging_per_hour: f64) -> f64 {
    let base = task.priority as f64;
    if aging_per_hour <= 0.0 {
        return base;
    }
    let waited_hours = (now - task.created_at).num_seconds().max(0) as f64 / 3600.0;
    base + aging_per_hour * waited_hours
}

/// Build a one-line, ~160-char context snippet around the first case-insensitive
/// occurrence of `needle` (already lowercased) in `text`. Returns None if absent.
fn snippet_of(text: Option<&str>, needle: &str) -> Option<String> {
    let text = text?;
    let pos = text.to_lowercase().find(needle)?;
    // Work on char boundaries so multi-byte text never panics.
    let start = text[..pos]
        .char_indices()
        .rev()
        .take(60)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(pos);
    let end_budget = pos + needle.len() + 100;
    let end = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .find(|&i| i >= end_budget)
        .unwrap_or(text.len());
    let mut snip = text[start..end]
        .replace(['\n', '\r'], " ")
        .trim()
        .to_string();
    if start > 0 {
        snip.insert(0, '…');
    }
    if end < text.len() {
        snip.push('…');
    }
    Some(snip)
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
            default_max_attempts: None,
            mcp_config: None,
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
            retry_at: None,
            notes: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn diagnostics_reports_core_categories() {
        let eng = engine();
        let diags = eng.diagnostics().unwrap();
        // One line per agent, plus git and database.
        assert!(diags.iter().any(|d| d.category == "git"));
        let db = diags.iter().find(|d| d.category == "database").unwrap();
        assert_eq!(db.level, "ok", "in-memory db must be writable");
        assert_eq!(
            diags.iter().filter(|d| d.category == "agent").count(),
            AgentKind::ALL.len()
        );
        // Every finding carries a known severity.
        assert!(diags
            .iter()
            .all(|d| ["ok", "warn", "error"].contains(&d.level.as_str())));
    }

    #[test]
    fn search_sessions_finds_content_and_snippets() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        let mut s = eng.new_session(
            &p,
            None,
            AgentKind::Claude,
            SessionKind::Task,
            "implement the widget exporter",
        );
        s.result_text = Some("Added a CSV exporter and tests for the widget pipeline".into());
        eng.db.upsert_session(&s).unwrap();

        let hits = eng.search_sessions("exporter", None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].matched_in, "result");
        assert!(hits[0].snippet.to_lowercase().contains("exporter"));

        // Matches the prompt too, and project filtering works.
        assert_eq!(eng.search_sessions("widget", Some(&p.id)).unwrap().len(), 1);
        assert_eq!(
            eng.search_sessions("widget", Some("other")).unwrap().len(),
            0
        );
        // Empty query yields nothing.
        assert!(eng.search_sessions("   ", None).unwrap().is_empty());
        // No false positives.
        assert!(eng
            .search_sessions("nonexistent-token", None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn export_task_transcript_includes_sessions() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        let task = crate::service::create_task(
            &eng.db,
            crate::service::CreateTaskInput {
                project_id: p.id.clone(),
                title: "Build it".into(),
                description: String::new(),
                priority: None,
                agent: None,
                model: None,
                depends_on: vec![],
                tags: vec![],
                max_attempts: None,
            },
        )
        .unwrap();
        let mut s = eng.new_session(
            &p,
            Some(&task),
            AgentKind::Claude,
            SessionKind::Task,
            "do the thing",
        );
        s.result_text = Some("done".into());
        eng.db.upsert_session(&s).unwrap();

        let md = eng.export_task_transcript(&task.id).unwrap();
        assert!(md.contains("# Transcript: Build it"));
        assert!(md.contains("1 session(s)"));
        assert!(md.contains("### Prompt"));
        assert!(md.contains("do the thing"));
    }

    #[test]
    fn purge_tasks_removes_only_matching_statuses() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        let mk = |status: TaskStatus| {
            let mut t = mk_task(&p.id, AgentKind::Claude, false);
            t.status = status;
            eng.db.upsert_task(&t).unwrap();
        };
        mk(TaskStatus::Completed);
        mk(TaskStatus::Completed);
        mk(TaskStatus::Cancelled);
        mk(TaskStatus::Pending);

        let removed = eng
            .db
            .purge_tasks(Some(&p.id), &[TaskStatus::Completed, TaskStatus::Cancelled])
            .unwrap();
        assert_eq!(removed, 3);
        let left = eng.db.list_tasks(Some(&p.id)).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].status, TaskStatus::Pending);
        // Empty status list is a no-op.
        assert_eq!(eng.db.purge_tasks(None, &[]).unwrap(), 0);
    }

    #[test]
    fn round_robin_shares_slots_fairly() {
        // A has lots of work, B has one task; with 3 global slots, B must still
        // get its turn instead of A taking everything.
        let caps = vec![("a".to_string(), 5), ("b".to_string(), 1)];
        let order = round_robin_allocation(&caps, 3);
        assert_eq!(order, vec!["a", "b", "a"]);
        assert_eq!(order.iter().filter(|x| *x == "b").count(), 1);

        // Capacity is respected: never hand a project more than it can take.
        let order = round_robin_allocation(&[("a".to_string(), 1), ("b".to_string(), 1)], 10);
        assert_eq!(order, vec!["a", "b"]);

        // Zero-capacity projects are skipped; zero slots yields nothing.
        assert!(round_robin_allocation(&[("a".to_string(), 0)], 5).is_empty());
        assert!(round_robin_allocation(&[("a".to_string(), 3)], 0).is_empty());
    }

    #[test]
    fn priority_aging_lets_old_low_priority_win() {
        let now = Utc::now();
        let mut high = mk_task("p", AgentKind::Claude, false);
        high.priority = 100;
        high.created_at = now; // fresh
        let mut low = mk_task("p", AgentKind::Claude, false);
        low.priority = 50;
        low.created_at = now - chrono::Duration::hours(40); // waited a long time

        // Aging off: base priority decides — high wins.
        assert!(effective_priority(&high, now, 0.0) > effective_priority(&low, now, 0.0));
        // Aging at +2/hr: 50 + 2*40 = 130 > 100, so the aged low-priority task wins.
        assert!(effective_priority(&low, now, 2.0) > effective_priority(&high, now, 2.0));
    }

    #[test]
    fn upcoming_queue_orders_by_effective_priority() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        let mut a = mk_task(&p.id, AgentKind::Claude, false);
        a.title = "high".into();
        a.priority = 100;
        let mut b = mk_task(&p.id, AgentKind::Claude, false);
        b.title = "low".into();
        b.priority = 0;
        eng.db.upsert_task(&a).unwrap();
        eng.db.upsert_task(&b).unwrap();

        let q = eng.upcoming_queue(10).unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].task.title, "high");
        assert_eq!(q[0].project_name, "p");
    }

    fn roadmap_outcome(text: &str) -> crate::runner::RunOutcome {
        crate::runner::RunOutcome {
            success: true,
            agent_session_id: None,
            model: None,
            result_text: Some(text.to_string()),
            usage: TokenUsage::default(),
            exit_code: Some(0),
            error: None,
            cancelled: false,
            timed_out: false,
        }
    }

    #[test]
    fn roadmap_dedups_open_tasks_and_respects_cap() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        // An open task the roadmap will try to duplicate.
        let mut existing = mk_task(&p.id, AgentKind::Claude, false);
        existing.title = "Add logging".into();
        existing.status = TaskStatus::Pending;
        eng.db.upsert_task(&existing).unwrap();

        let session = eng.new_session(&p, None, AgentKind::Claude, SessionKind::Roadmap, "plan");
        let batch = "```json\n[{\"title\":\"Add logging\"},{\"title\":\"Write docs\"},{\"title\":\"Refactor parser\"}]\n```";
        eng.handle_roadmap_outcome(&session, &roadmap_outcome(batch))
            .unwrap();

        // Duplicate of "Add logging" skipped; two new tasks created.
        let tasks = eng.db.list_tasks(Some(&p.id)).unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks.iter().filter(|t| t.auto_generated).count(), 2);
    }

    #[test]
    fn roadmap_cap_limits_new_tasks() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        let mut s = eng.db.get_settings().unwrap();
        s.roadmap_max_pending = 2;
        eng.db.save_settings(&s).unwrap();

        let session = eng.new_session(&p, None, AgentKind::Claude, SessionKind::Roadmap, "plan");
        let batch = "```json\n[{\"title\":\"a\"},{\"title\":\"b\"},{\"title\":\"c\"},{\"title\":\"d\"}]\n```";
        eng.handle_roadmap_outcome(&session, &roadmap_outcome(batch))
            .unwrap();
        // Cap of 2 pending honored.
        assert_eq!(eng.db.count_pending_tasks_for_project(&p.id).unwrap(), 2);
    }

    #[test]
    fn project_analytics_aggregates_sessions() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        let mk_sess = |agent, status, cost| {
            let mut s = eng.new_session(&p, None, agent, SessionKind::Task, "x");
            s.status = status;
            s.usage.total_cost_usd = cost;
            s.usage.input_tokens = 100;
            s.usage.output_tokens = 50;
            eng.db.upsert_session(&s).unwrap();
        };
        mk_sess(AgentKind::Claude, SessionStatus::Completed, 0.5);
        mk_sess(AgentKind::Claude, SessionStatus::Completed, 0.3);
        mk_sess(AgentKind::Claude, SessionStatus::Failed, 0.2);
        // A still-running session must be excluded from the finished stats.
        mk_sess(AgentKind::Claude, SessionStatus::Running, 9.9);

        let a = eng.project_analytics(&p.id, 14).unwrap();
        assert_eq!(a.stats.sessions, 3);
        assert_eq!(a.stats.completed, 2);
        assert_eq!(a.stats.failed, 1);
        assert!((a.stats.success_rate - 2.0 / 3.0).abs() < 1e-9);
        assert!((a.stats.total_cost_usd - 1.0).abs() < 1e-9);
        assert_eq!(a.stats.total_tokens, 450);
        assert_eq!(a.by_agent.len(), 1);
        assert_eq!(a.by_agent[0].sessions, 3);
    }

    #[test]
    fn stuck_tasks_flags_cycles_and_missing_deps() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        // A 2-cycle: a depends on b, b depends on a.
        let mut a = mk_task(&p.id, AgentKind::Claude, false);
        let mut b = mk_task(&p.id, AgentKind::Claude, false);
        a.depends_on = vec![b.id.clone()];
        b.depends_on = vec![a.id.clone()];
        eng.db.upsert_task(&a).unwrap();
        eng.db.upsert_task(&b).unwrap();
        // A task pointing at a non-existent prerequisite.
        let mut orphan = mk_task(&p.id, AgentKind::Claude, false);
        orphan.depends_on = vec!["ghost-id".into()];
        eng.db.upsert_task(&orphan).unwrap();

        let stuck = eng.stuck_tasks().unwrap();
        assert_eq!(
            stuck
                .iter()
                .filter(|s| s.reason == "dependency_cycle")
                .count(),
            2
        );
        assert!(stuck
            .iter()
            .any(|s| s.reason == "missing_dependency" && s.task.id == orphan.id));
    }

    #[test]
    fn validate_task_deps_rejects_cycles_and_self() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        let a = mk_task(&p.id, AgentKind::Claude, false);
        let mut b = mk_task(&p.id, AgentKind::Claude, false);
        b.depends_on = vec![a.id.clone()];
        eng.db.upsert_task(&a).unwrap();
        eng.db.upsert_task(&b).unwrap();

        // a depends on b would close the loop a->b->a.
        let mut a_cycle = a.clone();
        a_cycle.depends_on = vec![b.id.clone()];
        assert!(eng.validate_task_deps(&a_cycle).is_err());

        // Self-dependency is rejected.
        let mut a_self = a.clone();
        a_self.depends_on = vec![a.id.clone()];
        assert!(eng.validate_task_deps(&a_self).is_err());

        // A missing prerequisite is rejected.
        let mut a_missing = a.clone();
        a_missing.depends_on = vec!["nope".into()];
        assert!(eng.validate_task_deps(&a_missing).is_err());

        // A valid forward dependency is accepted.
        let mut a_ok = a.clone();
        a_ok.depends_on = vec![];
        assert!(eng.validate_task_deps(&a_ok).is_ok());
    }

    #[test]
    fn task_notes_round_trip() {
        let eng = engine();
        let p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        let mut t = mk_task(&p.id, AgentKind::Claude, false);
        t.notes = Some("ask the user about the API shape".into());
        eng.db.upsert_task(&t).unwrap();
        let got = eng.db.get_task(&t.id).unwrap();
        assert_eq!(
            got.notes.as_deref(),
            Some("ask the user about the API shape")
        );
    }

    #[test]
    fn diagnostics_flags_missing_project_path() {
        let eng = engine();
        mk_project(&eng, "/no/such/path/xyz", vec![AgentKind::Claude]);
        let diags = eng.diagnostics().unwrap();
        assert!(diags
            .iter()
            .any(|d| d.category == "project" && d.level == "error" && d.detail.contains("path")));
    }

    #[test]
    fn run_spec_carries_project_mcp_config() {
        let eng = engine();
        let mut p = mk_project(&eng, "/tmp/p", vec![AgentKind::Claude]);
        // No MCP config → None.
        let spec = eng.run_spec(&p, AgentKind::Claude, "do it", None);
        assert!(spec.mcp_config.is_none());
        // A configured path flows through to the spec.
        p.mcp_config = Some("/tmp/p/.mcp.json".into());
        let spec = eng.run_spec(&p, AgentKind::Claude, "do it", None);
        assert_eq!(
            spec.mcp_config.as_deref(),
            Some(Path::new("/tmp/p/.mcp.json"))
        );
        // Whitespace-only is treated as unset.
        p.mcp_config = Some("   ".into());
        let spec = eng.run_spec(&p, AgentKind::Claude, "do it", None);
        assert!(spec.mcp_config.is_none());
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
    fn backup_writes_and_prunes() {
        let eng = engine();
        let dir = std::env::temp_dir().join(format!("orch-bk-{}", Uuid::new_v4()));
        let mut s = eng.db.get_settings().unwrap();
        s.backup_dir = dir.to_string_lossy().into_owned();
        eng.db.save_settings(&s).unwrap();

        // Write more than the retention to exercise pruning.
        for i in 0..12 {
            // Distinct names: backup_now uses second precision, so stamp manually.
            let bundle = crate::service::export_config(&eng.db).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("orchestrator-config-2026010100000{i}.json")),
                serde_json::to_string(&bundle).unwrap(),
            )
            .unwrap();
        }
        super::prune_backups(&dir, 10);
        let count = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, 10);

        // A real backup writes a file and succeeds.
        let path = eng.backup_now().unwrap();
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backup_without_dir_errors() {
        let eng = engine();
        assert!(eng.backup_now().is_err());
    }

    #[test]
    fn backoff_skips_task_until_due() {
        let eng = engine();
        let tmp = std::env::temp_dir();
        let p = mk_project(&eng, &tmp.to_string_lossy(), vec![AgentKind::Claude]);
        let mut t = mk_task(&p.id, AgentKind::Claude, true);
        t.status = TaskStatus::NeedsReview;
        t.attempts = 1;
        t.retry_at = Some(Utc::now() + chrono::Duration::hours(1));
        eng.db.upsert_task(&t).unwrap();
        // Backing off: not eligible, and recognized as waiting (suppresses roadmap).
        assert!(eng.eligible_tasks(&p.id, 0.0).unwrap().is_empty());
        assert!(eng.has_backing_off_tasks(&p.id).unwrap());
        // Once due, it's eligible again.
        t.retry_at = Some(Utc::now() - chrono::Duration::minutes(1));
        eng.db.upsert_task(&t).unwrap();
        assert!(!eng.eligible_tasks(&p.id, 0.0).unwrap().is_empty());
        assert!(!eng.has_backing_off_tasks(&p.id).unwrap());
    }

    #[test]
    fn retry_delay_is_exponential_and_capped() {
        let eng = engine();
        let mut s = Settings {
            retry_enabled: true,
            retry_base_secs: 60,
            retry_max_secs: 600,
            ..Settings::default()
        };
        assert_eq!(eng.retry_delay(&s, 1).unwrap().num_seconds(), 60);
        assert_eq!(eng.retry_delay(&s, 2).unwrap().num_seconds(), 120);
        assert_eq!(eng.retry_delay(&s, 3).unwrap().num_seconds(), 240);
        assert_eq!(eng.retry_delay(&s, 10).unwrap().num_seconds(), 600); // capped
        s.retry_enabled = false;
        assert!(eng.retry_delay(&s, 1).is_none());
    }

    #[test]
    fn scheduled_tasks_are_not_retried() {
        let eng = engine();
        let dir = std::env::temp_dir().join(format!("orch-noretry-{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".orchestrator/scheduled")).unwrap();
        std::fs::write(
            dir.join(".orchestrator/scheduled/job.md"),
            "---\nevery: 1h\ntitle: Nightly\n---\nDo the thing.",
        )
        .unwrap();
        let p = mk_project(&eng, &dir.to_string_lossy(), vec![AgentKind::Claude]);
        eng.discover_all_scheduled().unwrap();
        let sched = eng.db.list_scheduled(Some(&p.id)).unwrap();
        let past = Utc::now() - chrono::Duration::minutes(1);
        eng.db
            .set_scheduled_run(&sched[0].id, past, Some(past))
            .unwrap();
        eng.fire_due_scheduled().unwrap();
        let tasks = eng.db.list_tasks(Some(&p.id)).unwrap();
        // Scheduled tasks get a single attempt — one failure is terminal.
        assert_eq!(tasks[0].max_attempts, 1);
        std::fs::remove_dir_all(&dir).ok();
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

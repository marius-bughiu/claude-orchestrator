//! SQLite persistence layer built on rusqlite.
//!
//! A single connection is guarded by a mutex. All methods are synchronous; async
//! callers (the engine, Tauri commands) wrap them in `spawn_blocking` when needed.
//! Queries use the runtime API (no compile-time DB), so the crate builds anywhere.

use crate::config::Settings;
use crate::error::{CoreError, Result};
use crate::models::*;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;
use std::sync::{Arc, Mutex};

const SCHEMA: &str = include_str!("schema.sql");

/// Additive column migrations for databases created before a column existed.
/// Each is idempotent: on a fresh DB the column already exists and the ALTER
/// fails with "duplicate column", which we ignore.
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE projects ADD COLUMN allowed_agents TEXT NOT NULL DEFAULT '[\"claude\"]'",
    "ALTER TABLE tasks ADD COLUMN auto_agent INTEGER NOT NULL DEFAULT 1",
    "ALTER TABLE tasks ADD COLUMN model TEXT",
];

fn run_migrations(conn: &Connection) {
    for stmt in MIGRATIONS {
        // Ignore "duplicate column" (already applied) and similar idempotent errors.
        let _ = conn.execute(stmt, []);
    }
}

/// Thread-safe handle to the orchestrator database.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open (or create) a database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Db> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    /// Open an in-memory database (used in tests).
    pub fn open_in_memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Db> {
        conn.execute_batch(SCHEMA)?;
        run_migrations(&conn);
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db mutex poisoned")
    }

    // ---- Settings -----------------------------------------------------------

    pub fn get_settings(&self) -> Result<Settings> {
        let conn = self.lock();
        let json: Option<String> = conn
            .query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0))
            .optional()?;
        match json {
            Some(j) => Ok(serde_json::from_str(&j)?),
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let json = serde_json::to_string(settings)?;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO settings (id, json) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET json = excluded.json",
            params![json],
        )?;
        Ok(())
    }

    // ---- Projects -----------------------------------------------------------

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT * FROM projects ORDER BY name COLLATE NOCASE")?;
        let rows = stmt
            .query_map([], map_project)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_project(&self, id: &str) -> Result<Project> {
        let conn = self.lock();
        conn.query_row(
            "SELECT * FROM projects WHERE id = ?1",
            params![id],
            map_project,
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("project {id}")))
    }

    pub fn upsert_project(&self, p: &Project) -> Result<()> {
        let allowed = serde_json::to_string(&p.allowed_agents)?;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO projects
               (id, name, path, description, enabled, default_agent, allowed_agents,
                max_concurrent, roadmap_enabled, verify_enabled, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(id) DO UPDATE SET
               name=excluded.name, path=excluded.path, description=excluded.description,
               enabled=excluded.enabled, default_agent=excluded.default_agent,
               allowed_agents=excluded.allowed_agents,
               max_concurrent=excluded.max_concurrent, roadmap_enabled=excluded.roadmap_enabled,
               verify_enabled=excluded.verify_enabled, updated_at=excluded.updated_at",
            params![
                p.id,
                p.name,
                p.path,
                p.description,
                p.enabled,
                p.default_agent.as_str(),
                allowed,
                p.max_concurrent,
                p.roadmap_enabled,
                p.verify_enabled,
                p.created_at,
                p.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        self.lock()
            .execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn count_projects(&self) -> Result<u32> {
        let conn = self.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?;
        Ok(n as u32)
    }

    // ---- Tasks --------------------------------------------------------------

    pub fn list_tasks(&self, project_id: Option<&str>) -> Result<Vec<Task>> {
        let conn = self.lock();
        let (sql, has_filter) = match project_id {
            Some(_) => (
                "SELECT * FROM tasks WHERE project_id = ?1 ORDER BY priority DESC, created_at ASC",
                true,
            ),
            None => (
                "SELECT * FROM tasks ORDER BY priority DESC, created_at ASC",
                false,
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = if has_filter {
            stmt.query_map(params![project_id.unwrap()], map_task)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], map_task)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    pub fn get_task(&self, id: &str) -> Result<Task> {
        let conn = self.lock();
        conn.query_row("SELECT * FROM tasks WHERE id = ?1", params![id], map_task)
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("task {id}")))
    }

    pub fn upsert_task(&self, t: &Task) -> Result<()> {
        let depends_on = serde_json::to_string(&t.depends_on)?;
        let tags = serde_json::to_string(&t.tags)?;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO tasks
               (id, project_id, title, description, status, priority, agent, auto_agent, model,
                parent_id, depends_on, attempts, max_attempts, tags, auto_generated,
                created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT(id) DO UPDATE SET
               title=excluded.title, description=excluded.description, status=excluded.status,
               priority=excluded.priority, agent=excluded.agent, auto_agent=excluded.auto_agent,
               model=excluded.model, parent_id=excluded.parent_id,
               depends_on=excluded.depends_on, attempts=excluded.attempts,
               max_attempts=excluded.max_attempts, tags=excluded.tags,
               auto_generated=excluded.auto_generated, updated_at=excluded.updated_at",
            params![
                t.id,
                t.project_id,
                t.title,
                t.description,
                t.status.as_str(),
                t.priority,
                t.agent.as_str(),
                t.auto_agent,
                t.model,
                t.parent_id,
                depends_on,
                t.attempts,
                t.max_attempts,
                tags,
                t.auto_generated,
                t.created_at,
                t.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn delete_task(&self, id: &str) -> Result<()> {
        self.lock()
            .execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Count tasks that are pending or awaiting retry for a project.
    pub fn count_schedulable_tasks(&self, project_id: &str) -> Result<u32> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE project_id = ?1 AND status IN ('pending','needs_review')",
            params![project_id],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    pub fn count_pending_tasks(&self) -> Result<u32> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status IN ('pending','needs_review')",
            [],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    /// Schedulable tasks for a project, highest priority first. Dependency
    /// satisfaction is checked by the engine.
    pub fn schedulable_tasks(&self, project_id: &str) -> Result<Vec<Task>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT * FROM tasks
             WHERE project_id = ?1 AND status IN ('pending','needs_review')
             ORDER BY priority DESC, created_at ASC",
        )?;
        let rows = stmt
            .query_map(params![project_id], map_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- Sessions -----------------------------------------------------------

    pub fn list_sessions(
        &self,
        task_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<Vec<Session>> {
        let conn = self.lock();
        let mut sql = String::from("SELECT * FROM sessions");
        let mut clauses = Vec::new();
        if task_id.is_some() {
            clauses.push("task_id = :task");
        }
        if project_id.is_some() {
            clauses.push("project_id = :project");
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::named_params! { ":task": task_id, ":project": project_id },
                map_session,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_session(&self, id: &str) -> Result<Session> {
        let conn = self.lock();
        conn.query_row(
            "SELECT * FROM sessions WHERE id = ?1",
            params![id],
            map_session,
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("session {id}")))
    }

    pub fn upsert_session(&self, s: &Session) -> Result<()> {
        let usage = serde_json::to_string(&s.usage)?;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO sessions
               (id, task_id, project_id, agent, kind, status, agent_session_id, model,
                prompt, result_text, error, exit_code, usage, started_at, ended_at, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(id) DO UPDATE SET
               task_id=excluded.task_id, status=excluded.status,
               agent_session_id=excluded.agent_session_id, model=excluded.model,
               result_text=excluded.result_text, error=excluded.error,
               exit_code=excluded.exit_code, usage=excluded.usage,
               started_at=excluded.started_at, ended_at=excluded.ended_at",
            params![
                s.id,
                s.task_id,
                s.project_id,
                s.agent.as_str(),
                s.kind.as_str(),
                s.status.as_str(),
                s.agent_session_id,
                s.model,
                s.prompt,
                s.result_text,
                s.error,
                s.exit_code,
                usage,
                s.started_at,
                s.ended_at,
                s.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn active_sessions(&self) -> Result<Vec<Session>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT * FROM sessions WHERE status IN ('pending','running') ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([], map_session)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn count_active_sessions(&self) -> Result<u32> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE status IN ('pending','running')",
            [],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    pub fn count_active_sessions_for_project(&self, project_id: &str) -> Result<u32> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE project_id = ?1 AND status IN ('pending','running')",
            params![project_id],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    pub fn count_active_sessions_for_agent(&self, agent: AgentKind) -> Result<u32> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE agent = ?1 AND status IN ('pending','running')",
            params![agent.as_str()],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    /// Reset any sessions left "running" by a previous process crash to failed.
    pub fn reconcile_orphan_sessions(&self) -> Result<u32> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE sessions SET status='failed', error='orphaned (app restarted)'
             WHERE status IN ('pending','running')",
            [],
        )?;
        Ok(n as u32)
    }

    // ---- Session events -----------------------------------------------------

    pub fn insert_event(
        &self,
        session_id: &str,
        kind: &str,
        text: Option<&str>,
        data: Option<&serde_json::Value>,
        created_at: DateTime<Utc>,
    ) -> Result<SessionEvent> {
        let data_str = data.map(|d| d.to_string());
        let conn = self.lock();
        conn.execute(
            "INSERT INTO session_events (session_id, kind, text, data, created_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![session_id, kind, text, data_str, created_at],
        )?;
        let id = conn.last_insert_rowid();
        Ok(SessionEvent {
            id,
            session_id: session_id.to_string(),
            kind: kind.to_string(),
            text: text.map(String::from),
            data: data.cloned(),
            created_at,
        })
    }

    pub fn list_events(&self, session_id: &str) -> Result<Vec<SessionEvent>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT * FROM session_events WHERE session_id = ?1 ORDER BY id ASC")?;
        let rows = stmt
            .query_map(params![session_id], map_event)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- Usage --------------------------------------------------------------

    pub fn insert_usage(
        &self,
        id: &str,
        agent: AgentKind,
        session_id: Option<&str>,
        usage: &TokenUsage,
        created_at: DateTime<Utc>,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO usage_records
               (id, agent, session_id, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, cost_usd, num_turns, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                id,
                agent.as_str(),
                session_id,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
                usage.total_cost_usd,
                usage.num_turns,
                created_at,
            ],
        )?;
        Ok(())
    }

    /// Aggregate usage for an agent since `since` (None = all time).
    pub fn usage_for_agent(
        &self,
        agent: AgentKind,
        since: Option<DateTime<Utc>>,
    ) -> Result<TokenUsage> {
        let conn = self.lock();
        let sql = "SELECT
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_creation_tokens),0),
                COALESCE(SUM(cost_usd),0.0), COALESCE(SUM(num_turns),0)
             FROM usage_records
             WHERE agent = ?1 AND (?2 IS NULL OR created_at >= ?2)";
        conn.query_row(sql, params![agent.as_str(), since], |r| {
            Ok(TokenUsage {
                input_tokens: r.get::<_, i64>(0)? as u64,
                output_tokens: r.get::<_, i64>(1)? as u64,
                cache_read_tokens: r.get::<_, i64>(2)? as u64,
                cache_creation_tokens: r.get::<_, i64>(3)? as u64,
                total_cost_usd: r.get(4)?,
                num_turns: r.get::<_, i64>(5)? as u32,
            })
        })
        .map_err(Into::into)
    }

    // ---- Scheduled tasks ----------------------------------------------------

    pub fn upsert_scheduled(&self, s: &ScheduledTask) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO scheduled_tasks
               (id, project_id, path, rel_path, title, schedule, schedule_kind, schedule_desc,
                agent, model, priority, enabled, valid, error, body, last_run, next_run,
                created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
             ON CONFLICT(id) DO UPDATE SET
               path=excluded.path, rel_path=excluded.rel_path, title=excluded.title,
               schedule=excluded.schedule, schedule_kind=excluded.schedule_kind,
               schedule_desc=excluded.schedule_desc, agent=excluded.agent, model=excluded.model,
               priority=excluded.priority, enabled=excluded.enabled, valid=excluded.valid,
               error=excluded.error, body=excluded.body, next_run=excluded.next_run,
               updated_at=excluded.updated_at",
            params![
                s.id,
                s.project_id,
                s.path,
                s.rel_path,
                s.title,
                s.schedule,
                s.schedule_kind,
                s.schedule_desc,
                s.agent.map(|a| a.as_str()),
                s.model,
                s.priority,
                s.enabled,
                s.valid,
                s.error,
                s.body,
                s.last_run,
                s.next_run,
                s.created_at,
                s.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_scheduled(&self, project_id: Option<&str>) -> Result<Vec<ScheduledTask>> {
        let conn = self.lock();
        let (sql, filtered) = match project_id {
            Some(_) => (
                "SELECT * FROM scheduled_tasks WHERE project_id = ?1 ORDER BY next_run ASC",
                true,
            ),
            None => ("SELECT * FROM scheduled_tasks ORDER BY next_run ASC", false),
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = if filtered {
            stmt.query_map(params![project_id.unwrap()], map_scheduled)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], map_scheduled)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    pub fn get_scheduled(&self, id: &str) -> Result<Option<ScheduledTask>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT * FROM scheduled_tasks WHERE id = ?1",
                params![id],
                map_scheduled,
            )
            .optional()?)
    }

    /// Scheduled tasks that are enabled, valid, and due (next_run <= now).
    pub fn due_scheduled(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledTask>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT * FROM scheduled_tasks
             WHERE enabled = 1 AND valid = 1 AND next_run IS NOT NULL AND next_run <= ?1
             ORDER BY next_run ASC",
        )?;
        let rows = stmt
            .query_map(params![now], map_scheduled)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_scheduled_run(
        &self,
        id: &str,
        last_run: DateTime<Utc>,
        next_run: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.lock().execute(
            "UPDATE scheduled_tasks SET last_run = ?2, next_run = ?3 WHERE id = ?1",
            params![id, last_run, next_run],
        )?;
        Ok(())
    }

    pub fn set_scheduled_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        self.lock().execute(
            "UPDATE scheduled_tasks SET enabled = ?2 WHERE id = ?1",
            params![id, enabled],
        )?;
        Ok(())
    }

    /// Remove scheduled rows for a project whose ids are not in `keep`.
    pub fn prune_scheduled(&self, project_id: &str, keep: &[String]) -> Result<()> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT id FROM scheduled_tasks WHERE project_id = ?1")?;
        let existing: Vec<String> = stmt
            .query_map(params![project_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for id in existing {
            if !keep.contains(&id) {
                conn.execute("DELETE FROM scheduled_tasks WHERE id = ?1", params![id])?;
            }
        }
        Ok(())
    }

    // ---- Timeline -----------------------------------------------------------

    pub fn timeline(&self, limit: u32) -> Result<Vec<TimelineItem>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.task_id, t.title, s.project_id, p.name, s.agent, s.kind,
                    s.status, s.started_at, s.ended_at, s.usage
             FROM sessions s
             LEFT JOIN tasks t ON t.id = s.task_id
             LEFT JOIN projects p ON p.id = s.project_id
             ORDER BY s.created_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                let usage_json: String = r.get(10)?;
                let usage: TokenUsage = serde_json::from_str(&usage_json).unwrap_or_default();
                Ok(TimelineItem {
                    session_id: r.get(0)?,
                    task_id: r.get(1)?,
                    task_title: r.get(2)?,
                    project_id: r.get(3)?,
                    project_name: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    agent: AgentKind::from_str(&r.get::<_, String>(5)?)
                        .unwrap_or(AgentKind::Claude),
                    kind: SessionKind::from_str(&r.get::<_, String>(6)?),
                    status: SessionStatus::from_str(&r.get::<_, String>(7)?),
                    started_at: r.get(8)?,
                    ended_at: r.get(9)?,
                    cost_usd: usage.total_cost_usd,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

// ---- Row mappers -----------------------------------------------------------

fn map_project(r: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: r.get("id")?,
        name: r.get("name")?,
        path: r.get("path")?,
        description: r.get("description")?,
        enabled: r.get("enabled")?,
        default_agent: AgentKind::from_str(&r.get::<_, String>("default_agent")?)
            .unwrap_or(AgentKind::Claude),
        allowed_agents: serde_json::from_str(&r.get::<_, String>("allowed_agents")?)
            .unwrap_or_else(|_| vec![AgentKind::Claude]),
        max_concurrent: r.get("max_concurrent")?,
        roadmap_enabled: r.get("roadmap_enabled")?,
        verify_enabled: r.get("verify_enabled")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

fn map_task(r: &Row) -> rusqlite::Result<Task> {
    let depends_on: String = r.get("depends_on")?;
    let tags: String = r.get("tags")?;
    Ok(Task {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        title: r.get("title")?,
        description: r.get("description")?,
        status: TaskStatus::from_str(&r.get::<_, String>("status")?),
        priority: r.get("priority")?,
        agent: AgentKind::from_str(&r.get::<_, String>("agent")?).unwrap_or(AgentKind::Claude),
        auto_agent: r.get("auto_agent")?,
        model: r.get("model")?,
        parent_id: r.get("parent_id")?,
        depends_on: serde_json::from_str(&depends_on).unwrap_or_default(),
        attempts: r.get::<_, i64>("attempts")? as u32,
        max_attempts: r.get::<_, i64>("max_attempts")? as u32,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        auto_generated: r.get("auto_generated")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

fn map_scheduled(r: &Row) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        path: r.get("path")?,
        rel_path: r.get("rel_path")?,
        title: r.get("title")?,
        schedule: r.get("schedule")?,
        schedule_kind: r.get("schedule_kind")?,
        schedule_desc: r.get("schedule_desc")?,
        agent: r
            .get::<_, Option<String>>("agent")?
            .and_then(|a| AgentKind::from_str(&a)),
        model: r.get("model")?,
        priority: r.get("priority")?,
        enabled: r.get("enabled")?,
        valid: r.get("valid")?,
        error: r.get("error")?,
        body: r.get("body")?,
        last_run: r.get("last_run")?,
        next_run: r.get("next_run")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

fn map_session(r: &Row) -> rusqlite::Result<Session> {
    let usage_json: String = r.get("usage")?;
    Ok(Session {
        id: r.get("id")?,
        task_id: r.get("task_id")?,
        project_id: r.get("project_id")?,
        agent: AgentKind::from_str(&r.get::<_, String>("agent")?).unwrap_or(AgentKind::Claude),
        kind: SessionKind::from_str(&r.get::<_, String>("kind")?),
        status: SessionStatus::from_str(&r.get::<_, String>("status")?),
        agent_session_id: r.get("agent_session_id")?,
        model: r.get("model")?,
        prompt: r.get("prompt")?,
        result_text: r.get("result_text")?,
        error: r.get("error")?,
        exit_code: r.get("exit_code")?,
        usage: serde_json::from_str(&usage_json).unwrap_or_default(),
        started_at: r.get("started_at")?,
        ended_at: r.get("ended_at")?,
        created_at: r.get("created_at")?,
    })
}

fn map_event(r: &Row) -> rusqlite::Result<SessionEvent> {
    let data: Option<String> = r.get("data")?;
    Ok(SessionEvent {
        id: r.get("id")?,
        session_id: r.get("session_id")?,
        kind: r.get("kind")?,
        text: r.get("text")?,
        data: data.and_then(|d| serde_json::from_str(&d).ok()),
        created_at: r.get("created_at")?,
    })
}

#[cfg(test)]
mod tests;

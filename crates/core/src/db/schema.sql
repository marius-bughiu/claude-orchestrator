-- Claude Orchestrator schema (SQLite).
-- Timestamps are RFC3339 TEXT (UTC). JSON-typed columns hold serde_json strings.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS settings (
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    json  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    path             TEXT NOT NULL,
    description      TEXT,
    enabled          INTEGER NOT NULL DEFAULT 1,
    default_agent    TEXT NOT NULL DEFAULT 'claude',
    allowed_agents   TEXT NOT NULL DEFAULT '["claude"]',
    max_concurrent   INTEGER,
    roadmap_enabled  INTEGER NOT NULL DEFAULT 1,
    verify_enabled   INTEGER NOT NULL DEFAULT 1,
    default_max_attempts INTEGER,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'pending',
    priority        INTEGER NOT NULL DEFAULT 50,
    agent           TEXT NOT NULL DEFAULT 'claude',
    auto_agent      INTEGER NOT NULL DEFAULT 1,
    model           TEXT,
    parent_id       TEXT,
    depends_on      TEXT NOT NULL DEFAULT '[]',
    attempts        INTEGER NOT NULL DEFAULT 0,
    max_attempts    INTEGER NOT NULL DEFAULT 3,
    tags            TEXT NOT NULL DEFAULT '[]',
    auto_generated  INTEGER NOT NULL DEFAULT 0,
    retry_at        TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tasks_project ON tasks(project_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);

CREATE TABLE IF NOT EXISTS sessions (
    id                TEXT PRIMARY KEY,
    task_id           TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    project_id        TEXT NOT NULL,
    agent             TEXT NOT NULL,
    kind              TEXT NOT NULL DEFAULT 'task',
    status            TEXT NOT NULL DEFAULT 'pending',
    agent_session_id  TEXT,
    model             TEXT,
    prompt            TEXT NOT NULL DEFAULT '',
    result_text       TEXT,
    error             TEXT,
    exit_code         INTEGER,
    usage             TEXT NOT NULL DEFAULT '{}',
    branch            TEXT,
    pr_url            TEXT,
    started_at        TEXT,
    ended_at          TEXT,
    created_at        TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_task ON sessions(task_id);
CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_id);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);

CREATE TABLE IF NOT EXISTS session_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    text        TEXT,
    data        TEXT,
    created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_session ON session_events(session_id);

CREATE TABLE IF NOT EXISTS usage_records (
    id                TEXT PRIMARY KEY,
    agent             TEXT NOT NULL,
    session_id        TEXT,
    input_tokens      INTEGER NOT NULL DEFAULT 0,
    output_tokens     INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd          REAL NOT NULL DEFAULT 0,
    num_turns         INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_usage_agent_time ON usage_records(agent, created_at);

CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path           TEXT NOT NULL,
    rel_path       TEXT NOT NULL,
    title          TEXT NOT NULL,
    schedule       TEXT NOT NULL DEFAULT '',
    schedule_kind  TEXT NOT NULL DEFAULT '',
    schedule_desc  TEXT NOT NULL DEFAULT '',
    agent          TEXT,
    model          TEXT,
    priority       INTEGER NOT NULL DEFAULT 50,
    enabled        INTEGER NOT NULL DEFAULT 1,
    valid          INTEGER NOT NULL DEFAULT 1,
    error          TEXT,
    body           TEXT NOT NULL DEFAULT '',
    last_run       TEXT,
    next_run       TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_scheduled_project ON scheduled_tasks(project_id);

CREATE TABLE IF NOT EXISTS activity_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL,
    level       TEXT NOT NULL DEFAULT 'info',
    message     TEXT NOT NULL,
    project_id  TEXT,
    task_id     TEXT,
    session_id  TEXT,
    created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_activity_time ON activity_log(created_at);

# Architecture

Claude Orchestrator is a cross-platform [Tauri 2](https://tauri.app) desktop app that drives autonomous coding agents (Claude Code, Gemini CLI, Codex CLI) across multiple local git repositories. It maintains a queue of tasks per project, allocates them to concurrent agent **sessions**, refills empty queues with a **roadmap** loop, and **verifies** finished work — re-queueing with reviewer feedback when a task falls short.

This document is for contributors. It walks through every layer, naming the real types and functions so you can navigate the code directly.

---

## 1. Layered overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  React + TypeScript frontend  (src/)                                          │
│                                                                               │
│   views/ (Projects, Tasks, Timeline, Settings, Task/Session/Project detail)   │
│   components/ (Layout, UsageBar, TaskTable, …)   store/ (zustand)             │
│   api/index.ts  — typed invoke() wrappers + event listener                    │
│   api/types.ts  — TS mirror of Rust models (camelCase)                        │
└───────────────▲───────────────────────────────────────────────┬──────────────┘
                │  invoke("command", args)                       │  listen()
                │  (Promise<T>)                                  │  "orchestrator://event"
                │                                                ▼
┌───────────────┴───────────────────────────────────────────────────────────────┐
│  Tauri host  (src-tauri/)                                                       │
│                                                                                 │
│   commands/mod.rs  — #[tauri::command] IPC surface (Result<T, String>)         │
│   state.rs         — AppState { engine: Arc<Engine> }                           │
│                      TauriSink: EventSink → app.emit(EVENT_CHANNEL, event)      │
│   lib.rs           — runtime setup, Db::open, Engine::new + start, handlers     │
└───────────────▲───────────────────────────────────────────────┬────────────────┘
                │  &Engine / Db calls                            │  OrchestratorEvent
                │                                                ▼  (via dyn EventSink)
┌───────────────┴───────────────────────────────────────────────────────────────┐
│  orchestrator-core  (crates/core/)  — platform-independent engine               │
│                                                                                 │
│   engine.rs    scheduler tick loop · roadmap · verification · retries           │
│   runner.rs    process spawn · stream parse · cancel · deadline · RunOutcome     │
│   agents/      AgentAdapter trait + Claude / Gemini / Codex adapters             │
│   parse.rs     roadmap & verify JSON output contracts                            │
│   conventions  .orchestrator/ files (+ embedded templates/)                      │
│   db/          rusqlite persistence (schema.sql)                                 │
│   models.rs · config.rs · event.rs · service.rs · error.rs · util.rs            │
└───────────────┬────────────────────────────────────────────────┬───────────────┘
                │  Command::spawn (stdout = stream-json)           │  SQL
                ▼                                                  ▼
        ┌───────────────────────────┐                   ┌──────────────────────┐
        │  Agent CLIs               │                   │  SQLite              │
        │  claude · gemini · codex  │                   │  orchestrator.sqlite │
        └───────────────────────────┘                   └──────────────────────┘
```

The split between `orchestrator-core` and `src-tauri` is deliberate. The core is a pure Rust library with **no GUI or Tauri dependency**, so it can be unit-tested in isolation (32 tests) and reused from any host. The Tauri crate is a thin shell: it owns the OS window, the tokio runtime, the SQLite file location, and the bridge that forwards engine events to the webview.

Two crates form a Cargo workspace (`Cargo.toml` at the root, `resolver = "2"`):

| Crate | Path | Role |
|---|---|---|
| `orchestrator-core` (lib `orchestrator_core`) | `crates/core` | The engine: data model, persistence, agent adapters, runner, scheduler. |
| `claude-orchestrator` (lib `claude_orchestrator_lib` + bin) | `src-tauri` | Tauri host wrapping the engine and exposing IPC. |

---

## 2. `orchestrator-core` module breakdown

`crates/core/src/lib.rs` declares the modules and re-exports the common surface (`Db`, `Engine`, `Settings`, `EventSink`, `OrchestratorEvent`, `CoreError`, `Result`, and all of `models::*`).

### 2.1 `models.rs` — the data model

Every struct here is `Serialize`/`Deserialize` with `#[serde(rename_all = "camelCase")]` (so it crosses to the frontend as camelCase) and is persisted to SQLite. There is no GUI concern in this file.

Enums (serialized lowercase / snake_case):

- **`AgentKind`** — `Claude | Gemini | Codex`. `ALL` is the canonical iteration order; `as_str`/`from_str` map to `"claude"` etc. Claude is the orchestrator and default executor; Gemini and Codex are delegable sub-agents.
- **`TaskStatus`** — `Pending | Queued | Running | NeedsReview | Completed | Failed | Cancelled | Blocked`. Helpers: `is_terminal()` (`Completed | Cancelled`) and `is_schedulable()` (`Pending | NeedsReview`).
- **`SessionKind`** — `Task | Roadmap | Verify`. Determines which convention prompt is used and how the result is interpreted.
- **`SessionStatus`** — `Pending | Running | Completed | Failed | Cancelled | TimedOut`. `is_active()` = `Pending | Running`.

Records:

- **`Project`** — a managed git repo: `path`, `enabled`, `default_agent`, `max_concurrent: Option<u32>` (None → use global), `roadmap_enabled`, `verify_enabled`.
- **`Task`** — `description` is the prompt handed to the agent. `priority: i64` (higher runs first), `depends_on: Vec<String>` (task ids that must be `Completed`), `attempts`/`max_attempts`, `auto_generated` (set by the roadmap loop), `parent_id`.
- **`TokenUsage`** — `input/output/cache_read/cache_creation` tokens, `total_cost_usd`, `num_turns`. `add()` accumulates one usage into another.
- **`Session`** — one CLI invocation. Holds `agent_session_id` (the agent's *own* session id, used to resume), `prompt`, `result_text`, `error`, `exit_code`, `usage`, and timestamps.
- **`SessionEvent`** — a normalized, persisted stream event: `kind` (`"init" | "assistant" | "thinking" | "tool_use" | "tool_result" | "result" | "error" | "raw"`), human-readable `text`, and structured `data`.
- **`AgentUsage` / `AgentLimits`** — per-agent rollups for the header: `window` vs `total` usage, `active_sessions`, `window_started_at`, `window_hours`.
- **`OrchestratorStatus`** — the top-bar snapshot.
- **`TimelineItem`** — a flattened session row (joined with task title + project name) for the timeline view.

### 2.2 `config.rs` — Settings and PermissionMode

- **`PermissionMode`** (serialized kebab-case) — `Default | AcceptEdits | Plan | BypassPermissions`. The default is **`BypassPermissions`**, which maps to Claude's `--dangerously-skip-permissions`; full unattended autonomy requires bypassing prompts, and that is an explicit, opt-in posture.
- **`AgentConfig`** — per-agent: `binary` override, `model`, `extra_args`, `limits`, `window_hours` (default **5**, matching Claude plan resets), `enabled`.
- **`Settings`** — global: `running`, `max_concurrent` (default 3), `tick_interval_secs` (default 10), `default_agent`, `permission_mode`, `session_timeout_secs` (default 1800; 0 = no limit), `roadmap_enabled`, `verify_enabled`, and `agents: BTreeMap<String, AgentConfig>` keyed by agent name. `Settings::default()` seeds an `AgentConfig` for every `AgentKind::ALL`. `agent_config(kind)` returns a clone or the default.

Settings are stored as a **single JSON row** (`id = 1`) in the `settings` table.

### 2.3 `db/` — SQLite persistence

`db/mod.rs` wraps a single `rusqlite::Connection` behind `Arc<Mutex<Connection>>`. `Db` is `Clone` (cheap, shares the `Arc`). All methods are **synchronous**; async callers hold the lock only for the duration of a query. The schema in `db/schema.sql` is applied via `execute_batch` on open (`from_connection`), so opening is idempotent (`CREATE TABLE IF NOT EXISTS …`). `open_in_memory()` is used throughout the tests.

Schema (timestamps are RFC3339 TEXT in UTC; JSON columns hold `serde_json` strings):

- `settings(id CHECK(id=1), json)`
- `projects(id, name, path, …, max_concurrent, roadmap_enabled, verify_enabled, …)`
- `tasks(…, depends_on '[]', attempts, max_attempts, tags '[]', auto_generated, …)` with `ON DELETE CASCADE` from `projects`; indexed on `project_id`, `status`.
- `sessions(…, agent_session_id, model, prompt, result_text, error, exit_code, usage '{}', started_at, ended_at, …)`; `task_id` is `ON DELETE SET NULL`; indexed on `task_id`, `project_id`, `status`.
- `session_events(id AUTOINCREMENT, session_id → sessions ON DELETE CASCADE, kind, text, data, created_at)`.
- `usage_records(id, agent, session_id, input/output/cache tokens, cost_usd, num_turns, created_at)`; indexed on `(agent, created_at)` for windowed aggregation.

`PRAGMA journal_mode = WAL` and `foreign_keys = ON` are set in the schema.

Notable query methods: `get/save_settings`, project/task CRUD, `schedulable_tasks(project_id)` (status in `pending`/`needs_review`, `ORDER BY priority DESC, created_at ASC`), the active-session counters (`count_active_sessions`, `…_for_project`, `…_for_agent`, `active_sessions()`), `insert_event`/`list_events`, `insert_usage`/`usage_for_agent(agent, since)` (windowed via `created_at >= since`), and `timeline(limit)` (a `LEFT JOIN` across sessions/tasks/projects). Row mapping is centralized in `map_project`/`map_task`/`map_session`/`map_event`.

**`reconcile_orphan_sessions()`** flips any session left in `pending`/`running` (e.g. by a crash) to `failed` with an "orphaned" error. The engine calls this once on startup.

### 2.4 `agents/` — adapter trait, AgentEvent, per-agent stream formats

Adapters are **pure**: they build a command line and parse output lines but never spawn processes or do IO, so they are deterministically unit-testable. Spawning lives entirely in `runner.rs`.

**`RunSpec`** carries everything needed to launch one invocation: `prompt`, `cwd` (the repo), optional `model`, `system_prompt_append`, `resume_session_id`, `session_id`, `permission_mode`, `add_dirs`, `mcp_config`, `extra_args`.

**`Invocation`** is the concrete result: `program`, `args`, and optional `stdin` payload.

**`AgentEvent`** is the normalized, agent-agnostic stream event:

```rust
enum AgentEvent {
    Init { agent_session_id: Option<String>, model: Option<String> },
    Assistant { text: String },
    Thinking { text: String },
    ToolUse { name: String, input: serde_json::Value },
    ToolResult { content: String, is_error: bool },
    Result { success: bool, result_text: Option<String>, usage: TokenUsage },
    Error { message: String },
    Raw { value: serde_json::Value },   // anything not specifically modeled
}
```

`AgentEvent::kind()` returns the short string persisted with each `SessionEvent`.

**`AgentAdapter` trait:**

```rust
trait AgentAdapter: Send + Sync {
    fn kind(&self) -> AgentKind;
    fn default_binary(&self) -> &'static str;
    fn build_invocation(&self, spec: &RunSpec, binary: &str) -> Invocation;
    fn parse_line(&self, line: &str) -> Vec<AgentEvent>;   // 0..n events per line
}
```

`adapter_for(kind) -> Box<dyn AgentAdapter>` is the factory.

Per-agent specifics:

- **`ClaudeAdapter`** (`claude.rs`) — invokes `claude -p --output-format stream-json --verbose [--model M] <perm flags> <prompt>`. `--resume <id>` continues a prior conversation and takes precedence over `--session-id`. Permission flags: `BypassPermissions → --dangerously-skip-permissions`, otherwise `--permission-mode default|acceptEdits|plan`. The prompt is the trailing positional arg. `parse_line` handles `system/init` → `Init`, `assistant`/`user` message envelopes (string or content-block array → `Assistant`/`Thinking`/`ToolUse`/`ToolResult`), and `result` (`subtype=="success" && !is_error` → `Result.success`, with `parse_usage` reading `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`, `total_cost_usd`, `num_turns`). Unrecognized lines become `Raw`; non-JSON lines become `Raw` text.
- **`GeminiAdapter`** (`gemini.rs`) — `gemini --output-format json [--model M] [--yolo] --prompt <prompt>`. `--yolo` auto-approves whenever the permission mode is not `Default`. Output is a single terminal `{"response": "...", "stats": {…}}` object → `Result`; `parse_stats` sums `stats.models.<model>.tokens` (`prompt`→input, `candidates`→output, `cached`→cache_read). Plain-text lines fall back to `Assistant`. Deliberately permissive: Gemini's JSON surface is still evolving.
- **`CodexAdapter`** (`codex.rs`) — `codex exec --json [--model M] [--session <id>] [--full-auto] <prompt>`. `--full-auto` enables unattended execution. Newline-delimited "thread events", flat or nested under `msg`: `session_configured`/`thread.started`→`Init`; `agent_message`/`assistant_message`→`Assistant`; `agent_reasoning`/`reasoning`→`Thinking`; `exec_command_begin`/`tool_call`/`command`→`ToolUse`; `token_count`/`usage`→a usage-only `Result`; `task_complete`/`turn.completed`/`thread.completed`→terminal `Result`; `error`/`stream_error`→`Error`.

### 2.5 `runner.rs` — process runner

`run_agent(adapter, binary, spec, cancel, timeout, on_event) -> Result<RunOutcome>` is the single entry point. It:

1. Calls `adapter.build_invocation(spec, binary)` and spawns a `tokio::process::Command` with `current_dir = spec.cwd`, piped stdout/stderr, `kill_on_drop(true)`. A missing binary surfaces as `CoreError::AgentUnavailable`.
2. Writes `invocation.stdin` if present, then drains **stderr** concurrently into a bounded (16 KB) buffer for error reporting.
3. Reads stdout line-by-line in a `tokio::select!` loop with `biased` priority: **cancel → timeout → next line**. The deadline is `Instant::now() + timeout` (or `pending()` forever when `timeout` is `None`). Each line is fed to `adapter.parse_line`; every resulting `AgentEvent` is accumulated via `apply_event` and forwarded to `on_event` synchronously.
4. On `Cancelled`/`TimedOut` it kills the child; otherwise it waits for exit and records `exit_code`.

**Outcome accumulation** (`RunOutcome`): `success`, `agent_session_id`, `model`, `result_text`, `usage`, `exit_code`, `error`, `cancelled`, `timed_out`. `apply_event` captures the `Init` session id/model, appends `Assistant` text into a running buffer, and on `Result` sets `success`, `result_text`, and `usage.add(...)`. After EOF: if no explicit `result_text` was emitted it falls back to the trimmed assistant buffer; success is the explicit `Result` flag if one was seen, else a clean exit (`exit_code == Some(0)`); if still unsuccessful with no error, it adopts the stderr text (or "cancelled"/"timed out").

**`CancelToken`** is a cloneable cooperative token over `AtomicBool` + `tokio::sync::Notify`. `cancel()` sets the flag and wakes waiters; `cancelled()` is an async future that resolves once cancelled.

`runner.rs` ships three `#[tokio::test]`s using a `FakeAdapter` that runs a shell script emitting canned Claude stream-json — exercising outcome collection, the `AgentUnavailable` path, and cancellation-kills-process — with **no real CLI installed**.

### 2.6 `engine.rs` — the scheduler

**`Engine`** holds `db: Db`, `sink: Arc<dyn EventSink>`, `running: Arc<Mutex<HashMap<String, CancelToken>>>` (session id → cancel token), and a `wake: Arc<Notify>`. Constructed via `Engine::new(db, sink) -> Arc<Engine>`.

- **`start(&Arc<Self>)`** spawns the background loop. It first calls `reconcile_orphan_sessions()`, then loops: `tick().await`, then `tokio::select!` on `sleep(tick_interval_secs)` or `wake.notified()`. `request_tick()` calls `wake.notify_one()` so commands can nudge the scheduler immediately.
- **`tick()`** is the heart of scheduling (detailed in §6). It respects the global and per-project concurrency caps, picks the next runnable task per project, and falls back to the roadmap loop when a project's queue is empty.
- **`start_task`** sets the task `Running`, builds the prompt (`task_prompt` = optional `.orchestrator/task.md` preamble + `# Task: <title>` + description), creates a `Task`-kind `Session`, and spawns the job.
- **`spawn_session_job`** (`tokio::spawn`) runs the session, then dispatches the outcome: `handle_task_outcome` for task sessions, `handle_roadmap_outcome` for roadmap sessions, `finalize_session_error` on a runner error. It finishes by emitting `StatusChanged` + `UsageUpdated` and `request_tick()` (freeing a slot may unblock more work).
- **`run_session`** registers the `CancelToken`, marks the session `Running`, resolves the binary (`AgentConfig.binary` or `adapter.default_binary()`) and timeout, then calls `runner::run_agent`. The per-event closure persists each event via `db.insert_event` and emits `OrchestratorEvent::SessionEvent`. On return it removes the cancel token and calls `finalize_session`.
- **`finalize_session`** maps the outcome onto a terminal `SessionStatus` (`Cancelled`/`TimedOut`/`Completed`/`Failed`), records `agent_session_id`/`model`/`result_text`/`error`/`exit_code`/`usage`, and — if usage is non-zero — writes a `usage_records` row.
- **`handle_task_outcome`** increments `attempts` and decides the next status:
  - **cancelled** → task back to `Pending`, attempt count rolled back.
  - **failed** → `NeedsReview` (with the error appended as feedback) unless `attempts >= max_attempts`, then `Failed`.
  - **success + verification disabled** (global *or* project) → `Completed`.
  - **success + verification enabled** → run a `Verify` session and apply the verdict.
- **`run_verification`** builds the verify prompt (`conventions::verify_prompt` + task title/description + the executing session's `result_text`), runs a `Verify` session, and parses the verdict with `parse::parse_verdict`. A `complete` verdict → `Completed`; an incomplete verdict appends `follow_up` (or `reason`) as reviewer feedback and re-queues as `NeedsReview` (or `Failed` if attempts are exhausted); **no parseable verdict** → the executor is trusted and the task is marked `Completed` with a warning log.
- **`handle_roadmap_outcome`** parses the result with `parse::parse_roadmap_tasks` and inserts each as a fresh `Pending`, `auto_generated` task (`max_attempts = 3`).
- **`send_message`** continues a conversation: it creates a new `Task` session resuming the prior session's `agent_session_id` and spawns it. **`trigger_roadmap`** forces a roadmap session regardless of queue depth. **`stop_session`** looks up the live `CancelToken` and cancels it.

`append_feedback` appends reviewer/error text under a clearly delimited `## Reviewer feedback (address this)` section so the next attempt sees it. `describe_event` turns an `AgentEvent` into the `(text, data)` pair stored on each `SessionEvent`.

### 2.7 `parse.rs` — output contracts

The roadmap and verify prompts ask the agent to end its response with **exactly one fenced ```json block**. `extract_last_json(text)` finds the last fenced JSON block (`last_fenced_json`), falling back to the last balanced `{…}`/`[…]` run that parses (`last_balanced_json`) — this is robust to prose that contains stray braces.

- **`parse_roadmap_tasks`** accepts a bare array or an object with a `tasks` array, deserializes `Vec<RoadmapTaskSpec>` (`title`, `description`, `priority`, `agent`, `tags`), and drops entries with an empty title. `priority_or_default()` → 50; `agent_kind(default)` resolves the agent string or falls back to the project default.
- **`parse_verdict`** deserializes a single `VerifyVerdict { complete, reason, follow_up }`.

### 2.8 `conventions.rs` + `templates/` — `.orchestrator/` files

Each managed project may contain an `.orchestrator/` directory steering its behavior. Every file is optional; sensible defaults are embedded via `include_str!` from `templates/`, so **any git repo works out of the box** while power users override per project.

- `config.json` — per-project config (currently advisory).
- `roadmap.md` — the roadmap-loop prompt. Defines the JSON task-array output contract (`title`, `description`, optional `priority`/`agent`/`tags`; 1–8 tasks; `[]` when nothing valuable remains).
- `verify.md` — the verifier prompt. Defines the `{ complete, reason, follow_up }` contract and instructs the verifier to check reality (files/tests/builds), not the agent's narrative.
- `task.md` — preamble prepended to every task prompt for the project.

`roadmap_prompt`/`verify_prompt`/`task_preamble` read the file or return the embedded default. `is_initialized` checks for the directory. `scaffold` writes the five default files (README, config, roadmap, verify, task) **without clobbering** any that already exist, returning the relative paths created.

### 2.9 `event.rs` — EventSink decoupling

The core never references Tauri. It emits **`OrchestratorEvent`** (serialized `{ "type": "...", … }`) through an **`EventSink`** trait (`emit(&self, event)`, `Send + Sync + 'static`). Variants: `SessionEvent`, `SessionUpdated`, `TaskUpdated`, `StatusChanged`, `UsageUpdated`, `Log`. `NullSink` drops everything (headless runs/tests); the Tauri host supplies `TauriSink`.

### 2.10 `service.rs` — host-facing operations

Higher-level operations with validation/defaults, used by the Tauri commands:

- **`add_project(db, AddProjectInput)`** — validates the path is a directory, canonicalizes it, rejects duplicates by path, derives a name from the folder if none given, records the project (git presence is advisory), and optionally `scaffold`s the conventions.
- **`create_task(db, CreateTaskInput)`** — requires a non-empty title and an existing project; defaults `priority = 50`, `agent = project.default_agent`, `max_attempts = 3`, status `Pending`.

### 2.11 `error.rs` / `util.rs`

`CoreError` (via `thiserror`) wraps `rusqlite`, `serde_json`, and `io` errors and adds `NotFound`, `Invalid`, `AgentUnavailable`, `Other`. `Result<T> = Result<T, CoreError>`. `util::binary_available(name)` checks `PATH` (and `PATHEXT` on Windows) to detect installed agent CLIs without spawning them — feeding `AgentUsage.available`.

---

## 3. The Tauri host (`src-tauri/`)

### 3.1 `AppState` and `TauriSink` (`state.rs`)

`AppState { engine: Arc<Engine> }` is the single managed Tauri state. `TauriSink { app: AppHandle }` implements `EventSink`: `emit` forwards each `OrchestratorEvent` to the webview via `app.emit(EVENT_CHANNEL, event)`, where `EVENT_CHANNEL = "orchestrator://event"`. The emit is best-effort — a failure (e.g. a closing window) must never crash the engine.

### 3.2 Runtime setup and wiring (`lib.rs`)

`run()` initializes `tracing_subscriber`, then **builds and enters a multi-thread tokio runtime** that is held for the app's lifetime. Entering the runtime is what lets the engine's `tokio::spawn` calls (issued from `setup` and from synchronous command handlers) find a reactor. In `tauri::Builder::setup`, it resolves a per-user data dir (`app_data_dir()`, falling back to `directories::ProjectDirs`), opens `orchestrator.sqlite` via `Db::open`, constructs `TauriSink`, builds the engine with `Engine::new(db, sink)`, calls `engine.start()` (which kicks off orphan reconciliation + the tick loop), and `app.manage(AppState { engine })`. The dialog and opener plugins are registered.

### 3.3 Command surface (`commands/mod.rs`)

Each `#[tauri::command]` takes `State<AppState>`, returns `Result<T, String>` (errors stringified so they surface cleanly in JS), and is registered in the `generate_handler!` list in `lib.rs`. Mutating commands call `engine.request_tick()` so the scheduler reacts immediately. The surface:

| Group | Commands |
|---|---|
| Projects | `list_projects`, `get_project`, `add_project`, `update_project`, `remove_project`, `scaffold_project`, `project_conventions` |
| Tasks | `list_tasks`, `get_task`, `create_task`, `update_task`, `delete_task`, `run_task_now` |
| Sessions | `list_sessions`, `get_session`, `get_session_events`, `send_message`, `stop_session` |
| Orchestrator | `get_status`, `set_running`, `get_settings`, `update_settings`, `trigger_roadmap`, `get_timeline` |

---

## 4. The frontend (`src/`)

- **`api/types.ts`** mirrors the Rust models exactly, in **camelCase** (matching the `#[serde(rename_all = "camelCase")]` on the core structs) — `Project`, `Task`, `Session`, `SessionEvent`, `TokenUsage`, `AgentUsage`, `OrchestratorStatus`, `TimelineItem`, `Settings`, `AgentConfig`, the input types, and the `OrchestratorEvent` discriminated union (`type: "sessionEvent" | "sessionUpdated" | "taskUpdated" | "statusChanged" | "usageUpdated" | "log"`). The string-literal unions for `TaskStatus`/`SessionStatus`/`SessionKind`/`PermissionMode`/`AgentKind` track the Rust serde renames (including kebab-case `permissionMode`).
- **`api/index.ts`** is a thin typed wrapper over `invoke<T>("command", args)` for every command, plus `onOrchestratorEvent(handler)` which `listen`s on `EVENT_CHANNEL`. Note the argument key casing must match the Tauri handler parameter names (e.g. `{ projectId }`, `{ taskId }`).
- **`store/index.ts`** is a single [zustand](https://github.com/pmndrs/zustand) store. `init()` runs once, does a `refreshAll()` (parallel fetch of status/projects/tasks/timeline/settings) to seed state and set `connected`, then subscribes to the event channel. **`handleEvent`** drives live updates without polling: `statusChanged`/`usageUpdated` → `refreshStatus`; `taskUpdated` → upsert the task in place; `sessionUpdated` → `refreshTimeline` + `refreshStatus`; `log` → prepend to a capped (200) log ring.
- **Routing/views** — `App.tsx` mounts a `createHashRouter` (hash routing suits the Tauri `asset:`/custom-protocol origin) under a shared `Layout`. Routes: `/projects`, `/projects/:id`, `/tasks`, `/tasks/:id`, `/timeline`, `/sessions/:id`, `/settings` (index redirects to `/projects`). `Layout` renders the nav, the connection indicator, and `UsageBar` (the per-agent usage/limits header fed by `OrchestratorStatus.agents`).

---

## 5. Data model and lifecycle

**Task lifecycle.** A task is created `Pending` (`Blocked` is reserved for unmet dependencies). The scheduler picks it, sets it `Running`, and spawns a `Task` session. On the session's outcome:

```
                       ┌──────────── cancelled ───────────┐
                       │                                   ▼
 Pending ──pick──► Running ──success──► [verify?] ──complete──► Completed
   ▲   ▲             │  │                   │
   │   │             │  └──fail──┐          └──incomplete──┐
   │   └─cancel rolls│           ▼                         ▼
   │     back attempt│      attempts<max ─► NeedsReview ──re-pick──► Running
   │                 │           │  (reviewer feedback appended)
   └─ NeedsReview ───┘           └─ attempts>=max ─► Failed
```

- **`NeedsReview`** is schedulable: it is re-picked exactly like `Pending`, but its description now carries appended reviewer/error feedback.
- **Retries** are bounded by `max_attempts`. Each finished task session increments `attempts`; once `attempts >= max_attempts`, the next failure/incomplete verdict yields `Failed` instead of `NeedsReview`.
- **Cancellation** (`stop_session`) marks the session `Cancelled` and returns the task to `Pending` with the attempt rolled back, so a manual stop is not punitive.
- `Queued` exists in the model for UI intent; the engine transitions `Pending`/`NeedsReview` straight to `Running` when a session starts.

**Session lifecycle.** A session starts `Pending`, becomes `Running` in `run_session`, and finalizes to `Completed | Failed | Cancelled | TimedOut`. Its `kind` (`Task | Roadmap | Verify`) selects the prompt source and the outcome handler. A successful `Task` session optionally spawns a `Verify` session whose verdict decides the task's fate; an empty queue spawns a `Roadmap` session whose parsed output becomes new tasks.

---

## 6. The scheduling algorithm (`Engine::tick`)

One tick:

1. Load `Settings`. If `!running`, return (the scheduler is paused).
2. Read `global_max = max_concurrent.max(1)` and `active = count_active_sessions()`. If `active >= global_max`, return — no global capacity.
3. Snapshot `active_sessions()` and `list_projects()`.
4. For each **enabled** project (projects are listed alphabetically), compute `proj_max = project.max_concurrent.unwrap_or(global_max).max(1)` and loop:
   - If `active >= global_max`, return (global cap reached mid-pass).
   - If `count_active_sessions_for_project >= proj_max`, break to the next project (per-project cap reached).
   - **`pick_task(project_id)`**: take `schedulable_tasks` (already ordered **`priority DESC, created_at ASC`** — higher priority first, then FIFO), skip tasks with `attempts >= max_attempts`, and return the first whose dependencies are satisfied. **`deps_satisfied`** requires every id in `depends_on` to resolve to a task with status `Completed`.
     - If a task is found → `start_task` and `active += 1`, then loop again (a project can fill several slots in one tick).
     - If **no task** is found (empty queue) → consider the **roadmap loop**: if `settings.roadmap_enabled && project.roadmap_enabled` and no roadmap session is already active/running for this project, `spawn_roadmap_session` and `active += 1`; then break to the next project.

So concurrency is enforced at two levels — a global ceiling and an optional per-project ceiling — and within a project, work is strictly priority-ordered with FIFO tie-breaking and hard dependency gating. The roadmap loop is the queue-refill mechanism: it only fires when a project has nothing runnable, is idempotent per project (guarded by both the `active_sessions` snapshot and a fresh `has_running_roadmap` check), and yields a new batch of `Pending` tasks the next ticks will schedule.

**Orphan reconciliation on startup.** Before the loop runs, `start()` calls `reconcile_orphan_sessions()`, flipping any session left `pending`/`running` by a previous crash to `failed`. This keeps the active-session counters honest so the concurrency math is correct from the first tick.

---

## 7. Concurrency and threading

- A single **multi-thread tokio runtime** (built in `lib.rs`) backs everything. It is entered for the process lifetime so `tokio::spawn` works from both the setup hook and the synchronous Tauri command handlers.
- The **scheduler loop** is one spawned task. Each session runs in its **own spawned task** (`spawn_session_job`), so many agents execute concurrently up to the configured caps.
- **Cancellation** is cooperative: the engine keeps `running: Arc<Mutex<HashMap<session_id, CancelToken>>>`. `stop_session` cancels the token; the runner's `biased` `select!` notices and kills the child (also `kill_on_drop(true)` cleans up on early return). Per-session **deadlines** come from `session_timeout_secs` and surface as `TimedOut`.
- **Database access** is serialized through `Arc<Mutex<Connection>>`. `Db` is `Clone` (shares the `Arc`), so the engine, the per-event persistence closure, and command handlers all share one connection; rusqlite calls are synchronous and short-lived, so lock contention is minimal. WAL mode keeps reads non-blocking against the single writer.
- The **engine is `Arc`-shared**; the `EventSink` is `Arc<dyn EventSink>` and `Send + Sync`, so events can be emitted from any spawned task.

---

## 8. Extensibility

### Adding a new agent adapter

1. Add a variant to **`AgentKind`** (`models.rs`) and update `ALL`, `as_str`, `from_str`. The TS union in `api/types.ts` must gain the same lowercase string.
2. Create `crates/core/src/agents/<agent>.rs` with a unit struct implementing **`AgentAdapter`**: `kind`, `default_binary`, `build_invocation` (translate `RunSpec` — including `permission_mode`, `model`, `resume_session_id`, `extra_args` — into an `Invocation`), and `parse_line` (map the CLI's stream format into `AgentEvent`s; preserve unknowns as `Raw`).
3. Register it in `agents/mod.rs`: add the `mod`/`pub use` and a match arm in **`adapter_for`**.
4. Add unit tests next to the adapter (invocation shape + a few `parse_line` cases) — no process needed.

`Settings::default()` already seeds an `AgentConfig` for every `AgentKind::ALL`, and `agent_usage` iterates `ALL`, so the header and per-agent config pick up the new agent automatically.

### Adding a Tauri command + TS binding

1. Write a `#[tauri::command]` in `commands/mod.rs` taking `State<AppState>`, returning `Result<T, String>` (use the `err` helper). Call `engine.request_tick()` if it mutates schedulable state.
2. Register the function name in the `generate_handler!` list in `lib.rs`.
3. Add a typed wrapper in `api/index.ts` (`invoke<T>("command_name", args)`), matching the handler's parameter names as the argument-object keys.
4. If new data crosses the boundary, mirror the Rust type in `api/types.ts` (camelCase) and wire it into the store/views.

---

## 9. Testing strategy

- **Core unit tests (32).** `cargo test -p orchestrator-core` runs them with no external CLIs, no GUI, and an in-memory DB:
  - `agents/claude.rs` (7), `agents/gemini.rs` (3), `agents/codex.rs` (3) — invocation building and `parse_line` normalization per agent.
  - `parse.rs` (6) — fenced/balanced JSON extraction and the roadmap/verify contracts (including prose-with-braces and last-block-wins).
  - `db/tests.rs` (6) — settings roundtrip, project/task CRUD, schedulable ordering, cascade delete, session/event/usage aggregation, and orphan reconciliation (all on `Db::open_in_memory`).
  - `conventions.rs` (2) — defaults-when-missing and scaffold-then-preserve.
  - `service.rs` (2) — input validation and project/task creation defaults.
  - `runner.rs` (3) — the **fake-adapter runner tests**: a `FakeAdapter` runs a shell script emitting canned stream-json, exercising outcome accumulation, the `AgentUnavailable` path, and cancellation killing the process — full runner coverage without a real agent installed.
- **Frontend type/build check.** `pnpm build` runs `tsc --noEmit` then `vite build`, catching drift between `api/types.ts` and the Rust models at compile time.
- **Tauri crate check.** `cargo check -p claude-orchestrator` type-checks the host. It links against system **WebKitGTK** dev libraries on Linux, so that environment must have them installed; the engine logic itself is already covered by the core tests, keeping the GUI crate's check fast and mostly structural.

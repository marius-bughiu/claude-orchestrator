# Claude Orchestrator

A cross-platform desktop app that orchestrates autonomous coding agents — Claude Code, Gemini CLI, and Codex CLI — across multiple local git repositories. You point it at your repos, describe what you want, and it runs agents in parallel sessions, generates its own follow-up work when a project's queue runs dry, verifies finished tasks, and surfaces live progress and usage in one place. It's a Tauri 2 app: a platform-independent Rust engine (`orchestrator-core`) driving a React/TypeScript UI.

**Features**

- **Projects** — register local git repos; each gets a scaffolded `.orchestrator/` to steer behavior.
- **Tasks** — units of work with priority, dependencies, retry caps, tags, and a chosen agent.
- **Autonomous scheduler** — allocates pending tasks to concurrent sessions, with globally and per-project configurable concurrency.
- **Multi-agent** — Claude is the default orchestrator/executor; Gemini and Codex are delegable sub-agents. Each CLI runs with streaming JSON output, parsed into a normalized event stream.
- **Roadmap loop** — when a project's queue empties, an agent reads the project's direction and proposes the next batch of tasks.
- **Verification** — finished tasks are independently judged; failed verification re-queues the task with reviewer feedback, up to `max_attempts`.
- **Usage / limits tracking** — per-agent token and cost usage over a rolling window (default 5h), shown in the top bar.
- **Timeline** — a chronological view of every session across all projects.
- **Live session streaming** — watch an agent's events (assistant text, tool calls, results) as they happen.
- **Message injection** — send a follow-up message into a session to continue its conversation with full prior context.

## How it works

The scheduler runs on a tick. On each tick it walks every enabled project and, while concurrency budget remains (global cap and per-project cap), picks the highest-priority schedulable task whose dependencies are met and whose attempts aren't exhausted, then spawns a session to execute it. The agent CLI runs with streaming JSON output, which is normalized into a persisted event stream. When a task's session finishes, a verifier session independently judges whether the goal was actually met — if not, the task is re-queued with the verifier's follow-up feedback, up to `max_attempts`. When a project's task queue is empty, the roadmap loop runs instead: an agent reads the project's vision and emits a fresh batch of tasks, and the cycle continues.

## Architecture

The codebase is two Rust crates plus a React frontend. The split between `orchestrator-core` and the Tauri host is deliberate: the engine is GUI-agnostic and fully unit-tested (32 tests), while `src-tauri` is a thin host exposing it over IPC.

```
claude-orchestrator/
├── crates/core/          # orchestrator-core: the platform-independent engine
│   └── src/
│       ├── models.rs         # core data model (Project, Task, Session, usage…)
│       ├── config.rs         # Settings + PermissionMode
│       ├── db/               # SQLite store (schema.sql, queries, tests)
│       ├── agents/           # claude.rs, gemini.rs, codex.rs adapters + stream-json parsing
│       ├── runner.rs         # process runner (spawn, stream, cancel)
│       ├── engine.rs         # scheduler / roadmap / verify loop
│       ├── parse.rs          # roadmap & verify JSON output contracts
│       ├── conventions.rs    # .orchestrator/ file resolution + scaffolding
│       ├── templates/        # embedded default .orchestrator files
│       ├── service.rs        # host-facing ops (add project, create task) + validation
│       └── event.rs          # orchestrator event types
│
├── src-tauri/            # claude-orchestrator: the Tauri host (thin)
│   └── src/
│       ├── commands/mod.rs   # the IPC command surface
│       ├── state.rs          # event bridge (core events → frontend)
│       └── lib.rs            # app setup
│
└── src/                  # React / TypeScript frontend
    ├── api/                  # types.ts + index.ts (typed IPC client)
    ├── store/                # Zustand state
    ├── views/                # Projects, Tasks, Timeline, Settings, Session/Task detail
    └── components/
```

## Prerequisites

- **Rust** (stable, 1.77+) with Cargo.
- **Node.js 22** and **pnpm**.
- **Agent CLIs** on your `PATH`:
  - **Claude Code** (`claude`) — required; it's the default orchestrator/executor.
  - **Gemini CLI** (`gemini`) and **Codex CLI** (`codex`) — optional; only needed if you delegate work to them.
- **Linux only:** WebKit development libraries are required to build the Tauri app (e.g. `webkit2gtk-4.1` / `libwebkit2gtk-4.1-dev`, plus the usual GTK/AppIndicator build deps). See the [Tauri Linux prerequisites](https://tauri.app/start/prerequisites/) for your distro.

## Getting started

Install frontend dependencies:

```bash
pnpm install
```

Run the app in development (hot-reloading Vite frontend + Tauri host):

```bash
pnpm tauri dev
```

Build a production desktop bundle:

```bash
pnpm tauri build
```

Run the core engine's test suite:

```bash
cargo test -p orchestrator-core
```

Other useful checks: `pnpm build` (typecheck + build the frontend) and `cargo check -p claude-orchestrator` (build the Tauri host — needs the Linux WebKit dev libs above).

## Usage

1. **Add a project.** Point the orchestrator at a local git repo. On add, it scaffolds an `.orchestrator/` directory with default convention files (without overwriting anything that already exists), so the repo works out of the box.
2. **Get tasks.** Either create tasks yourself (title, description / acceptance criteria, priority, agent), or leave the project's queue empty and let the **roadmap loop** generate them from the project's direction.
3. **Press Run.** Start the scheduler. It allocates pending tasks to sessions within your configured concurrency limits and delegates to the chosen agent.
4. **Watch the timeline.** Follow sessions live as they stream events; finished tasks are verified automatically, and anything that fails verification is re-queued with feedback. Send a follow-up message into a session at any time to steer it.

## Per-project configuration (`.orchestrator/`)

Each managed repo can carry an `.orchestrator/` directory that steers autonomous behavior — `config.json` (default agent, concurrency, feature toggles), `roadmap.md` (the roadmap-loop prompt), `verify.md` (the verifier prompt), and `task.md` (a preamble prepended to every task prompt). Every file is optional; sensible defaults are embedded in the engine, so any repo works without setup. The `roadmap.md` and `verify.md` files each define a small JSON output contract that the engine parses — keep those contracts intact when customizing.

See [docs/CONVENTIONS.md](docs/CONVENTIONS.md) for the full reference.

## Project status

This is an early **v0.1** public project. It maintains itself via its own orchestration — Claude Orchestrator is registered as one of its own projects and dogfoods the roadmap/verify loop to drive its development. Expect rough edges and rapid change.

## Contributing

Contributions are welcome. Start with [CLAUDE.md](CLAUDE.md) for working conventions and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for a deeper tour of the engine and how the pieces fit together.

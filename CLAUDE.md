# CLAUDE.md

Guidance for Claude Code (and other agents) working in this repository.

## What this is

Claude Orchestrator is a cross-platform **Tauri 2** desktop app that orchestrates
autonomous coding agents (Claude Code, Gemini CLI, Codex CLI) across multiple
local git repositories. It maintains itself — see `.orchestrator/`.

## Layout

- `crates/core/` — `orchestrator-core`: the platform-independent engine
  (data model, SQLite, agent adapters, process runner, scheduler). **No GUI deps.**
  - `models.rs` data model · `config.rs` settings · `db/` SQLite ·
    `agents/` per-CLI adapters + stream-json parsing · `runner.rs` process runner ·
    `engine.rs` scheduling/roadmap/verify loop · `parse.rs` output contracts ·
    `conventions.rs` + `templates/` the `.orchestrator/` files · `event.rs` EventSink.
- `src-tauri/` — the Tauri host: `commands/` IPC surface, `state.rs` event bridge,
  `lib.rs` wiring.
- `src/` — React + TypeScript + Tailwind frontend: `api/` (typed IPC + `types.ts`),
  `store/` (zustand + live events), `views/`, `components/`.

## Architecture rules

- Keep orchestration logic in `orchestrator-core`. Expose it via Tauri commands in
  `src-tauri/src/commands`. Consume it from the typed API in `src/api`.
- When you change a Rust model, update the mirrored TypeScript type in
  `src/api/types.ts` (Rust serializes camelCase).
- Adding an agent → implement `AgentAdapter` in `crates/core/src/agents`.
- Adding a command → add the handler, register it in `generate_handler!`
  (`src-tauri/src/lib.rs`), and add a wrapper in `src/api/index.ts`.

## Build & verify (definition of done)

```bash
cargo test -p orchestrator-core        # engine unit/integration tests
cargo clippy -p orchestrator-core --all-targets -- -D warnings
cargo fmt --all -- --check
pnpm build                             # frontend typecheck + bundle
cargo check -p claude-orchestrator     # full Tauri crate (needs WebKit dev libs)
```

Building the Tauri crate on Linux requires the WebKit dev libraries:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev librsvg2-dev libgtk-3-dev
```

Run the app in development with `pnpm tauri dev`.

## Conventions

- Rust: small focused modules, `thiserror` errors, unit tests beside the code.
- TS: function components, the zustand store, shared `.card` / `.btn` / `.input`
  classes from `src/styles/index.css`.
- Keep changes scoped; don't refactor unrelated code. Add tests for new behavior.

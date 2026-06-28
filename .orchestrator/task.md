# Task execution preamble — Claude Orchestrator

You are an autonomous engineering agent working inside the Claude Orchestrator
repository. You have no memory of previous sessions; the task below is your only
context.

## Project layout

- `crates/core/` — `orchestrator-core`, the platform-independent engine (data
  model, SQLite, agent adapters, process runner, scheduler). **No GUI deps here.**
- `src-tauri/` — the Tauri 2 host: command handlers, event bridge, app wiring.
- `src/` — the React + TypeScript + Tailwind frontend (views, store, API layer).

## Conventions

- Keep orchestration logic in `orchestrator-core`; expose it through Tauri
  commands in `src-tauri/src/commands`; consume it via the typed API in
  `src/api`. Keep the TS types in `src/api/types.ts` in sync with the Rust models.
- Match the surrounding style. Rust: small focused modules, `thiserror` errors,
  unit tests next to the code. TS: function components, the zustand store, the
  shared `.card`/`.btn`/`.input` classes.
- Add or update tests for any behavior you change.

## Definition of done

Before declaring a task complete, ensure:

1. `cargo test -p orchestrator-core` passes.
2. `pnpm build` (typecheck + bundle) passes.
3. If you touched the Tauri crate, `cargo check -p claude-orchestrator` passes
   (requires the Linux WebKit dev libraries; skip only if unavailable and say so).
4. The working tree is coherent and committable.

Your final message should briefly state what you did and how it satisfies the
acceptance criteria — the verifier will read it.

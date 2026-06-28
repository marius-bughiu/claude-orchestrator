# Contributing to Claude Orchestrator

Thanks for your interest! This project is itself maintained by autonomous agents
(see [`.orchestrator/`](.orchestrator)), but human contributions are very welcome.

## Getting set up

```bash
pnpm install
pnpm tauri dev      # run the desktop app
```

Prerequisites: Rust (stable), Node 22 + pnpm, the `claude` CLI, and the Linux
WebKit dev libraries (see the [README](README.md)).

## Project layout

- `crates/core/` — `orchestrator-core`: the platform-independent engine (data
  model, SQLite, agent adapters, runner, scheduler). **No GUI dependencies.**
- `src-tauri/` — the Tauri host: IPC commands, event bridge, plugins.
- `src/` — the React + TypeScript + Tailwind frontend.

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`CLAUDE.md`](CLAUDE.md).

## Before you open a PR

Run the same checks CI does:

```bash
cargo test -p orchestrator-core
cargo clippy -p orchestrator-core --all-targets -- -D warnings
cargo fmt --all -- --check
pnpm build
cargo check -p claude-orchestrator   # needs the Linux WebKit dev libs
```

## Conventions

- **Keep orchestration logic in `orchestrator-core`**, expose it via a Tauri
  command, consume it from the typed API in `src/api`.
- When you change a Rust model, update the mirrored TypeScript type in
  `src/api/types.ts` (Rust serializes camelCase).
- Add tests for new behavior. Keep changes scoped; don't refactor unrelated code.
- Rust: small focused modules, `thiserror` errors, unit tests beside the code.
- TS: function components, the zustand store, the shared `.card`/`.btn`/`.input`
  classes.

## Reporting bugs / requesting features

Open an issue using one of the templates. For security issues, please see
[`SECURITY.md`](SECURITY.md) if present, or email the maintainer rather than
filing a public issue.

# `.orchestrator/` (self-hosted)

Claude Orchestrator manages itself. These files configure how the app runs
autonomous agents against **this** repository — the same convention files any
managed project can have.

| File          | Purpose |
|---------------|---------|
| `config.json` | Per-project settings. |
| `roadmap.md`  | Roadmap loop prompt — generates the product's next tasks when the queue empties. |
| `verify.md`   | Verifier prompt — checks finished tasks against `cargo test` / `pnpm build` reality. |
| `task.md`     | Preamble prepended to every task, encoding this repo's layout and definition-of-done. |
| `scheduled/`  | Recurring scheduled tasks (see `scheduled/example-dependency-audit.md`). |

If you remove a file, the orchestrator falls back to the built-in defaults
embedded in `crates/core/src/templates/`.

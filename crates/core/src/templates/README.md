# `.orchestrator/`

This directory configures how [Claude Orchestrator](https://github.com/marius-bughiu/claude-orchestrator)
runs autonomous agents against this repository. Every file is optional; the
orchestrator falls back to sensible built-in defaults when one is missing.

| File          | Purpose |
|---------------|---------|
| `config.json` | Per-project settings (default agent, concurrency, feature toggles). |
| `roadmap.md`  | Prompt for the **roadmap loop** — generates new tasks when the queue empties. |
| `verify.md`   | Prompt for the **verifier** — judges whether a finished task met its goal. |
| `task.md`     | Preamble prepended to every task prompt for this project. |

Edit these to steer autonomous behavior. The `roadmap.md` and `verify.md` files
each define a small JSON output contract the orchestrator parses — keep those
contracts intact when customizing.

See the [conventions guide](https://github.com/marius-bughiu/claude-orchestrator/blob/main/docs/CONVENTIONS.md)
for details.

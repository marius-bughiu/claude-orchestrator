# Project Conventions: the `.orchestrator/` directory

Every repository managed by Claude Orchestrator can carry a small set of
convention files under `.orchestrator/`. These files steer how autonomous agents
plan, execute, and verify work in that specific project.

## Overview

The convention layer lives in [`crates/core/src/conventions.rs`](../crates/core/src/conventions.rs),
with the default file contents embedded from
[`crates/core/src/templates/`](../crates/core/src/templates/) via `include_str!`.

Three properties make this safe and ergonomic:

- **Everything is optional.** Each file has an embedded default that is used
  whenever the corresponding file is absent. A bare git repo with no
  `.orchestrator/` directory works out of the box: `roadmap_prompt` and
  `verify_prompt` fall back to `DEFAULT_ROADMAP` / `DEFAULT_VERIFY`, and
  `task_preamble` returns `None`.
- **Scaffolding never clobbers.** `conventions::scaffold` writes the defaults
  into `.orchestrator/` but skips any file that already exists, so re-scaffolding
  preserves your customizations. It returns the list of files it actually
  created.
- **Two ways to scaffold.** Files are written when you add a project with the
  `scaffold` option enabled (`service::add_project` calls
  `conventions::scaffold` when `input.scaffold` is true), or on demand via the
  **Scaffold** button in the UI, which invokes the `scaffold_project` Tauri
  command (`src-tauri/src/commands/mod.rs`).

The directory name is fixed: `.orchestrator` (`conventions::DIR_NAME`).

## The files

| File          | Purpose |
|---------------|---------|
| `README.md`   | Human-facing explainer for the directory. Not consumed by the engine. |
| `config.json` | Per-project settings (default agent, concurrency, feature toggles). Currently advisory — see below. |
| `roadmap.md`  | Prompt for the **roadmap loop** — generates new tasks when the project's queue empties. Parsed for a JSON task array. |
| `verify.md`   | Prompt for the **verifier** — judges whether a finished task met its goal. Parsed for a JSON verdict. |
| `task.md`     | Preamble prepended to every task prompt for this project. |

Of these, `roadmap.md` and `verify.md` carry **machine-parsed JSON output
contracts**. Keep those contracts intact when you customize the prose around
them (details below).

## `config.json`

The scaffolded default (`crates/core/src/templates/config.json`):

```json
{
  "$schema": "https://github.com/marius-bughiu/claude-orchestrator/blob/main/docs/schema/project-config.json",
  "version": 1,
  "defaultAgent": "claude",
  "maxConcurrent": null,
  "roadmapEnabled": true,
  "verifyEnabled": true,
  "tags": []
}
```

| Field           | Meaning |
|-----------------|---------|
| `$schema`       | JSON Schema reference for editor tooling. |
| `version`       | Config format version. |
| `defaultAgent`  | Default agent for tasks that don't pin one. One of `claude`, `gemini`, `codex`. |
| `maxConcurrent` | Per-project cap on concurrent sessions. `null` means "use the global setting". |
| `roadmapEnabled`| Whether the roadmap loop may auto-generate tasks when the queue empties. |
| `verifyEnabled` | Whether finished tasks are auto-verified by a verifier session. |
| `tags`          | Free-form project-level labels. |

> **Note: `config.json` is currently advisory.** The scheduler reads the
> authoritative values from the project record in the database
> (`Project.default_agent`, `Project.max_concurrent`, `Project.roadmap_enabled`,
> `Project.verify_enabled` in `crates/core/src/models.rs`), which you edit
> through the app's project settings. `config.json` documents intended
> configuration alongside the code and is reserved for future use; it is not
> parsed back into the runtime today. The fields above mirror the per-project
> knobs exactly so the two stay conceptually aligned.

## `roadmap.md`

**Role.** The roadmap loop runs automatically whenever a project's task queue
empties (and both the global and per-project `roadmap_enabled` flags are on — see
the gate in `Engine::tick` in `engine.rs`). Its job is to read the project's vision and emit a
batch of concrete, independent, runnable tasks for agents to pick up. The
session uses the project's `default_agent`.

The default prompt (`crates/core/src/templates/roadmap.md`) instructs the planner
to:

- Read direction from `ROADMAP.md`, `docs/ROADMAP.md`, a README roadmap section,
  `TODO.md`, `// TODO` / `FIXME` comments, failing/skipped tests, recent git
  history, and design docs under `docs/`.
- Slice work into the smallest next increments, each completable in one focused
  session and self-contained (a fresh agent has no memory of the planning step).
- Inspect the codebase to avoid duplicating finished work.
- Emit between 1 and 8 tasks, quality over quantity — and emit an **empty array
  `[]`** if there is genuinely nothing valuable to do, rather than inventing
  busywork.

### Output contract (machine-parsed)

The planner must end its response with **exactly one** fenced ` ```json ` block
containing an array of task objects. Parsing is implemented in
[`crates/core/src/parse.rs`](../crates/core/src/parse.rs)
(`parse_roadmap_tasks` / `extract_last_json`). Be exact:

- The engine extracts the **last** fenced JSON block in the text. A fence counts
  as JSON if its language tag is empty or `json` (case-insensitive). Everything
  before it is treated as reasoning and ignored, so put your thinking first.
- If no fenced block parses, it falls back to the **last balanced** top-level
  `{...}` or `[...]` run in the text that parses as JSON. This is why prose
  braces like "I considered {this}" don't break parsing — only the final
  balanced JSON run is used.
- The parsed value may be **either a bare array** of task objects **or an object
  with a `"tasks"` array**. Any other shape yields no tasks.
- Each task object's schema:

```json
[
  {
    "title": "Short imperative summary",
    "description": "Full instructions and acceptance criteria for the executing agent. Be explicit; the executor has no other context.",
    "priority": 50,
    "agent": "claude",
    "tags": ["area:backend"]
  }
]
```

| Field         | Required | Default | Meaning |
|---------------|----------|---------|---------|
| `title`       | yes      | —       | One-line imperative summary. Tasks with an empty/whitespace title are dropped. |
| `description` | no       | `""`    | Everything the executor needs; it has no other context. |
| `priority`    | no       | `50`    | Higher runs first. Convention: 0 low, 50 normal, 100 high, 200 urgent. |
| `agent`       | no       | project default | One of `claude`, `gemini`, `codex`. Unrecognized values fall back to the project default. |
| `tags`        | no       | `[]`    | Free-form labels for filtering. |

- An **empty array `[]`** is valid and means "no new work"; the engine logs
  "roadmap generated no new tasks" and creates nothing.
- Generated tasks are inserted as `Pending`, flagged `auto_generated`, and start
  with `attempts: 0` and `max_attempts: 3` (`engine.rs::handle_roadmap_outcome`).

## `verify.md`

**Role.** After a task session finishes successfully, and if verification is
enabled (both global and per-project `verify_enabled`), the engine runs a
**verifier** session using the project's `default_agent`
(`engine.rs::run_verification`). The verifier is handed the task title, its
description / acceptance criteria, and the executing session's final result text,
and runs inside the project repository so it can inspect the actual code, tests,
and behavior. The default prompt instructs it to judge against reality rather
than the executor's narrative, and to be strict but fair.

### Verdict contract (machine-parsed)

The verifier must end with **exactly one** fenced ` ```json ` block (same
last-fenced-then-last-balanced extraction as roadmap; `parse_verdict` in
`parse.rs`):

```json
{
  "complete": true,
  "reason": "One or two sentences explaining the verdict.",
  "follow_up": "If complete is false: the exact instruction for the next attempt. Empty string if complete."
}
```

| Field       | Required | Default | Meaning |
|-------------|----------|---------|---------|
| `complete`  | yes      | —       | Boolean. Whether the task's goal was actually achieved. |
| `reason`    | no       | `""`    | Short explanation of the verdict. |
| `follow_up` | no (required when `complete` is false) | `""` | Concrete, actionable instruction for the next attempt. |

### What happens with the verdict

Handled in `engine.rs::handle_task_outcome`:

- **`complete: true`** → the task is marked `Completed`.
- **`complete: false`** → feedback (the `follow_up`, or the `reason` if
  `follow_up` is blank) is appended to the task's description, and the task's
  attempt counter is incremented. If `attempts >= max_attempts` the task becomes
  `Failed`; otherwise it becomes **`needs_review`** and is re-queued for another
  attempt — this time carrying the reviewer's feedback so the next executor sees
  exactly what was missing.
- **No parseable verdict** → the engine logs a warning, trusts the executor, and
  marks the task `Completed`.

(A task whose session fails outright, before verification, follows the same
`attempts`/`max_attempts` rule: `needs_review` until the cap, then `Failed`.)

## `task.md`

`task.md` is a preamble prepended to **every** task prompt for the project
(`engine.rs::task_prompt`, via `conventions::task_preamble`). When present, its
contents are emitted first, followed by a `---` separator, then the task's title
and description. If the file is absent, no preamble is added.

Use it to encode project-wide conventions once — coding style, test commands,
"leave the tree committable", "don't refactor unrelated code" — so individual
task descriptions can stay focused on the specific goal. The default
(`crates/core/src/templates/task.md`) sets up an autonomous engineering agent
with no memory of prior sessions and operating principles like "implement, don't
just plan", "run tests/linters before declaring done", and "your final message
will be read by the verifier".

## Worked example: customizing for a project

Suppose a Rust web service where you want the roadmap loop to focus on closing
`// TODO` comments and adding integration tests, and you want the verifier to
insist that `cargo test` passes.

`.orchestrator/roadmap.md`:

```markdown
# Roadmap loop — payments-api

You plan work for the payments-api service. Generate the next batch of tasks.

Priorities for this project, in order:
1. Resolve open `// TODO(payments)` comments in `src/`.
2. Add integration tests under `tests/` for any handler lacking coverage.
3. Tighten error handling on the `/charge` and `/refund` endpoints.

Each task must be runnable in one session by an agent with no other context.
Default the executor to `codex` for test-writing tasks. If nothing above
applies, emit `[]`.

End with exactly one ```json block — a bare array of task objects:

```json
[
  {
    "title": "Add integration test for POST /refund",
    "description": "Cover the happy path and the double-refund error in tests/refund.rs. Acceptance: `cargo test` passes and the new test fails if the idempotency check is removed.",
    "priority": 100,
    "agent": "codex",
    "tags": ["area:payments", "tests"]
  }
]
```
```

`.orchestrator/verify.md`:

```markdown
# Verifier — payments-api

Judge whether the task was actually completed. You are in the repo; inspect it.

Hard requirements for *every* task in this project:
- `cargo test` must pass. Run it.
- No new `// TODO(payments)` may be introduced.

If either fails, set `complete: false` and put the exact remediation in
`follow_up` (e.g. which test fails and why).

End with exactly one ```json block:

```json
{ "complete": true, "reason": "cargo test passes; refund idempotency covered", "follow_up": "" }
```
```

The prose above the final fenced block is free to be as project-specific as you
like — only the last JSON block is parsed, against the schemas documented above.

## This repository dogfoods these files

Claude Orchestrator manages itself: there is an `.orchestrator/` directory at the
repository root (`config.json`, `roadmap.md`, `verify.md`, `task.md`,
`README.md`) tuned for this codebase. It is the reference example of the
conventions described here — read it alongside the templates in
`crates/core/src/templates/` to see defaults and a real customization
side by side.

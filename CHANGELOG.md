# Changelog

All notable changes to Claude Orchestrator are documented here. Releases are cut
automatically on every push to `main` (version `0.1.<build>`); this file groups
the notable user-facing changes.

## Unreleased

### Added
- **Token-by-token streaming** of agent output in the session view (live caret).
- **Live mid-run message injection** — type into a running session and the
  message is delivered to the agent on stdin without interrupting it; finished
  sessions resume in a new session instead.
- **Desktop notifications** when a task completes or fails (toggle in Settings).
- **Command palette** (Cmd/Ctrl+K) for navigation and quick actions.
- **Per-project git status** (branch, dirty, ahead/behind, last commit) on
  project cards.
- **Task search** across title, description, and tags.
- **Dashboard CSV export** of the current usage series.
- **System (auto) theme** option alongside dark and light.
- **Usage dashboards** — cost, tokens, sessions, and turns per day/month/year.
- **Header usage meters** — per-agent session and weekly percent-of-limit.
- **Scheduled tasks** — recurring jobs from `.orchestrator/scheduled/*.md`.
- **Upcoming** section projecting the next scheduled firings.
- **Per-project allowed agents** and usage-balanced dispatch.
- **Auto-update** with graceful drain, and per-push releases for macOS, Windows,
  and Linux.

### Changed
- Reworked the usage model into session + weekly rolling windows.

> Earlier history predates this changelog; see the git log.

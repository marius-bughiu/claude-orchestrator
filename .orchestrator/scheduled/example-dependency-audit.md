---
schedule: "0 9 * * 1"   # every Monday at 09:00 (5-field cron; seconds optional)
agent: claude            # optional — omit to let the scheduler balance load
model: opus              # optional — defaults to the agent's latest
priority: normal
enabled: false           # disabled by default; flip to true to activate
title: Weekly dependency audit
---
Audit this repository's dependencies for outdated or vulnerable packages.

1. Check the Rust crates (`cargo update --dry-run`, advisories) and the npm
   packages (`pnpm outdated`).
2. For anything materially out of date or flagged, open a concise follow-up task
   describing the upgrade and its risk.
3. Do not perform the upgrades here — just report and queue the work.

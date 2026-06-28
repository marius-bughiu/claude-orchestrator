## Summary

<!-- What does this PR change and why? -->

## Changes

<!-- Bullet the notable changes. -->
-

## Checks

- [ ] `cargo test -p orchestrator-core`
- [ ] `cargo clippy -p orchestrator-core --all-targets -- -D warnings`
- [ ] `cargo fmt --all -- --check`
- [ ] `pnpm build`
- [ ] `cargo check -p claude-orchestrator` (if the Tauri crate changed)
- [ ] Updated `src/api/types.ts` if a Rust model changed
- [ ] Added/updated tests for new behavior

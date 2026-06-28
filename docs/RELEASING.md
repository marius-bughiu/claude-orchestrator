# Releasing & auto-updates

Claude Orchestrator ships a release on **every push to `main`** and the desktop
app **auto-updates** from that channel.

## How it works

- `.github/workflows/release.yml` runs on each push to `main` (code changes only;
  doc-only pushes are skipped). It:
  1. Derives a version `0.1.<run_number>` (monotonically increasing).
  2. Stamps that version into `package.json`, `src-tauri/tauri.conf.json`, and the
     workspace `Cargo.toml`.
  3. Builds installers for **macOS (Apple Silicon + Intel), Windows, and Linux**
     via [`tauri-action`](https://github.com/tauri-apps/tauri-action).
  4. Publishes a GitHub Release tagged `app-v<version>` with the installers and a
     signed `latest.json` updater manifest.
- The app's updater (`tauri-plugin-updater`) is pointed at
  `releases/latest/download/latest.json`. On launch it checks that endpoint; when
  a newer version exists the in-app banner offers **Update & restart**.
- Choosing to update **drains the scheduler first**: no new jobs are scheduled,
  in-flight sessions are allowed to finish, then the update is downloaded,
  installed, and the app relaunches.

## One-time signing setup (required for working updates)

Updates must be cryptographically signed. The public key is committed in
`src-tauri/tauri.conf.json` (`plugins.updater.pubkey`); the matching **private
key** lives only in CI secrets. The committed key is a placeholder — generate
your own before publishing real releases:

```bash
# Generate a keypair (set a password when prompted).
pnpm tauri signer generate -w ~/.tauri/claude-orchestrator.key

# Copy the printed public key into src-tauri/tauri.conf.json -> plugins.updater.pubkey
```

Then add two **repository secrets** (Settings → Secrets and variables → Actions):

| Secret | Value |
|--------|-------|
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of the generated private key file. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password you set (empty if none). |

> Keep the private key safe. If you lose it, existing installs can no longer
> verify updates and users must reinstall.

## Code signing & notarization (optional but recommended)

Unsigned macOS/Windows builds will warn users on first launch. To sign:

- **macOS**: set `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` secrets;
  `tauri-action` picks them up automatically.
- **Windows**: configure `bundle.windows.certificateThumbprint` (or an Azure Code
  Signing setup) in `tauri.conf.json`.

## Cutting a manual release

Trigger the workflow by hand from the **Actions** tab (`workflow_dispatch`), or
just push to `main`.

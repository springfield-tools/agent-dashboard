# agent-dashboard

A Mac menu-bar dashboard for tracking Cursor cloud agent work across GitHub repos. The Tauri 2 tray app polls on a configurable interval (default 60 seconds), can pause from the tray, and groups issues and pull requests by status: **Agent working** (`agent-working`), **Needs testing** (`needs-testing`), **Needs input** (`needs-input`), **Ready to merge** (`ready-to-merge`), **CI failing**, and **Not started**.

The implementation plan is in [`docs/PLAN.md`](docs/PLAN.md). Accepted architecture decisions are in [`docs/adr/`](docs/adr/). A separate daily morning summary lives outside this repo; the plan describes the snapshot file it can read.

## Downloads

macOS (Apple Silicon) builds are on [GitHub Releases](https://github.com/springfield-tools/agent-dashboard/releases). Packaged builds are **unsigned** (open the `.dmg`, then right-click the app → **Open** if Gatekeeper blocks). For development from source, use the checkout at `~/Development/springfield-tools/agent-dashboard`.

## Development

Node.js 26 (see `.nvmrc`) and Yarn 4 via Corepack. Rust **1.90 or newer** (see `rust-version` in `src-tauri/Cargo.toml`; the locked `tray-icon` crate requires 1.90) and [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) are required on macOS.

Runtime config is written on first launch to `~/Library/Application Support/com.springfield.agent-dashboard/config.toml`. See [`config.example.toml`](config.example.toml). Live mode polls GitHub; offline fixture demo: `yarn tauri:dev:fixture`.

### GitHub fine-grained PAT (one-time)

1. GitHub → **Settings** → **Developer settings** → **Fine-grained tokens** → **Generate new token**.
2. **Resource owner:** `springfield-ham-radio`. If the org does not appear, an owner must allow fine-grained PATs. If the org requires approval, submit a justification; the token can read public repos until approved.
3. **Repository access:** All repositories.
4. **Permissions:** **Metadata** Read (automatic), **Issues** Read, **Pull requests** Read, **Commit statuses** Read. There is no **Checks** permission on fine-grained tokens. For **private** repositories, also grant **Contents** Read if commit `statusCheckRollup` data is denied (the app shows a per-org banner and gray **CI ?** chips instead of raw errors).
5. **Expiration:** 90 days (or your org’s shorter maximum).
6. In the app, open **Settings** and paste the token once (stored in Keychain only).

`statusCheckRollup.state` drives CI grouping; check **names** may be missing without the Checks API. The app still shows **CI failing** from rollup state.

**Alternative:** set `credential = "gh"` under `[github]` in `~/Library/Application Support/com.springfield.agent-dashboard/config.toml`. The app runs `gh auth token` each poll (token cached in memory for the session, cleared on 401). Requires `gh` on PATH or in `/opt/homebrew/bin` or `/usr/local/bin`, and `gh auth login`. Nothing is persisted to disk or logs (see ADR 0002).

```bash
cd ~/Development/springfield-tools/agent-dashboard
corepack enable
yarn install
yarn tauri:dev
```

Other commands:

```bash
yarn lint
yarn test
cargo test --manifest-path src-tauri/Cargo.toml
```

## License

[MIT](LICENSE)

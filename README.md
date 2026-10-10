# agent-dashboard

A Mac menu-bar dashboard for tracking Cursor cloud agent work across GitHub repos. The Tauri 2 tray app polls on a configurable interval (default 30 seconds), can pause from the tray, and groups issues and pull requests by status: **Agent working** (`agent-working`), **Needs testing** (`needs-testing`), **Needs input** (`needs-input`), **Ready to merge** (`ready-to-merge`), **CI failing**, and **Not started**.

The implementation plan is in [`docs/PLAN.md`](docs/PLAN.md). Accepted architecture decisions are in [`docs/adr/`](docs/adr/). A separate daily morning summary lives outside this repo; the plan describes the snapshot file it can read.

## Development

Node.js 26 (see `.nvmrc`) and Yarn 4 via Corepack. Rust **1.90 or newer** (see `rust-version` in `src-tauri/Cargo.toml`; the locked `tray-icon` crate requires 1.90) and [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) are required on macOS.

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

Packaged macOS builds are **unsigned** in early phases (right-click → Open if Gatekeeper blocks).

## License

[MIT](LICENSE)

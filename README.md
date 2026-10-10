# agent-dashboard

A Mac menu-bar dashboard for tracking Cursor cloud agent work across GitHub repos. The Tauri 2 tray app polls on a configurable interval (default 30 seconds), can pause from the tray, and groups issues and pull requests by status: **Agent working** (`agent-working`), **Needs testing** (`needs-testing`), **Needs input** (`needs-input`), **Ready to merge** (`ready-to-merge`), **CI failing**, and **Not started**.

The implementation plan is in [`docs/PLAN.md`](docs/PLAN.md). A separate daily morning summary lives outside this repo; the plan describes the snapshot file it can read.

## License

[MIT](LICENSE)

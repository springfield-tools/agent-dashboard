# 0001. Tauri 2 tray app with Vue 3 and Vite

## Status

Accepted

## Context

The product is a macOS menu-bar dashboard with a single popover UI, background polling, and Keychain-backed secrets. It must stay small, run without a browser tab, and share tooling with `springfield-ham-radio/ham-radio-ui` where practical.

## Decision

- **Shell:** Tauri 2 on macOS with `ActivationPolicy::Accessory` (no Dock icon), system tray, and a borderless always-on-top webview for the popover.
- **Frontend:** Vue 3 + Vite + TypeScript in `src/`, not Nuxt (no router or SSR).
- **Backend:** Rust in `src-tauri/` for tray, polling, GitHub, and Keychain (phased).

## Consequences

- macOS is the primary target; Linux builds may show the popover window for development.
- CI `build` runs on `macos-latest` and compiles the Rust crate.
- Release signing and updater artifacts are deferred per the implementation plan.

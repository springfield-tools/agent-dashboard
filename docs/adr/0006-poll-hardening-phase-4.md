# 0006. Phase 4 poll hardening (wake, idle, ETag, brakes)

## Status

Accepted (2026-10-11)

## Context

Phase 3 shipped live GraphQL polling with user pause. Phase 4 needs resilient polling: rate-limit brakes and backoff, wake-from-sleep refresh, optional REST ETag skip, optional idle auto-pause, and clearer offline/stale UX — without clearing manual pauses on wake or idle resume.

## Decision

- **Backoff** lives in `src-tauri/src/backoff.rs` with unit tests (transient 30→300s jitter, primary wait until `resetAt` + 1–5s jitter, secondary `Retry-After`).
- **Brakes** live in `src-tauri/src/brakes.rs`: projected hour cap 2,500, five-minute polling when `remaining < 500`, pause until `resetAt` when `remaining < 100`, runtime-only slowdown when projection exceeds the cap (config interval unchanged).
- **Wake**: macOS `NSWorkspaceDidWakeNotification` in `src-tauri/src/power.rs` triggers one poll when user pause is not active; timed/manual pause survives wake.
- **Idle**: `CGEventSourceSecondsSinceLastEventType` in `src-tauri/src/idle.rs`; runtime `idle_pause_active` (not `poll_pause`); manual/timed pause wins over idle resume.
- **ETag probe**: `src-tauri/src/etag_probe.rs`, persisted `etag-store.json` beside config; default off; skips GraphQL only when every probed repo returns 304 and snapshot is younger than `ci_max_staleness_seconds`.
- **In-flight guard**: at most one GitHub request; sleep scheduled from poll completion.

## Consequences

- CI can lag when ETag probe skips GraphQL; popover banner states staleness.
- 15s interval with heavy ticks may hit the projection brake; the 60s default (~240 pts/hr at 4 pts/tick) stays well under 2,500/hr for one org.
- Wake listener is macOS-only; non-macOS dev builds omit AppKit wiring.

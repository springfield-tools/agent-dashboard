# 0005. Offline demo via `--fixture` CLI flag

## Status

Accepted

## Context

Phase 2 validated grouping against a checked-in GraphQL-shaped fixture without network or tokens. Phase 3 makes live polling the default. CI and local demos still need the fixture path without a PAT.

## Decision

- Pass `--fixture` on the Rust binary (for example `yarn tauri dev -- --fixture`) to skip the background poll loop and load `src-tauri/fixtures/ham-radio-open.json` on startup.
- Live mode loads the last `snapshot.json` from Application Support when present, then polls GitHub on the configured interval.

## Consequences

- Bryan tests live GitHub with `yarn tauri:dev`; fixture demos and docs use `--fixture`.
- The popover shows `Fixture` in the header when the flag is set.

# ADR 0010: Per-org enabled toggle

## Status

Accepted (2026-10-14)

## Context

Bryan runs three orgs by default (`springfield-ham-radio`, `springfield-tools`, `springfield-usa`). He sometimes wants to stop polling one org (for example `springfield-usa`) without removing it from config or losing token setup when he turns it back on.

## Decision

- Add `enabled` on each `[[orgs]]` table (`true` by default). Omitting the key behaves as `true`.
- Persist toggles from Settings with `toml_edit` so comments and ordering in `config.toml` stay intact.
- When `enabled = false`:
  - Skip the org in ETag probe and GraphQL multi-org fetch (no token resolution for that org).
  - Do not count the org toward global auth failure when every *enabled* org lacks credentials.
  - Exclude its rows from popover org sections, global tray counts, and GraphQL budget footer lines.
  - Do not show per-org auth banners for that org.
  - Reject row write actions targeting that org.
- Settings still lists disabled orgs (dimmed) with an on/off control next to token status.
- The popover footer links to Settings when any org is disabled (`N project(s) off`).
- Turning an org on saves config and forces an immediate poll cycle for live data.

## Consequences

- Disabled org rows are dropped from the in-memory snapshot on toggle-off without a network call; stale usa data does not linger in tray counts.
- `credential_is_ready` and global auth banners only consider enabled orgs.
- Snapshot schema version stays at 2; `disabledOrgCount` is an optional additive field for the footer.

# ADR 0008: Per-org credentials and popover grouping (Phase 5.1)

## Status

Accepted (2026-10-11)

## Context

Fine-grained GitHub PATs have a single resource owner. Bryan tracks work across `springfield-ham-radio`, `springfield-tools`, and `springfield-usa`. Phase 5 row writes and Phase 3 polling used one global token. The popover listed six status groups only; org was present on each row but not used for layout or auth.

## Decision

1. **Per-org credential** — Each `[[orgs]]` entry may set `credential = "keychain"` or `credential = "gh"`. When omitted, `[github].credential` applies. Keychain items use account `github-token-{org}` under service `com.springfield.agent-dashboard`. The legacy account `github-token` migrates automatically into `github-token-springfield-ham-radio` on first read.

2. **Polling** — Each org fetch uses that org's resolved token. Credential or fetch failures set an `authBanner` on that org's section only; other orgs keep polling. GraphQL cost and remaining are tracked per credential bucket (`org` name for Keychain PATs, `gh` for CLI). Footer shows each bucket; brakes use the minimum remaining across buckets used in the tick.

3. **Snapshot** — `schemaVersion` 2 adds `orgSections` (org name, per-org tray counts, optional `authBanner`, nested status `groups`) and `graphqlBudgets`. Top-level `groups` remain the flattened union for tray math and write reconciliation.

4. **UI** — Popover groups by org in collapsible `<details>` sections. Expanded/collapsed state persists in `ui-state.json` beside config. Row chips carry a `variant` for tinted styling (issue/PR, status labels, CI).

## Consequences

- Settings saves tokens per org via `store_github_token(org, token)`.
- Row writes resolve the token for `row.org`.
- Multi-org installs need one PAT per resource owner (or `gh` where appropriate).
- Consumers of `snapshot.json` should prefer `orgSections` when present.

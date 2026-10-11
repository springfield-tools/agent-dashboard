# Architecture decision records

Accepted decisions for Agent Dashboard. Bryan approved `docs/PLAN.md`; where open questions were unresolved, implementation follows the plan’s recommendations and these ADRs.

| ADR | Title |
| --- | --- |
| [0001](0001-tauri-vue-stack.md) | Tauri 2 tray app with Vue 3 and Vite |
| [0002](0002-github-auth-keychain-pat.md) | GitHub auth via fine-grained PAT in Keychain |
| [0003](0003-graphql-polling-and-interval.md) | GraphQL org search, `statusCheckRollup`, 60s default interval |
| [0004](0004-status-group-precedence.md) | Status groups and precedence |
| [0005](0005-offline-fixture-flag.md) | Offline demo via `--fixture` CLI flag |
| [0006](0006-poll-hardening-phase-4.md) | Wake, idle pause, ETag probe, brakes/backoff |
| [0007](0007-row-write-actions-phase-5.md) | Phase 5 row actions, `@cursor` comments, undo |
| [0008](0008-multi-org-credentials-and-grouping.md) | Per-org Keychain PATs, org sections, chip variants |

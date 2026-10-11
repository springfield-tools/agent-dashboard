# 0007. Phase 5 row write actions (REST comments and label diffs)

## Status

Accepted (2026-10-11)

## Context

Phases 1–4 are read-only polling. Bryan needs to move work through status labels and wake Cursor cloud agents via `@cursor` GitHub comments without leaving the menu bar. GraphQL polling budget must stay unchanged; writes are infrequent REST mutations on user confirm.

## Decision

- **No Cursor API** — handoff is `POST /repos/{owner}/{repo}/issues/{number}/comments` with `@cursor` as the first token when waking an agent; audit comments omit `@cursor`.
- **Order on confirm** — post comment, then add/remove **status labels only** (`LabelsConfig` names); never `PUT` the full label set.
- **No automatic retries** — failures surface in the popover; optional **Retry labels** when comment succeeded but labels failed.
- **Undo** — 60 seconds or until the next successful poll; restore prior labels; delete audit comments only when the body did not start with `@cursor`.
- **Auth** — same `credential` modes as Phase 3 (`keychain` fine-grained PAT or `gh` OAuth); 403 shows a banner explaining Issues + Pull requests **Read and write**.
- **No merge** from the dashboard (PLAN §6 / open question 18).

## Consequences

- Fine-grained PATs must be upgraded from read-only to Issues/Pull requests write; Contents remains off.
- `@cursor` on issues starts a new cloud agent session each time (PLAN open question 17 — accepted).
- Approve is offered on **Not started** and **Agent working** (open question 16 — plan default yes).
- On **Needs testing**, **Ready to merge** is the default confirm on the Testing passed sheet; **Hand back** is the secondary button (open question 15 — plan default).

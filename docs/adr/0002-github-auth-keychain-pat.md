# 0002. GitHub auth via fine-grained PAT in Keychain

## Status

Accepted

## Context

The app polls GitHub on an interval and will later post comments and change labels. Credentials must not live in config files, logs, or the webview. Bryan already uses `gh` locally; device-flow OAuth would embed a client secret in a desktop binary.

## Decision

- Store a **fine-grained personal access token** in the macOS Keychain (`com.springfield.agent-dashboard` / `github-token`) via the `keyring` crate (Phase 3+).
- **Phases 1–4:** Issues, Pull requests, and Checks **read** only.
- **Phase 5:** Issues and Pull requests **read and write**; Contents stays off (no push or merge).
- Optional config `credential = "gh"` shells out to `gh auth token` for convenience.

## Consequences

- Org approval may be required for fine-grained tokens.
- Token rotation is a paste-into-settings flow; HTTP 401/403 surfaces in the popover.
- `gh auth token` remains broader than the app needs if selected.

# 0002. GitHub auth via fine-grained PAT in Keychain

## Status

Accepted

## Context

The app polls GitHub on an interval and will later post comments and change labels. Credentials must not live in config files, logs, or the webview. Bryan already uses `gh` locally; device-flow OAuth would embed a client secret in a desktop binary.

Fine-grained PATs are limited to one resource owner per token. GitHub documents that they **cannot call the Checks API**; repository permissions include **Commit statuses** (`statuses`), not a separate **Checks** entry.

## Decision

- Store a **fine-grained personal access token** in the macOS Keychain (`com.springfield.agent-dashboard` / `github-token`) via the `keyring` crate (Phase 3+).
- **Token setup (document in README and PR):**
  - Resource owner: `springfield-ham-radio` (add other orgs when they appear in config).
  - If the org blocks fine-grained PATs, it will not appear as a resource owner until an org owner allows them. If the org requires approval, the token stays pending until approved (public repos only until then).
  - Repository access: all repositories in that org.
  - Repository permissions: **Metadata** Read (mandatory), **Issues** Read, **Pull requests** Read, **Commit statuses** Read.
  - Do **not** grant a Checks permission (it does not exist on fine-grained tokens). Leave **Contents** off unless `statusCheckRollup` comes back empty on a private repo.
  - Expiration: 90 days, or the org/enterprise maximum if shorter.
- **Phases 1–4:** read-only on those permissions.
- **Phase 5:** Issues and Pull requests **read and write**; Contents stays off (no push or merge).
- Optional config `credential = "gh"` shells out to `gh auth token` when CheckRun names or broader API access matter; that token is broader than the app needs.

## CI rollup behavior

Grouping uses GraphQL `statusCheckRollup.state` (FAILURE, PENDING, etc.). Failed-check **names** come from `CheckRun` nodes in rollup contexts; fine-grained PATs may return rollup state without those names. The app still places PRs in **CI failing** from rollup state alone; chips may list no check names. For full check names, use `credential = "gh"` or wait for GitHub fine-grained coverage to improve.

## Consequences

- Org approval may be required for fine-grained tokens.
- Token rotation is a paste-into-settings flow; HTTP 401 and permission 403 surfaces in the popover (distinct from rate-limit responses).
- `gh auth token` remains broader than the app needs if selected.

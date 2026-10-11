# 0009. Release pipeline and dedicated GitHub App

## Status

Accepted (Phase 6).

## Context

Releases use [semantic-release](https://github.com/semantic-release/semantic-release) on pushes to `main` (and `beta` prereleases). The default `GITHUB_TOKEN` cannot push the release version commit through branch protection on `main`. Ham-radio-ui uses a dedicated GitHub App; **agent-dashboard uses its own app**, not ham-radio-ui’s, so permissions and audit scope stay limited to this repository.

## Decision

- **Versioning:** Conventional Commits on `main` drive semantic-release. Plugins match `package.json` `release` config: changelog, `package.json`, `src-tauri/Cargo.toml` / `Cargo.lock` (via `scripts/set-cargo-version.mjs`), a git commit `chore(release): <version> [skip ci]`, then `@semantic-release/github` so the tag points at the version-bump commit (not the pre-bump SHA).
- **CI:** The `release` job runs after `build` on pushes to `main` / `beta`. If `RELEASE_APP_ID` or `RELEASE_APP_PRIVATE_KEY` is unset, the job logs a skip and exits successfully so `main` stays green before Bryan wires secrets.
- **Auth:** `actions/create-github-app-token@v2` mints a token from repository (or org) secrets `RELEASE_APP_ID` and `RELEASE_APP_PRIVATE_KEY`.
- **Artifacts:** A follow-on `tauri` job in `ci.yml` runs on `macos-latest` only, building an **unsigned** Apple Silicon `.app` / `.dmg` via `tauri-apps/tauri-action@v1` and attaching assets to the GitHub Release. `.github/workflows/release-tauri.yml` is **workflow_dispatch** only for manual rebuilds of a tag.
- **Dependabot:** Weekly grouped updates for npm, GitHub Actions, and Cargo under `src-tauri`.

## Version commit on `main`

We keep `@semantic-release/git` so `package.json`, `Cargo.toml`, and `CHANGELOG.md` on `main` match the release tag. The commit message includes `[skip ci]` to avoid a second full CI run on the bookkeeping commit. If branch protection blocks direct pushes, add the release GitHub App as a **bypass actor** for `main` (or allow that app to push). Dropping the git plugin would avoid a version commit but leave `main` out of sync with tags; we prefer the ham-radio-ui pattern and a bypass for the app.

## Consequences

- Bryan must create and install a **separate** GitHub App on `springfield-tools/agent-dashboard` and store the two secrets before the first automated release.
- Until secrets exist, the `release` job still runs on `main` pushes but skips semantic-release successfully; `build` passes and no GitHub Release is created.
- macOS artifacts are unsigned; README documents Gatekeeper bypass and the local dev path.

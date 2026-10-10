# Agent Dashboard — implementation plan

Menu-bar app for Bryan Hunt that shows in-flight GitHub work across his orgs and refreshes every 30 seconds. This document is the plan only. A follow-up agent implements it after review.

The daily morning summary stays outside this repo. This app writes a versioned snapshot file the summary can read, so the two do not each invent a GitHub client.

Local checkouts, used in every setup and test step below:

- This repo: `~/Development/springfield-tools/agent-dashboard`
- Ham radio repos: `~/Development/springfield-ham-radio/<repo>`

## Recommendation

Build a Tauri 2 macOS tray app. Rust owns polling, the keychain, and grouping. Vue 3 renders one popover. Ship a fine-grained read-only personal access token in the macOS Keychain. Poll with one GitHub GraphQL search per org every 30 seconds, and read CI from `statusCheckRollup`. Keep the bot filter configurable, defaulting to a collapsed Dependabot row that still surfaces failing bot PRs.

## Snapshot of the data (2026-10-10)

Verified with the GitHub REST and GraphQL APIs. `springfield-ham-radio` has these 12 repositories, all public, default branch `main`:

`ham-radio-ui`, `ham-radio-ops`, `ham-radio-sniffer`, `ham-radio-registry`, `radio-module-baofeng`, `radio-module-kenwood`, `ham-radio-api`, `ham-radio-driver`, `ham-radio-utils`, `.github`, `ham-radio-docs`, `radio-module-catalog`.

| What | Count |
| --- | --- |
| Open issues (REST issues list, PRs removed) | 0 |
| Open pull requests | 33 |
| Of those, author `dependabot[bot]` | 31 |
| Dependabot PRs whose `statusCheckRollup` is `SUCCESS` | 29 |
| Dependabot PRs with no checks (`ham-radio-docs` #13 and #14) | 2 |
| Human PRs | 2, both drafts: `ham-radio-ui` #84 and `.github` #4 |

No open issue or PR currently carries `agent-working`, `needs-testing`, `needs-input`, or `ready-to-merge`. Those labels do exist, with the same names, colors, and descriptions, on `springfield-ham-radio/ham-radio-ui` and on `springfield-tools/agent-dashboard`:

| Label | Color | Description |
| --- | --- | --- |
| `agent-working` | `1d76db` | A cloud agent is actively working on this |
| `needs-testing` | `fbca04` | Ready for Bryan to test locally |
| `needs-input` | `d93f0b` | Waiting on a decision or answer from Bryan |
| `ready-to-merge` | `0e8a16` | Tested and approved, waiting on merge |

CI evidence, `springfield-ham-radio/ham-radio-ui` pull request 79: `GET /repos/.../commits/<head>/status` returns `state: pending`, `total_count: 0`, and an empty `statuses` array. The same commit has three check runs (`build` success, `release` skipped, `tauri` skipped), and GraphQL `statusCheckRollup.state` is `SUCCESS`. A one-page GraphQL search of `org:springfield-ham-radio is:open` returned all 33 PRs and cost 2 points. Every PR's legacy `commit.status` was null. The dashboard must treat check runs (via `statusCheckRollup`) as the CI source. A rollup that is missing, and a combined status of `pending` with zero statuses, both mean "no CI", which is a different state from "CI pending" and from "CI failing".

`springfield-tools` had no open issues or pull requests at the same time.

## 1. Product

A Tauri 2 macOS menu-bar app. It does not appear in the Dock (`ActivationPolicy::Accessory`). The poll loop lives in Rust so it keeps running while the popover is closed.

### Tray

The status item is a monochrome template image plus a short title. The title is the counts Bryan acts on, in this fixed order, so a glance stays meaningful when a number is zero:

`input · testing · failing · merge`

Example: `1·0·2·3` means 1 needs input, 0 need testing, 2 have failing CI, 3 are ready to merge. When all four are zero the title is empty and the tooltip is "Nothing waiting on you". The tooltip always names the four slots.

`agent-working` and `not-started` stay out of the menu-bar title. They are context, and the Dependabot pile would crowd out the numbers that matter. Both are visible in the popover.

Left-click toggles the popover. Right-click opens a small native menu: Refresh, Open config, Quit.

### Popover

A borderless, always-on-top window anchored to the tray icon, about 400 by 560 points. Clicking outside closes it. Sections follow the precedence order below. A section with no rows is omitted.

Each section header is the group name and the count (`Needs testing · 2`).

Header of the window: time of the last successful fetch, a refresh control, and a one-line error banner when the last poll failed. The banner sits above the last good snapshot. The app keeps showing that snapshot until a poll succeeds.

Footer: a collapsed bot summary when the filter says so, then Settings (reveals the config file) and Quit.

### Groups and precedence

Each issue and pull request is placed in exactly one group. First match wins:

| Order | Group | Rule |
| --- | --- | --- |
| 1 | Needs input | Has the configured `needs-input` label |
| 2 | CI failing | Pull request whose `statusCheckRollup.state` is `FAILURE` or `ERROR` |
| 3 | Needs testing | Has the configured `needs-testing` label |
| 4 | Ready to merge | Has the configured `ready-to-merge` label |
| 5 | Agent working | Has the configured `agent-working` label |
| 6 | Not started | Open issue or pull request with none of the four labels, and CI is not failing |

Why this order: needs-input is the only state where Bryan is the blocker, including when an agent is also working or CI is red. A failing rollup outranks needs-testing and ready-to-merge because the head commit is not in the state those labels promise. Needs-testing outranks ready-to-merge because testing is still open. Agent-working is informational. Not-started is the remainder.

Other labels and the CI state that did not win still show up as chips on the row, so a ready-to-merge PR that landed in CI failing still shows the ready-to-merge chip.

Rollup handling:

| `statusCheckRollup.state` | Result |
| --- | --- |
| `FAILURE`, `ERROR` | Eligible for CI failing (unless needs-input won) |
| `PENDING` | Stays in its label group. Row chip: "CI running" |
| `SUCCESS` | Stays in its label group. Skipped checks (`release`, `tauri` on ham-radio-ui) do not count as failures |
| `null` (no checks) | Stays in its label group. Same treatment as `ham-radio-docs` #13 and `.github` #4 |

Issues have no rollup, so they never land in CI failing. Within a group, rows sort by `updatedAt` descending.

The CI failing row lists the failed check names from rollup contexts whose conclusion is `FAILURE`, `CANCELLED`, or `TIMED_OUT`. `SKIPPED`, `NEUTRAL`, and `SUCCESS` are omitted. Cap the chip list at three names plus "and N more".

### Bot filter

Config key `filters.bots`, one of `hide`, `collapse`, `show`. Default `collapse`.

Default logins are `dependabot[bot]` and `renovate[bot]`, the names shown on GitHub. GraphQL `author.login` for that account is `dependabot` with `__typename` `Bot` (verified on an open Dependabot PR). REST `user.login` is `dependabot[bot]`. The matcher compares the configured login both as written and with a trailing `[bot]` removed, and it ignores the word "dependabot" inside a title.

| Mode | Behavior |
| --- | --- |
| `hide` | Bot items leave every group and every tray count |
| `collapse` | Bot items leave the six groups. One footer row summarizes them |
| `show` | Bot items use the same precedence as human items |

`filters.surface_failing_bots` defaults to true. A bot pull request in CI failing still appears in that section under `hide` and `collapse`, and it counts in the tray's failing number. Green and check-less bot PRs stay collapsed. With today's data the footer row would read "Dependabot · 29 green · 2 no CI" and would open this search in the browser:

`https://github.com/search?q=org%3Aspringfield-ham-radio+is%3Apr+is%3Aopen+author%3Aapp%2Fdependabot`

Drafts stay in their group and wear a Draft chip. Today's two human drafts would sit in Not started.

### What a row shows

- Repo short name (`ham-radio-ui`) and `#84`
- Title, single line, truncated
- Kind chip: Issue, PR, or Draft
- Chips for the status labels that did not decide the group, and the CI chip when it adds information
- Author login
- Relative update time
- The whole row is one link

Activate a row and the app opens `html_url` with the Tauri opener plugin (the system browser). The URL is the GitHub issue or pull request page. The app does not read the local checkout to render a row. When Bryan wants to confirm a row against disk, the matching tree is `~/Development/springfield-ham-radio/<repo>`.

## 2. Polling

### Query

One GraphQL search per configured org, every `poll_interval_seconds` (default 30), to `https://api.github.com/graphql`:

```graphql
query Dashboard($searchQuery: String!, $after: String) {
  rateLimit { cost remaining resetAt }
  search(query: $searchQuery, type: ISSUE, first: 100, after: $after) {
    issueCount
    pageInfo { hasNextPage endCursor }
    nodes {
      __typename
      ... on Issue {
        number title url updatedAt
        author { login }
        labels(first: 20) { nodes { name } }
        repository { name nameWithOwner }
      }
      ... on PullRequest {
        number title url updatedAt isDraft
        author { login }
        labels(first: 20) { nodes { name } }
        repository { name nameWithOwner }
        commits(last: 1) {
          nodes {
            commit {
              oid
              statusCheckRollup {
                state
                contexts(first: 20) {
                  nodes {
                    ... on CheckRun { name conclusion status }
                    ... on StatusContext { context state }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
```

Search string: `org:<name> is:open archived:false`. That returns issues and pull requests. Include and exclude lists are applied on the client after the response, so the query string stays stable when a repo is added and stays well under the 256-character search limit. A space in GitHub search is AND; listing twelve `repo:` qualifiers with `OR` is the fragile alternative.

`first: 100` covers today's 33 items in one page. When `pageInfo.hasNextPage` is true, the same poll follows `endCursor` until exhausted, then groups the combined set. There is no second timer tick in between.

The legacy `status` field and the REST combined-status endpoint stay out of the query. They are empty or `pending` with zero statuses on this org, so a `status:failure` search would miss real failures.

### GraphQL has no ETags

Conditional requests (`If-None-Match` / 304) are a REST feature. The GraphQL endpoint ignores them, so every GraphQL poll spends points even when nothing changed.

The REST probe that does support ETags, per repo:

`GET /repos/{owner}/{repo}/issues?state=open&sort=updated&direction=desc&per_page=1`

with `If-None-Match` set to the stored ETag and `Accept: application/vnd.github+json`. A 304 does not spend the primary rate limit. It still counts toward the secondary limit.

That probe sees label, title, and open/closed changes. It does not see CI. Check-run completion does not change the issues list, which is why a 304 can arrive while `statusCheckRollup` flips from `PENDING` to `FAILURE`.

So the probe is implemented and defaulted off (`use_etag_probe = false`). With the default, every tick runs GraphQL and CI stays within the 30-second interval.

When the probe is turned on, a tick skips GraphQL only if every repo returned 304 and the last GraphQL snapshot is younger than `ci_max_staleness_seconds`. The default staleness is 30 seconds, which means the probe will not skip anything until that value is raised. Raising it to 300 trades up to five minutes of CI lag for a much smaller GraphQL bill. The popover tooltip should say when CI may be that stale.

Repo names for the probe come from the config include list. When include is empty, `GET /orgs/{org}/repos?per_page=100` runs at most once an hour, itself with an ETag, and the name list is cached.

### Rate-limit budget

Measured on 2026-10-10: the search above, first 50 nodes, cost 2 GraphQL points. Primary GraphQL limit on a user token is 5,000 points per hour. REST core is also 5,000 per hour. REST search is a separate budget of 30 requests per minute; this design does not use it. Authenticated requests are required. Unauthenticated GraphQL is 60 points per hour, which is less than one 30-second poll sustained.

Assume 120 ticks per hour, one org, one page.

| Mode | Primary spend per hour | Share of 5,000 |
| --- | --- | --- |
| GraphQL at the measured cost of 2 | 240 points | 4.8% |
| GraphQL with a 10-point headroom budget (more fields, or a second page) | 1,200 points | 24% |
| Two orgs at the 10-point budget | 2,400 points | 48% |
| ETag probe, 12 repos, all 304 | 0 primary points, 1,440 REST calls | secondary limit only |
| ETag probe when a repo returns 200 | 1 core point per changed repo | negligible |

24 REST calls per minute (12 repos every 30 seconds) sits under the rough secondary ceiling of about 90 requests per minute, and the poller never runs those calls concurrently with a second poll. One GraphQL request in flight at a time.

The same token still has several thousand points left for `gh` on the command line at the measured cost. The app reads `rateLimit` on every GraphQL response and `X-RateLimit-Remaining` / `X-RateLimit-Reset` on every REST response.

Automatic brakes:

- If one poll reports `cost > 50`, log a warning.
- If `cost × (3600 / poll_interval_seconds) > 2500`, lengthen the live interval so the projected hour stays at or under 2,500, and show "Slowed to Ns to stay inside the GitHub rate limit" in the popover.
- If `remaining < 500`, poll every 5 minutes.
- If `remaining < 100`, pause until `resetAt` and show the clock time.

Minimum configured interval is 15 seconds. Default is 30. A tick that starts while a poll is in flight is skipped. The next tick is scheduled from the completion of the previous poll, so a slow response does not pile up.

### Backoff

| Failure | Wait |
| --- | --- |
| Primary limit exhausted (403 or 429 with remaining 0) | Until `resetAt`, plus 1–5 seconds of jitter |
| Secondary limit, or any 403/429 with `Retry-After` | `Retry-After` seconds. If the header is missing, exponential backoff |
| HTTP 5xx, DNS, timeout, connection reset | 30s, 60s, 120s, 240s, then cap at 5 minutes, with full jitter |
| GraphQL `errors` and usable `data.search` | Accept the data, show a one-line warning |
| GraphQL `errors` and null `data.search` | Treat as a failed poll |

One successful poll restores the normal interval. Failed polls keep the last snapshot on screen.

### Sleep and offline

The timer is a Rust interval, not a `setInterval` in the webview. The process does not take a power assertion and does not stop the Mac from sleeping.

Sleep drops timer ticks. On wake (`NSWorkspaceDidWakeNotification`, macOS-only, isolated in `src-tauri/src/power.rs`) the app polls once immediately and then resumes the normal interval. It does not fire one poll per missed interval.

A connect or DNS failure sets an offline banner: "Offline · last updated 4m ago". The next success clears it and refreshes immediately. Opening the popover also triggers a poll if the snapshot is older than the interval.

## 3. Config

TOML, because comments survive and the Rust `toml` crate can load it. The file is not committed and is not created inside the git checkout.

| | Path |
| --- | --- |
| Runtime config | `~/Library/Application Support/com.springfield.agent-dashboard/config.toml` |
| Last snapshot, for the morning summary | `~/Library/Application Support/com.springfield.agent-dashboard/snapshot.json` |
| Logs | `~/Library/Logs/com.springfield.agent-dashboard/` |
| Example checked into the repo | `config.example.toml` |

Tauri's app config dir is the Application Support path above. First launch writes `config.toml` from the baked-in defaults when the file is absent. A syntax error keeps the previous in-memory config and shows the parse error in the popover. Unknown keys are ignored and logged.

```toml
# ~/Library/Application Support/com.springfield.agent-dashboard/config.toml

poll_interval_seconds = 30

# GraphQL has no ETags. The REST probe below can skip a GraphQL call only
# while every repo returns 304 and the last check-run snapshot is younger
# than ci_max_staleness_seconds. Leave the probe off to keep CI within
# poll_interval_seconds. Check runs do not change the issues-list ETag.
use_etag_probe = false
ci_max_staleness_seconds = 30

[github]
# "keychain" reads the fine-grained PAT. "gh" shells out to `gh auth token`.
credential = "keychain"

[filters]
# hide | collapse | show
bots = "collapse"
bot_logins = ["dependabot[bot]", "renovate[bot]"]
surface_failing_bots = true

[tray]
# Menu-bar title slots, left to right.
counts = ["needs-input", "needs-testing", "ci-failing", "ready-to-merge"]

[labels]
agent_working = "agent-working"
needs_testing = "needs-testing"
needs_input = "needs-input"
ready_to_merge = "ready-to-merge"

[[orgs]]
name = "springfield-ham-radio"
# Empty include means every repo in the org. exclude is applied after include.
include = []
exclude = []

# [[orgs]]
# name = "springfield-tools"
# include = ["agent-dashboard"]
```

Label keys are the only names the grouper understands. Renaming a label on GitHub means editing this file. Group order and precedence stay in code so the tray slots do not reshuffle between machines.

Nothing in v1 is hardcoded to `springfield-ham-radio` except the default file written on first launch.

## 4. GitHub auth

The token lives in the macOS Keychain (service `com.springfield.agent-dashboard`, account `github-token`) via the `keyring` crate. It is never written to `config.toml`, `snapshot.json`, logs, or the webview. Rust performs every GitHub call. The frontend receives the grouped snapshot only.

A dev rebuild often changes the binary's code signature, and macOS will prompt once to allow the new binary to read the keychain item. That prompt is expected.

### Comparison

| | `gh auth token` | Fine-grained PAT | Device-flow OAuth app |
| --- | --- | --- | --- |
| Privilege | Whatever `gh` already has. Often `repo` plus the ability to push and admin | Read-only Issues, Pull requests, and Checks on the chosen org | Classic scopes only. Private repos mean the `repo` scope, which can push |
| Setup | Lowest if `gh` is already logged in. The app depends on the `gh` binary and on calling it every poll so a refreshed token is picked up | Create a token, paste it once, approve it for the org if the org requires that | Register an OAuth app, implement device flow and refresh, get the app approved for the org |
| Org approval | Already done for `gh` | Required when the org gates fine-grained tokens. One owner approval | Required when the org gates OAuth apps |
| Expiry | `gh`'s own refresh, or never, if a PAT was pasted into `gh` | Required, at most one year. A 401 means the popover asks for a new token | Access token about 8 hours. A refresh token must also live in the Keychain |
| Secret in the app binary | None | None | The OAuth client secret has to ship inside a public desktop client |
| Fit for this app | Convenient and broader than the app needs | Matches a single-user read-only tray app | Fits a later build that other people install |

### Recommendation

Use a fine-grained PAT in the Keychain. The tray app should not hold a credential that can push. Device flow cannot express "issues read, pull requests read, checks read", and it would embed a client secret. `gh auth token` stays available as `credential = "gh"` for a day when Bryan would rather not mint a second secret.

Token settings to document in the README:

- Resource owner: `springfield-ham-radio` (add `springfield-tools` when that org is in the config)
- Repository access: all repositories, so a new repo appears without a new token
- Repository permissions: Metadata Read (automatic), Issues Read, Pull requests Read, Checks Read
- Contents stays off. If a future private repo returns an empty rollup, add Contents Read then
- Expiration: 90 days is a reasonable first choice

All 12 ham-radio repos are public today, so this permission set is enough for the measured query. The popover treats HTTP 401 as "token rejected" and links to the GitHub token settings page. It does not retry a 401 on the backoff schedule.

## 5. Stack and conventions

Follow `springfield-ham-radio/ham-radio-ui`, including the shared Cursor rules that repo links to from `springfield-ham-radio/ham-radio-ops/.cursor/rules/`. Where this plan diverges, it says so.

### Adopt from ham-radio-ui

- Node `26.10.0` via `.nvmrc`. Yarn `4.18.0` via the `packageManager` field and Corepack. `.yarnrc.yml` uses `nodeLinker: node-modules` and `enableScripts: true`. CI installs with `yarn install --immutable`.
- `"type": "module"`, MIT license, author Bryan Hunt.
- Conventional Commits, from the shared `conventional-commits.mdc` rule: `type(scope): description`, types `feat`, `fix`, `test`, `refactor`, `docs`, `ci`, `revert`, `style`, `chore`. A breaking API change gets a `BREAKING CHANGE:` footer. The rule's sentence that limits `ci` to `.gitlab-ci.yml` is stale; ham-radio-ui already uses `ci:` for GitHub Actions, and this repo will too.
- semantic-release, from `package.json` `"release"` and from `semantic-release.mdc`: versions and `CHANGELOG.md` are written only by semantic-release. Preset `conventionalcommits`. Plugins, in order: commit-analyzer, release-notes-generator, `@semantic-release/changelog`, `@semantic-release/npm` with `npmPublish: false`, `@semantic-release/exec` to write the version into `src-tauri/Cargo.toml` (the ham-radio-ui script is `scripts/set-cargo-version.mjs`), `@semantic-release/github`, `@semantic-release/git` committing `CHANGELOG.md`, `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/Cargo.lock` with message `chore(release): ${nextRelease.version} [skip ci]`. Branches: `main`, plus `beta` as a prerelease.
- CI shape from `.github/workflows/ci.yml`: workflow name `CI`, concurrency group that cancels in progress, `actions/checkout@v7`, `actions/setup-node@v7` with `node-version-file: .nvmrc`, Corepack, then `yarn install --immutable`. The required job is named `build`.
- Release auth from that same workflow: the `release` job depends on `build`, runs only on a push to `main` (and `beta` if that branch exists), and mints a token with `actions/create-github-app-token@v2` from `vars.RELEASE_APP_ID` and `secrets.RELEASE_APP_PRIVATE_KEY`, because the default `GITHUB_TOKEN` cannot push the release commit through branch protection. The Tauri publish job calls `tauri-apps/tauri-action@v1` the way `.github/workflows/release-tauri.yml` does, on `macos-latest` only.
- oxlint `1.85.0`, scripts `lint` and `lint:fix`, `.oxlintrc.json` with the `import` plugin. One divergence: ham-radio-ui ignores `test/`, and this repo will lint tests.
- Vitest 5, config modeled on `vitest.config.ts`, tests in `test/unit`, file name matching the source file. `yarn test` runs `yarn test:unit` and accepts no extra arguments (shared `typescript-testing.mdc`). Factories use `fishery`. Test-driven development from the shared `testing.mdc`: a failing test, the minimum code that passes, then refactor, before the next behavior.
- TypeScript style from `typescript-coding.mdc`: semicolons, blank lines around `if` / `for` / `while` / `try`, no unusual abbreviations, `_` prefix on unused parameters, `readonly` where a field is never reassigned.
- Logging from `typescript-logging.mdc`: `loglayer` typed as `ILogLayer`, `withError` for errors, `withMetadata` for structured fields, `MockLogLayer` in tests. Rust side uses the `log` crate and `tauri-plugin-log`, matching `src-tauri/Cargo.toml`.
- Vue single-file components put the template first, then the script (`vue.mdc`).
- Tauri crate layout from `src-tauri/Cargo.toml` and `tauri.conf.json`: edition 2021, `rust-version = "1.85.0"`, a lib plus a bin, `tauri` 2.9 and `@tauri-apps/cli` 2.11, `tauri.conf.json` `"version": "../package.json"`, identifier style `com.springfield.ham-radio` so this app uses `com.springfield.agent-dashboard`, bundle copyright "Copyright (c) 2026 Bryan Hunt", MIT.
- Dependabot from `.github/dependabot.yml`: weekly on Monday, grouped minor and patch updates, commit prefixes `fix` (production), `chore` (development), `ci` (actions). Ecosystems: npm, github-actions, and cargo under `/src-tauri`.
- `.gitignore` patterns for Node, Yarn, `.env`, `dist`, and `src-tauri/target`.

The shared general rule says to honor a root `.editorconfig`. ham-radio-ui does not have one. Phase 1 adds a small one: UTF-8, LF, final newline, two-space indent for TypeScript, JSON, YAML, and TOML.

`@springfield/*` dependencies and the shared `dependencies.mdc` rule do not apply. This app does not depend on those packages.

### Husky

ham-radio-ui has no `.husky/` directory and no `husky` dependency. Its `prepare` script is `yarn build`, which is a Nuxt behavior. This repo still adds Husky, because commit messages feed semantic-release.

- `prepare` runs `husky`
- `commit-msg` runs commitlint with `@commitlint/config-conventional`
- The pre-commit hook does not run the full test suite. CI does

### Frontend

Vue 3 + Vite + TypeScript. ham-radio-ui is Nuxt 4 because it is a multi-page desktop app with `yarn dev` / `yarn generate` writing `.output/public`. This UI is a single popover with no router and no SSR. Vite is the Tauri default (`devUrl` on localhost, `frontendDist` `../dist`) and it keeps the popover's dev loop short. Vue stays, so the template-then-script rule, oxlint, and vitest match the ham radio app. `@nuxt/ui` and Tailwind stay out; one plain stylesheet is enough for a list of rows.

Frontend source lives in `src/`. That diverges from ham-radio-ui's Nuxt `app/` directory on purpose.

### Rust layout

```
src-tauri/
  Cargo.toml
  tauri.conf.json
  build.rs
  capabilities/
  src/
    main.rs          # bin, calls the lib
    lib.rs           # builder, accessory policy, plugins
    tray.rs
    poll.rs          # interval, single-flight, backoff, wake
    power.rs         # NSWorkspace wake, macOS only
    config.rs
    keychain.rs
    snapshot.rs      # write snapshot.json
    model.rs         # precedence, bot filter, tray title
    github/
      graphql.rs
      probe.rs       # REST ETag probe
      rate_limit.rs
```

`model.rs` is the source of truth for grouping. The webview renders the grouped snapshot it is given. Unit tests in `src-tauri` cover the precedence table in section 1, the bot filter, tray-title formatting, config parsing, and backoff delays with a fake clock. GraphQL fixtures are checked in. Tests do not call GitHub.

TypeScript unit tests cover relative time and any formatting that stays in the webview.

### Sharing the data with the morning summary

Each successful poll writes:

`~/Library/Application Support/com.springfield.agent-dashboard/snapshot.json`

mode `0600`, no token, no headers. Shape:

```json
{
  "schemaVersion": 1,
  "fetchedAt": "2026-10-10T19:45:00Z",
  "stale": false,
  "groups": [
    {
      "id": "needs-input",
      "title": "Needs input",
      "items": [
        {
          "kind": "pr",
          "draft": false,
          "org": "springfield-ham-radio",
          "repo": "ham-radio-ui",
          "number": 84,
          "title": "feat(location): shared location-format module over GeoPoint",
          "url": "https://github.com/springfield-ham-radio/ham-radio-ui/pull/84",
          "author": "BryanHunt",
          "updatedAt": "2026-10-10T19:34:25Z",
          "labels": [],
          "ci": { "state": "SUCCESS", "failedChecks": [] }
        }
      ]
    }
  ],
  "bots": { "mode": "collapse", "green": 29, "noCi": 2, "failing": 0 }
}
```

The morning job can read that file, or the same config TOML, without linking this crate. A CLI that prints the same JSON is out of scope for v1 and listed in section 6.

### Signing, notarization, and distribution

Defer Apple Developer ID signing and notarization. ham-radio-ui already ships unsigned installers and documents the workaround in its README: macOS right-click, Open. This app does the same until it needs to be installed on a machine that is not Bryan's. Notarization needs an Apple Developer membership, a Developer ID certificate, and a hardened runtime. Those secrets are not required for any v1 phase.

Updater signing (Tauri minisign, which ham-radio-ui already uses via `TAURI_SIGNING_PRIVATE_KEY` and `createUpdaterArtifacts`) also waits. v1 has no update feed.

Distribution for v1 is a local dev session and, in the release phase, an unsigned `.app` or `.dmg` attached to a GitHub Release by `tauri-action`. The README will say the build is unsigned.

### CI job `build`

`build` runs on `macos-latest` and is the job to mark required on `main`. It is the only required job.

Steps: checkout, setup Node from `.nvmrc`, Corepack, `yarn install --immutable`, `yarn lint`, `yarn test`, install stable Rust (`dtolnay/rust-toolchain`), `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo build --manifest-path src-tauri/Cargo.toml`.

Bundling a `.dmg` stays in the release workflow so the required check remains a compile-and-test. Node 26 images sometimes lack Corepack; ham-radio-ui's release workflow already works around that, and the macOS job should use the same `corepack enable` step.

## 6. Later ideas

Out of scope for v1. Do not build these in the phases below.

- **Notifications.** When a snapshot transition moves an item into needs-input, or into CI failing, post a macOS notification. Clicking it opens the popover. Needs a baseline snapshot so the first poll after install does not notify for everything already open.
- **Cursor cloud agent status.** v1 trusts the four labels. A later version can map a Cursor cloud-agent API, if one is available to a desktop app, onto the same groups: running to agent-working, waiting on the user to needs-input. Scraping the Cursor website is not a plan.
- **Applying labels from the app.** That needs a write token and can move an item into the wrong group. Keep the PAT read-only. Any future write action is explicit and separate from the poller.
- **CLI dump** of `snapshot.json` for the morning summary, for hosts where the tray app is not running.
- **Device-flow OAuth** and **Apple notarization**, when someone besides Bryan installs the app.
- **Snooze, author filter, keyboard shortcut, menu-bar color dots.** Product polish after the live poll is trustworthy.

## 7. Phased build

Each phase merges on its own. Later phases assume the earlier ones are on `main`. Commands assume:

```bash
cd ~/Development/springfield-tools/agent-dashboard
```

Ham radio checkouts used for eyeballing a row against disk:

```bash
cd ~/Development/springfield-ham-radio/ham-radio-ui
```

### Phase 1 — Tray shell

**Scope:** Yarn 4, Husky, commitlint, oxlint, vitest, `.nvmrc`, `.editorconfig`, `.gitignore`, the macOS `build` workflow, and a Tauri 2 accessory app. The popover lists the six section titles with one placeholder row each. No GitHub client, no keychain, no config file yet. README describes `yarn tauri:dev` and points at this plan.

**Acceptance:**

- `yarn lint`, `yarn test`, and `cargo test --manifest-path src-tauri/Cargo.toml` pass locally and in the `build` job on `macos-latest`.
- The app shows a menu-bar icon and no Dock icon.
- Left-click opens the popover. Right-click Quit exits.
- A placeholder commit message that violates Conventional Commits is rejected by the hook.

**Bryan tests:**

```bash
cd ~/Development/springfield-tools/agent-dashboard
corepack enable
yarn install
yarn tauri:dev
```

Confirm the icon, the popover, and Quit. Push a draft PR and confirm the check named `build` is the one GitHub reports.

### Phase 2 — Config, grouping, fixture

**Scope:** `config.example.toml`, loader, and `model.rs` with the precedence and bot-filter tests from section 1. The popover and tray title render a checked-in fixture shaped like the GraphQL snapshot (today's 33 PRs is a fine fixture). Changing `labels` or `filters.bots` in the Application Support config and restarting re-groups the fixture. Still no network.

**Acceptance:**

- The fixture's 29 green Dependabot PRs collapse to one footer row, and the two human drafts sit in Not started.
- A fixture PR labeled `needs-testing` with rollup `FAILURE` appears only under CI failing and keeps a needs-testing chip.
- A fixture item labeled `needs-input` and `agent-working` appears only under Needs input.
- Tray title for an all-quiet fixture is empty.

**Bryan tests:** from `~/Development/springfield-tools/agent-dashboard`, run `yarn tauri:dev`. Edit `~/Library/Application Support/com.springfield.agent-dashboard/config.toml`, set `filters.bots` to `show`, restart, and see the Dependabot rows appear under Not started. Set it back to `collapse`.

### Phase 3 — Live GitHub

**Scope:** Keychain write/read, settings affordance to paste the PAT once, GraphQL poll on the interval, grouping of the live payload, `snapshot.json` rewritten on every success. Manual Refresh. Default org `springfield-ham-radio`. The fixture remains the offline demo behind a `--fixture` flag so Phase 2's checks still run without a token.

**Acceptance:**

- With a valid token, the popover matches GitHub within one poll: the two human drafts, the collapsed Dependabot row (29 green, 2 with no CI), and zero rows in the four label groups.
- `snapshot.json` `schemaVersion` is 1 and contains no token.
- An invalid token shows the rejection banner and does not spin on backoff.
- Opening a row opens that pull request in the browser.

**Bryan tests:**

```bash
cd ~/Development/springfield-tools/agent-dashboard
yarn tauri:dev
```

Paste the fine-grained PAT when asked. Compare the popover with `https://github.com/pulls?q=org%3Aspringfield-ham-radio+is%3Aopen`. Open the `ham-radio-ui` draft row and confirm the local branch in `~/Development/springfield-ham-radio/ham-radio-ui` is the one you expect. Add a temporary `needs-input` label to a throwaway issue or PR, wait one interval, and see it take the top section. Remove the label afterward.

### Phase 4 — Poll hardening

**Scope:** Backoff, rate-limit brakes, wake refresh, offline banner, and the ETag probe defaulted off. Tests use a fake clock and a fake HTTP layer.

**Acceptance:**

- Airplane mode (or a denied network) leaves the last snapshot up, with the offline banner, and recovers on the next success without a burst of parallel polls.
- Closing the lid for a minute and reopening refreshes once, then returns to 30 seconds.
- Setting `use_etag_probe = true` and `ci_max_staleness_seconds = 300` skips GraphQL while the issues lists 304, and the popover says CI may be up to five minutes old.

**Bryan tests:** from `~/Development/springfield-tools/agent-dashboard`, run `yarn tauri:dev`, turn Wi-Fi off, confirm the banner and the still-visible rows, turn Wi-Fi on, confirm a refresh. Sleep the Mac briefly and confirm a refresh on wake.

### Phase 5 — Release

**Scope:** semantic-release config and the Cargo version script, the `release` job, and a macOS-only `tauri-action` publish of an unsigned artifact. README install note matches ham-radio-ui's unsigned-build wording. Dependabot config for npm, actions, and cargo.

**Acceptance:**

- A `feat` commit on `main` produces a GitHub Release with a macOS artifact and a changelog entry.
- A `docs` or `chore` commit does not.
- The app version in `package.json` and `src-tauri/Cargo.toml` matches the tag, and both were written by semantic-release.

**Bryan tests:** after the release workflow has the GitHub App secrets the ham-radio-ui repo already uses, merge a small `feat` and install the unsigned `.app` or `.dmg` from the Release. First launch still reads the Keychain item; allow the prompt if macOS asks.

## 8. Open questions

1. Tray slots. Is `input · testing · failing · merge` the right menu-bar title, or should `agent-working` replace one of those?
2. Precedence. Should CI failing outrank needs-testing and ready-to-merge, as specified, or should a labeled item stay in its label group with a red chip?
3. Drafts. Should `ham-radio-ui` #84 and `.github` #4 appear in Not started, or should drafts be hidden until they are marked ready?
4. Orgs. Should the default config include `springfield-tools` as well as `springfield-ham-radio`?
5. Auth. Is the fine-grained PAT acceptable, or do you want `credential = "gh"` as the default despite the broader scopes?
6. Token lifetime. 90 days, or the one-year maximum?
7. Private repos. All 12 ham-radio repos are public. If a private repo should show up, the token needs access to it. Any planned?
8. Bots. Is collapse-plus-surface-failures the right default, or should Dependabot be hidden entirely?
9. The "about 11 open issues" figure. On 2026-10-10 the REST issues API returned 0 open issues across all 12 repos, and 33 open PRs. Were those 11 issues since closed, or do they live on a project board or another org this plan should include?
10. Required check. Is `yarn lint`, `yarn test`, `cargo test`, and `cargo build` enough for the required `build` job, or must that job also produce a `.app`?
11. Release secrets. Can this repo reuse the ham-radio-ui `RELEASE_APP_ID` / `RELEASE_APP_PRIVATE_KEY` GitHub App, or does it need its own?
12. Morning summary. Is reading `snapshot.json` from Application Support a workable handoff, and does that job run on the same Mac?
13. Label names. `.github` pull request 4 ("Sync org issue labels from labels.yml") is still a draft. Are `agent-working`, `needs-testing`, `needs-input`, and `ready-to-merge` stable?
14. Battery. Should the interval stay 30 seconds on battery, or slow down when the Mac is unplugged?

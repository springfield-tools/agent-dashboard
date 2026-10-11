# Agent Dashboard — implementation plan

Menu-bar app for Bryan Hunt that shows in-flight GitHub work across his orgs. It polls on a configurable interval that defaults to 60 seconds, and he can pause polling from the tray. This document is the plan only. A follow-up agent implements it after review.

The daily morning summary stays outside this repo. This app writes a versioned snapshot file the summary can read, so the two do not each invent a GitHub client.

Local checkouts, used in every setup and test step below:

- This repo: `~/Development/springfield-tools/agent-dashboard`
- Ham radio repos: `~/Development/springfield-ham-radio/<repo>`

**Architecture decisions (accepted):** [docs/adr/](adr/README.md) — stack, auth, polling, and status precedence.

## Recommendation

Build a Tauri 2 macOS tray app. Rust owns polling, the keychain, and grouping. Vue 3 renders one popover. The first phases ship a fine-grained read-only personal access token in the macOS Keychain. A later phase adds issue and pull-request write so a row can change labels and post a comment. Poll with one GitHub GraphQL **organization.repositories** fetch per org (fine-grained PATs cannot rely on org search). The interval is a setting in the config file and in the popover, default 60 seconds, allowed range 15 seconds to 15 minutes. Read CI from `statusCheckRollup`. Pause and resume from the tray menu or the popover, including "for 1 hour" and "until tomorrow". The icon shows a paused state while polling is stopped. Keep the bot filter configurable, defaulting to a collapsed Dependabot row that still surfaces failing bot PRs.

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

Left-click toggles the popover. Right-click opens a small native menu: Refresh, Pause polling (or Resume polling), Pause for 1 hour, Pause until tomorrow, Open config, Quit.

While polling is paused the template image switches to a pause mark and the count title is replaced with the word `Paused`. The tooltip carries the resume condition: "Polling paused", "Polling paused until 18:00", or "Polling paused until tomorrow (00:00)". A monochrome title is the signal, so the state stays obvious on a light or dark menu bar. Counts return when polling resumes. A rate-limit slowdown keeps the normal icon and uses the popover banner described below. It is a different state from a user pause.

### Popover

A borderless, always-on-top window anchored to the tray icon, about 400 by 560 points. Clicking outside closes it. Sections follow the precedence order below. A section with no rows is omitted.

Each section header is the group name and the count (`Needs testing · 2`).

Header of the window: time of the last successful fetch, the active interval (for example `every 1m`), a refresh control, a pause control, and a one-line banner when the last poll failed or polling is paused. The banner sits above the last good snapshot. The app keeps showing that snapshot until a poll succeeds. Refresh while paused fetches once and leaves the pause in place.

The pause control is one click to pause and one click to resume. Its menu has the same extra choices as the tray menu: Pause for 1 hour, and Pause until tomorrow.

Footer: the interval control, a collapsed bot summary when the filter says so, then Settings (reveals the config file) and Quit.

The interval control is four presets, 15s, 30s, 1m, and 2m, plus the current value when the config holds some other allowed number. Choosing a preset writes `poll_interval_seconds` and reschedules the timer. The allowed range is 15 to 900 seconds. A value outside that range is rejected, the previous interval keeps running, and the popover says so. Anything between the presets is edited in the config file.

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

One GraphQL **organization.repositories** query per configured org (paginated), every `poll_interval_seconds` (default 60, minimum 15, maximum 900), to `https://api.github.com/graphql`. Fine-grained PATs often cannot use `search(query: "org:…")` (HTTP 200 with errors and empty nodes); the app lists org repos and loads open pull requests and issues per repo instead. The popover interval control writes that same key.

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

Live fetch uses `organization(login: <name>).repositories` with open `pullRequests` and `issues` per repo (not GraphQL search). Include and exclude lists are applied on the client per repository name; archived repos are skipped. Reference search string (OAuth/`gh` only): `org:<name> is:open archived:false`.

`first: 100` covers today's 33 items in one page. When `pageInfo.hasNextPage` is true, the same poll follows `endCursor` until exhausted, then groups the combined set. There is no second timer tick in between.

The legacy `status` field and the REST combined-status endpoint stay out of the query. They are empty or `pending` with zero statuses on this org, so a `status:failure` search would miss real failures.

### GraphQL has no ETags

Conditional requests (`If-None-Match` / 304) are a REST feature. The GraphQL endpoint ignores them, so every GraphQL poll spends points even when nothing changed.

The REST probe that does support ETags, per repo:

`GET /repos/{owner}/{repo}/issues?state=open&sort=updated&direction=desc&per_page=1`

with `If-None-Match` set to the stored ETag and `Accept: application/vnd.github+json`. A 304 does not spend the primary rate limit. It still counts toward the secondary limit.

That probe sees label, title, and open/closed changes. It does not see CI. Check-run completion does not change the issues list, which is why a 304 can arrive while `statusCheckRollup` flips from `PENDING` to `FAILURE`.

So the probe is implemented and defaulted off (`use_etag_probe = false`). With the default, every tick runs GraphQL and CI stays within the configured interval, 60 seconds unless Bryan changes it.

When the probe is turned on, a tick skips GraphQL only if every repo returned 304 and the last GraphQL snapshot is younger than `ci_max_staleness_seconds`. The default staleness is 60 seconds, matching the default interval, which means the probe will not skip anything until that value is raised above the interval. Lengthening the poll interval on its own does not enable skips. Raising staleness to 300 trades up to five minutes of CI lag for a much smaller GraphQL bill. The popover tooltip should say when CI may be that stale.

Repo names for the probe come from the config include list. When include is empty, `GET /orgs/{org}/repos?per_page=100` runs at most once an hour, itself with an ETag, and the name list is cached.

### Rate-limit budget

Measured on 2026-10-11 (live `rateLimit.cost` via `scripts/measure-graphql-cost.sh`): one cheap **`organization.repositories`** page for `springfield-ham-radio` (12 repos, 33 open PRs/issues, no extra repo/PR/issue pages, no failure-context follow-ups) cost **4** GraphQL points per tick. Each failing PR adds about **1** point when `contexts(first: 10)` is fetched. Repo/PR/issue pagination adds another org or repo page at roughly **4** points each when limits are exceeded. Primary GraphQL limit on a user token is 5,000 points per hour (shared with `gh` and other tools). REST core is also 5,000 per hour.

Points per hour for the measured steady-state tick:

`cost_per_tick × (3600 / poll_interval_seconds)`

The headroom column uses **8** points per tick (extra failure-context queries, pagination, or heavier rollups).

| Interval | Ticks per hour | 4 pts/tick (measured) | Share of 5,000 | 8 pts/tick (headroom) | Share of 5,000 |
| --- | --- | --- | --- | --- | --- |
| 15s | 240 | 960 | 19.2% | 1,920 | 38.4% |
| 30s | 120 | 480 | 9.6% | 960 | 19.2% |
| 60s (default) | 60 | 240 | 4.8% | 480 | 9.6% |
| 120s | 30 | 120 | 2.4% | 240 | 4.8% |

How many orgs fit under 5,000 points at one measured page each:

| Interval | Orgs at 4 pts/tick | Orgs at 8 pts/tick (headroom) |
| --- | --- | --- |
| 15s | 5 (4,800). A 6th org crosses the limit | 2 (3,840). A 3rd org crosses the limit |
| 30s | 10 (4,800). An 11th org crosses the limit | 5 (4,800). A 6th org crosses the limit |
| 60s | 20 (4,800). A 21st org crosses the limit | 10 (4,800). An 11th org crosses the limit |
| 120s | 41 (4,920). A 42nd org crosses the limit | 20 (4,800). A 21st org crosses the limit |

The default, one org at 60 seconds and the measured 4-point cost, is **240** points per hour (4.8%). The automatic brake slows the live interval when `cost × (3600 / poll_interval_seconds) > 2,500`. A future **pushedAt-gated** incremental fetch (cheap repo list every tick, full payload only for repos that moved) could approach a lower steady-state GraphQL bill; that is not implemented in Phase 4. The app reads `rateLimit` on every GraphQL response and shows `pts/poll · remaining` in the popover footer.

The REST ETag probe, when enabled, is one call per repo per tick and spends no primary points on a 304. At 12 repos that is 48 calls per minute at 15 seconds, 24 at 30 seconds, 12 at 60 seconds, and 6 at 120 seconds. All of those sit under the rough secondary ceiling of about 90 requests per minute. The poller never runs a second poll concurrently. One GraphQL request in flight at a time. A 200 from the probe costs 1 core point for that repo.

Automatic brakes, which keep the normal tray icon and explain themselves in the popover banner:

- If one poll reports `cost > 50`, log a warning.
- If `cost × (3600 / poll_interval_seconds) > 2500`, lengthen the live interval so the projected hour stays at or under 2,500, and show "Slowed to Ns to stay inside the GitHub rate limit". The configured `poll_interval_seconds` is left as Bryan set it and is restored when the projection fits again. The brake may poll slower than the 900-second maximum.
- If `remaining < 500`, poll every 5 minutes.
- If `remaining < 100`, wait until `resetAt` and show the clock time.

A tick that starts while a poll is in flight is skipped. The next tick is scheduled from the completion of the previous poll, so a slow response does not pile up. Paused polling spends no points. The brakes above apply only while polling is running.

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

Sleep drops timer ticks. The lid being closed does not switch the icon to the paused mark, because the display is off and the process is simply suspended. On wake (`NSWorkspaceDidWakeNotification`, macOS-only, isolated in `src-tauri/src/power.rs`) the app polls once immediately and then resumes the configured interval, unless a user pause is still in effect. A manual pause or a timed pause survives sleep and survives relaunch. Wake does not fire one poll per missed interval.

A connect or DNS failure sets an offline banner: "Offline · last updated 4m ago". The next success clears it and refreshes immediately. Opening the popover also triggers a poll if the snapshot is older than the interval and polling is running.

### Pause and resume

Three user pauses, all the same mechanism:

| Action | What is stored | When polling resumes |
| --- | --- | --- |
| Pause polling | `poll_pause = "manual"` | Resume is clicked |
| Pause for 1 hour | `poll_pause` set to an RFC3339 timestamp one hour out | That timestamp, then the key is cleared and a poll runs immediately |
| Pause until tomorrow | `poll_pause` set to the next local midnight | That midnight |

The tray menu and the popover pause menu both write this key. Resume clears it from either place. The value lives in the config file so a quit and relaunch keeps the pause. A one-shot timer waits until a timestamped pause expires, so a paused app does not keep waking the network stack on the poll interval.

While paused, the poll loop makes no GitHub calls. The last snapshot stays on screen under the pause banner. Refresh fetches once and leaves the pause in place.

Idle, optional and off by default. `pause_when_idle = true` starts an idle pause after `idle_pause_after_seconds` (default 300) with no keyboard or mouse input, read from `CGEventSourceSecondsSinceLastEventType`. The icon uses the same paused mark, and the tooltip says "Polling paused while idle". The next input resumes polling and fetches immediately. A manual or timed pause wins over idle: coming back to the keyboard does not clear "Pause for 1 hour". Idle detection is a local timer only. It does not call GitHub.

Sleep stays a process suspend, described above, and does not set `poll_pause`. An idle pause and a user pause are the ones that show the paused icon.

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

# Also edited from the popover presets (15s, 30s, 1m, 2m).
# Allowed range is 15 to 900 seconds (15 minutes). Default 60.
poll_interval_seconds = 60

# Written by Pause / Resume. Omit the key, or set "running", to poll.
# "manual" stays paused until Resume. An RFC3339 timestamp resumes then.
# poll_pause = "manual"

# Skip GitHub while the Mac is idle. Default off. See "Pause and resume".
pause_when_idle = false
idle_pause_after_seconds = 300

# GraphQL has no ETags. The REST probe below can skip a GraphQL call only
# while every repo returns 304 and the last check-run snapshot is younger
# than ci_max_staleness_seconds. Leave the probe off to keep CI within
# poll_interval_seconds. Check runs do not change the issues-list ETag.
use_etag_probe = false
ci_max_staleness_seconds = 60

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

Nothing in v1 is hardcoded to `springfield-ham-radio` except the default file written on first launch. The popover is another editor for `poll_interval_seconds` and `poll_pause`. Both paths write the same file.

## 4. GitHub auth

The token lives in the macOS Keychain (service `com.springfield.agent-dashboard`, account `github-token`) via the `keyring` crate. It is never written to `config.toml`, `snapshot.json`, logs, or the webview. Rust performs every GitHub call. The frontend receives the grouped snapshot only.

A dev rebuild often changes the binary's code signature, and macOS will prompt once to allow the new binary to read the keychain item. That prompt is expected.

### Comparison

| | `gh auth token` | Fine-grained PAT | Device-flow OAuth app |
| --- | --- | --- | --- |
| Privilege | Whatever `gh` already has. Often `repo` plus the ability to push and admin | Read-only Issues, Pull requests, and Commit statuses on the chosen org (no separate Checks permission on fine-grained PATs) | Classic scopes only. Private repos mean the `repo` scope, which can push |
| Setup | Lowest if `gh` is already logged in. The app depends on the `gh` binary and on calling it every poll so a refreshed token is picked up | Create a token, paste it once, approve it for the org if the org requires that | Register an OAuth app, implement device flow and refresh, get the app approved for the org |
| Org approval | Already done for `gh` | Required when the org gates fine-grained tokens. One owner approval | Required when the org gates OAuth apps |
| Expiry | `gh`'s own refresh, or never, if a PAT was pasted into `gh` | Required, at most one year. A 401 means the popover asks for a new token | Access token about 8 hours. A refresh token must also live in the Keychain |
| Secret in the app binary | None | None | The OAuth client secret has to ship inside a public desktop client |
| Fit for this app | Convenient, and broad enough to push | Read phases stay read-only. The write phase adds Issues and Pull requests write, and still omits Contents | Fits a later build that other people install |

### Recommendation

Use a fine-grained PAT in the Keychain. Through Phase 4 the token is read-only. Phase 5 (row actions) needs Issues **Read and write** and Pull requests **Read and write** on the same token. Contents stays off, so the token still cannot push commits or merge. Device flow cannot express that split, and it would embed a client secret. `gh auth token` stays available as `credential = "gh"` for a day when Bryan would rather not mint a second secret. A typical `gh` token can already comment and relabel, and it can also push, which this app does not need.

Token settings to document in the README:

- Resource owner: `springfield-ham-radio` (add `springfield-tools` when that org is in the config)
- Repository access: all repositories, so a new repo appears without a new token
- Phases 1–4: Metadata Read (automatic), Issues Read, Pull requests Read, Commit statuses Read. Fine-grained PATs have no **Checks** permission; `statusCheckRollup.state` still works on public repos, but `CheckRun` names in rollup contexts may be empty until GitHub closes that gap (or use `credential = "gh"`).
- Phase 5: the same list, with Issues and Pull requests raised to Read and write. Commit statuses stays Read. Contents stays off unless rollups come back empty on a private repo
- If a future private repo returns an empty rollup, add Contents Read then
- Expiration: 90 days is a reasonable first choice

GitHub sets fine-grained permissions when the token is created. Raising Issues and Pull requests to write usually means a new token pasted into the existing Keychain prompt. The read phases keep working if Bryan creates the write token early. A 401 still means "token rejected" and is not retried. A 403 on a comment or label call means the token is missing write, and the popover says so and links to the token settings page.

All 12 ham-radio repos are public today. The read set is enough for the measured query. Commenting and labeling use the issues API for both issues and pull requests (`POST /repos/{owner}/{repo}/issues/{number}/comments`, `POST` and `DELETE` on `.../issues/{number}/labels`). Fine-grained tokens authorize those routes per item kind: Issues write for an issue, Pull requests write for a pull request. The write phase requests both.

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
- **Merge.** `POST /repos/{owner}/{repo}/pulls/{pull_number}/merge` needs a token that can update the branch, and this org's required checks are check runs, so `mergeable` is eventually consistent and easy to get wrong. A merge cannot be undone by restoring labels. It stays out of Phase 5.
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

**Scope:** `config.example.toml`, loader, and `model.rs` with the precedence and bot-filter tests from section 1. The loader accepts `poll_interval_seconds` and rejects values outside 15–900. The popover and tray title render a checked-in fixture shaped like the GraphQL snapshot (today's 33 PRs is a fine fixture). Changing `labels` or `filters.bots` in the Application Support config and restarting re-groups the fixture. Still no network. The interval control and pause actions land in Phase 3, when a timer exists to reschedule.

**Acceptance:**

- The fixture's 29 green Dependabot PRs collapse to one footer row, and the two human drafts sit in Not started.
- A fixture PR labeled `needs-testing` with rollup `FAILURE` appears only under CI failing and keeps a needs-testing chip.
- A fixture item labeled `needs-input` and `agent-working` appears only under Needs input.
- Tray title for an all-quiet fixture is empty.

**Bryan tests:** from `~/Development/springfield-tools/agent-dashboard`, run `yarn tauri:dev`. Edit `~/Library/Application Support/com.springfield.agent-dashboard/config.toml`, set `filters.bots` to `show`, restart, and see the Dependabot rows appear under Not started. Set it back to `collapse`.

### Phase 3 — Live GitHub

**Scope:** Keychain write/read, settings affordance to paste the PAT once, GraphQL poll on the configured interval, grouping of the live payload, `snapshot.json` rewritten on every success. Popover interval presets that write `poll_interval_seconds` and reschedule. Pause, Resume, Pause for 1 hour, and Pause until tomorrow from both the tray menu and the popover, with the paused icon and title. Manual Refresh, including one-shot refresh while paused. Default org `springfield-ham-radio`. The fixture remains the offline demo behind a `--fixture` flag so Phase 2's checks still run without a token.

**Acceptance:**

- With a valid token, the popover matches GitHub within one poll: the two human drafts, the collapsed Dependabot row (29 green, 2 with no CI), and zero rows in the four label groups.
- The header shows the default interval of 60 seconds. Choosing 30 seconds writes `~/Library/Application Support/com.springfield.agent-dashboard/config.toml` and the following gap is 30 seconds. A value under 15 or over 900 is rejected and the previous interval keeps running.
- Pause stops GitHub calls, switches the tray icon to the pause mark, and replaces the count title with `Paused`. Resume clears `poll_pause` and fetches immediately. Pause for 1 hour and Pause until tomorrow (next local midnight) survive quit and relaunch and then resume on their own.
- Refresh while paused fetches once and leaves the pause in place.
- `snapshot.json` `schemaVersion` is 1 and contains no token.
- An invalid token shows the rejection banner and does not spin on backoff.
- Opening a row opens that pull request in the browser.

**Bryan tests:**

```bash
cd ~/Development/springfield-tools/agent-dashboard
yarn tauri:dev
```

Paste the fine-grained PAT when asked. Compare the popover with `https://github.com/pulls?q=org%3Aspringfield-ham-radio+is%3Aopen`. Open the `ham-radio-ui` draft row and confirm the local branch in `~/Development/springfield-ham-radio/ham-radio-ui` is the one you expect. Add a temporary `needs-input` label to a throwaway issue or PR, wait one interval, and see it take the top section. Remove the label afterward.

From the same checkout, set the popover interval to 1m and confirm the config file and the header. Pause from the tray menu, confirm the icon says `Paused` and the "last updated" time holds still, then Resume and confirm a fetch. Pause for 1 hour, quit the app, run `yarn tauri:dev` again, and confirm it is still paused with the resume time in the tooltip.

### Phase 4 — Poll hardening

**Scope:** Backoff, rate-limit brakes, wake refresh, offline banner, the ETag probe defaulted off, and optional idle auto-pause defaulted off. Tests use a fake clock and a fake HTTP layer. A user pause from Phase 3 stays in effect across wake.

**Acceptance:**

- Airplane mode (or a denied network) leaves the last snapshot up, with the offline banner, and recovers on the next success without a burst of parallel polls.
- Closing the lid for a minute and reopening refreshes once, then returns to the configured interval (default 60 seconds). The icon stays the normal mark across that wake.
- Closing the lid while Pause for 1 hour is active wakes still paused, with no GitHub call.
- `pause_when_idle = true` and `idle_pause_after_seconds = 10` stops polls after 10 seconds with no input, shows the paused icon with the idle tooltip, and resumes on the next key or mouse event. A manual pause is not cleared by that input.
- Setting `use_etag_probe = true` and `ci_max_staleness_seconds = 300` skips GraphQL while the issues lists 304, and the popover says CI may be up to five minutes old.

**Bryan tests:** from `~/Development/springfield-tools/agent-dashboard`, run `yarn tauri:dev`, turn Wi-Fi off, confirm the banner and the still-visible rows, turn Wi-Fi on, confirm a refresh. Sleep the Mac briefly and confirm a refresh on wake. Pause for 1 hour, sleep again, and confirm it is still paused. Then set `pause_when_idle = true` and `idle_pause_after_seconds = 10` in `~/Library/Application Support/com.springfield.agent-dashboard/config.toml`, leave the Mac idle, and confirm the paused icon. Touch the trackpad and confirm a fetch. Set `pause_when_idle` back to false afterward.

### Phase 5 — Row actions

Read-only phases 1–4 stay as they are. This phase is the first one that writes to GitHub. Row click still opens the GitHub page. Actions live on a menu on the row.

**Scope.** The actions below. Each one opens a confirm sheet before any request. The sheet shows the repo, number, title, the label diff, and the exact comment. Confirm sends it. Cancel sends nothing. The note field is editable on the sheet.

| Action | Shown on | Labels | Comment |
| --- | --- | --- | --- |
| Testing passed → Ready to merge | Needs testing. Default button in that sheet | Remove `needs-testing`, add `ready-to-merge` | Audit only |
| Testing passed → Hand back to agent | Second button on the same sheet | Remove `needs-testing`, add `agent-working` | `@cursor` plus an optional note. Same path as Send back |
| Send back to agent | Needs testing, CI failing | Remove `needs-testing`, `needs-input`, and `ready-to-merge` when present. Add `agent-working`. Leave every other label alone | `@cursor` plus an optional note. CI failing prefills the failed check names |
| Answer | Needs input | Remove `needs-input`, add `agent-working` | `@cursor` plus the answer. The answer is required |
| Approve / ready to merge | Not started, and Agent working | Add `ready-to-merge`. Remove `needs-testing` when present | Audit only |

Needs testing does not also show Approve. Testing passed → Ready to merge is that action. CI failing does not show Approve. Needs input shows Answer and does not show a note-less Send back.

Label calls add and remove only those status labels. They do not `PUT` the full label set, so `dependencies`, `bug`, and the rest stay.

**Cursor handoff.** The dashboard does not call a Cursor API. It posts a normal GitHub comment as Bryan, and the Cursor GitHub App wakes a cloud agent from that comment. Sources, checked 2026-10-10:

- [Cloud Agents](https://cursor.com/docs/cloud-agent): "Comment `@cursor` on a GitHub PR or issue, or on a Bitbucket PR, to kick off an agent." The GitHub connection has to be in place first ([GitHub integration](https://cursor.com/docs/integrations/github), app listed at `github.com/apps/cursor`), the account needs repository access, and Cloud Agents need a paid Cursor plan.
- Cursor staff on the forum: put `@cursor` at the start of the comment; that is the reliable form. The instruction is the rest of the same comment. Example shape: `@cursor read userId from req.body instead of req.query, validate as string, create a new branch and open a fix PR`. The agent reads the diff, the thread, and CI. This path works on a merged or closed PR. The dashboard only offers it on open items.
- The same staff note: a comment on the pull request is the supported way to keep the agent on that PR's head branch. Agents started from the Cursor window open a fresh `cursor/…` branch. Agents started from a `@cursor` comment on the PR commit to the PR branch.
- Cursor staff, April 2026: a follow-up `@cursor` comment continues the existing session on a pull request, matched by the PR branch. On a GitHub issue, each `@cursor` mention starts a new session.
- A second Cursor user mentioning `@cursor` on a PR that already has someone else's agent is treated as a follow-up and can be denied unless team follow-ups are on. Posting as Bryan's user token, which is the PAT this app stores, makes Bryan the comment author. A GitHub App bot comment is a different author and is not the path this phase uses.
- [Capabilities](https://cursor.com/docs/cloud-agent/capabilities): `@cursor autofix off` and `@cursor autofix on` toggle automatic CI repair on that PR. `@cursor please fix the CI failures` is an ordinary instruction. Bugbot is a different feature and starts from `cursor review` or `bugbot run`. Generated bodies must not use those strings.

The comment is a top-level issue comment (`POST /repos/{owner}/{repo}/issues/{number}/comments`), which is the conversation on an issue or pull request. It is not an inline review comment.

Agent-waking body:

```text
@cursor Testing did not pass. Continue on this pull request.
<optional note Bryan typed>

— Agent Dashboard
```

Answer uses the same shape with the answer in place of the default sentence. Audit-only body:

```text
Testing passed. Ready to merge.

— Agent Dashboard
```

`@cursor` is the first token when the action should wake an agent. The last line is always `— Agent Dashboard`, so a comment Bryan typed by hand is distinguishable from one the tray posted. Approve uses "Approved. Ready to merge." Send back from CI failing prefills "Look at the failing checks: <names>. Continue on this pull request." On an issue, the default sentence is "Continue on this issue." and the plan records that this starts a new cloud agent session.

There is no documented `@cursor stop`. Undo does not send `autofix off`.

**Confirm, optimistic UI, undo.**

- The first click opens the sheet. Nothing is sent until Confirm.
- On Confirm the row moves to the destination group immediately and shows a pending mark.
- The app posts the comment, then applies the label diff. Other labels are untouched.
- Success clears the pending mark and keeps an undo record: previous status labels, comment id, and whether the body started with `@cursor`.
- Failure moves the row back and shows GitHub's error. If the comment was posted and the labels failed, the banner offers Retry labels. If the labels changed and the comment failed, the banner offers Retry comment.
- The next successful poll reconciles. When GitHub matches the optimistic labels, the pending mark drops.
- Undo stays available until that poll or for 60 seconds, whichever comes first. Undo restores the previous status labels.
- When the comment did not start with `@cursor`, Undo also deletes it.
- When it did, Undo restores the labels and leaves the comment. The toast says the cloud agent may already be running.

**Tests.** Unit tests for the comment builder: agent-waking bodies match `^@cursor `, every body ends with `— Agent Dashboard`, and no body contains `cursor review`, `bugbot run`, `@cursor autofix off`, or `@cursor autofix on`. Tests for the label diff cover "add and remove only the status labels". A fake HTTP layer covers comment-then-labels, each partial failure, and undo.

**Acceptance.**

- With a read-only token, Confirm shows the permission banner and leaves labels and comments unchanged.
- With Issues and Pull requests write, Testing passed → Ready to merge on a scratch issue removes `needs-testing`, adds `ready-to-merge`, and posts an audit comment with no `@cursor`. Undo within the window restores the labels and deletes that comment.
- Send back on a scratch issue posts a comment whose first token is `@cursor`, adds `agent-working`, and a cloud agent run appears for Bryan's Cursor account. Undo restores the labels and leaves the comment.
- Answer on a `needs-input` item requires text, then does the same handoff.
- Approve on a not-started item adds `ready-to-merge` and an audit comment.
- A label such as `dependencies` is still present after every action.
- The poller is unchanged: it never comments or relabels on its own.

**Bryan tests.** From `~/Development/springfield-tools/agent-dashboard`, run `yarn tauri:dev`. In a scratch issue under `~/Development/springfield-ham-radio/<repo>` (create one; leave `ham-radio-ui` #84 alone), add `needs-testing`, run Testing passed → Ready to merge, and confirm the labels and the footer on GitHub. Undo from the popover and confirm the label and the comment are gone. Then Send back with a short note and confirm a cloud agent starts on that issue. Remove the scratch issue's labels when finished. Paste a write-capable token when the app reports a 403.

### Phase 6 — Release

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
5. Auth. Is the fine-grained PAT acceptable, or do you want `credential = "gh"` as the default despite the broader scopes? Phase 5 raises that PAT from read to Issues and Pull requests write, and still leaves Contents off.
6. Token lifetime. 90 days, or the one-year maximum?
7. Private repos. All 12 ham-radio repos are public. If a private repo should show up, the token needs access to it. Any planned?
8. Bots. Is collapse-plus-surface-failures the right default, or should Dependabot be hidden entirely?
9. The "about 11 open issues" figure. On 2026-10-10 the REST issues API returned 0 open issues across all 12 repos, and 33 open PRs. Were those 11 issues since closed, or do they live on a project board or another org this plan should include?
10. Required check. Is `yarn lint`, `yarn test`, `cargo test`, and `cargo build` enough for the required `build` job, or must that job also produce a `.app`?
11. Release secrets. Can this repo reuse the ham-radio-ui `RELEASE_APP_ID` / `RELEASE_APP_PRIVATE_KEY` GitHub App, or does it need its own?
12. Morning summary. Is reading `snapshot.json` from Application Support a workable handoff, and does that job run on the same Mac?
13. Label names. `.github` pull request 4 ("Sync org issue labels from labels.yml") is still a draft. Are `agent-working`, `needs-testing`, `needs-input`, and `ready-to-merge` stable?
14. Idle and sleep. Idle auto-pause defaults off, with a 5-minute threshold, and sleep only suspends the process. Should idle auto-pause default on? Is "until tomorrow" the next local midnight, or a morning hour?
15. On Needs testing, is Ready to merge the default choice, with Hand back to agent as the second button?
16. Should Approve be offered while a row is still Agent working?
17. Answering a GitHub issue with `@cursor` starts a new cloud agent session each time. Is that acceptable, or should `@cursor` be limited to pull requests?
18. Merge stays a later idea. Confirm that Phase 5 should not merge.

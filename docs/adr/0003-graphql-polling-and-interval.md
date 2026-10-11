# 0003. GraphQL org repositories, statusCheckRollup, and poll interval

## Status

Accepted (amended 2026-10-11)

## Context

Across many org repos, REST per-repo polling is heavy. Combined commit status on `springfield-ham-radio` is empty; CI appears only on check runs. GraphQL has no ETags. Bryan wants a configurable refresh rate without blowing the 5,000 points/hour GraphQL budget.

**Fine-grained PATs:** GraphQL `search(query: "org:…")` often returns HTTP 200 with `errors` (forbidden search) and empty `nodes`, while `issueCount` may still be non-zero. OAuth/`gh` tokens can search; fine-grained tokens should not rely on search.

## Decision

- Poll with **`organization(login).repositories`** per configured org (paginated, `first: 50` repos per page), loading open **pullRequests** and **issues** per repo with **`statusCheckRollup`** on PR heads. Client-side `include` / `exclude` repo lists and `isArchived` skipping match the old search design.
- Legacy search string for reference: `org:<name> is:open archived:false` (not used for live fetch).
- **`poll_interval_seconds`** defaults to **30**, configurable in Application Support config and the popover (**15–900** seconds).
- Optional REST issues-list ETag probe ships **default off** (`use_etag_probe = false`).
- User **pause/resume** (including timed pause) stops polling; optional idle pause defaults off.
- Empty fetches must surface a **banner** (`empty_hint` or GraphQL warning), never a silent success.

## Consequences

- GraphQL cost scales with repo count, pagination, and failure-context follow-ups. Measured **2026-10-11** (`scripts/measure-graphql-cost.sh`): one cheap `springfield-ham-radio` org page costs **4** points per tick (~480/hr at the 30s default). Budget tables in `docs/PLAN.md` use **4** measured / **8** headroom. Hot path omits `contexts`; failing PRs fetch `contexts(first: 10)` in a separate query (~1 pt each).
- Paused polling spends no GraphQL points.
- A follow-up phase adds row write actions without changing the poller’s read-only contract until invoked by the user.

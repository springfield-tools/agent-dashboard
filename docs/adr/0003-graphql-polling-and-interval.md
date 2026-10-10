# 0003. GraphQL org search, statusCheckRollup, and poll interval

## Status

Accepted

## Context

Across many org repos, REST per-repo polling is heavy. Combined commit status on `springfield-ham-radio` is empty; CI appears only on check runs. GraphQL has no ETags. Bryan wants a configurable refresh rate without blowing the 5,000 points/hour GraphQL budget.

## Decision

- Poll with **one GraphQL search per configured org** (`org:<name> is:open archived:false`), paginated at `first: 100`.
- Read CI from **`statusCheckRollup`** on the pull request head commit, not legacy `status` or REST combined status.
- **`poll_interval_seconds`** defaults to **30**, configurable in Application Support config and the popover (**15–900** seconds).
- Optional REST issues-list ETag probe ships **default off** (`use_etag_probe = false`).
- User **pause/resume** (including timed pause) stops polling; optional idle pause defaults off.

## Consequences

- Measured query cost (~2 points/org/tick) scales linearly with org count; budget tables in `docs/PLAN.md` guide interval choice.
- Paused polling spends no GraphQL points.
- A follow-up phase adds row write actions without changing the poller’s read-only contract until invoked by the user.

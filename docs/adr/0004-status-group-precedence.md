# 0004. Status groups and precedence

## Status

Accepted

## Context

Issues and pull requests can carry several labels and a CI rollup at once. The tray and popover must show each item in exactly one primary group so counts and attention order stay predictable.

## Decision

Assign each open issue or PR to **one** group; **first match wins**:

1. Needs input (`needs-input`)
2. CI failing (`statusCheckRollup` is `FAILURE` or `ERROR` on a PR)
3. Needs testing (`needs-testing`)
4. Ready to merge (`ready-to-merge`)
5. Agent working (`agent-working`)
6. Not started (none of the four status labels, CI not failing)

Other labels remain visible as row chips. **Dependabot** defaults to **collapse** with `surface_failing_bots = true`. Tray title shows four counts only: needs input, needs testing, CI failing, ready to merge (`input · testing · failing · merge`).

## Consequences

- Needs-input outranks CI red so Bryan sees blockers first.
- CI failing outranks testing/merge labels when the head commit is not green.
- Draft PRs appear in their computed group (default: Not started when unlabeled).
- Label names are configurable in TOML but precedence order stays in code.

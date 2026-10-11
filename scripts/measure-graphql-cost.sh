#!/usr/bin/env bash
set -euo pipefail
TOKEN="${GITHUB_TOKEN:-$(gh auth token 2>/dev/null || true)}"
if [[ -z "$TOKEN" ]]; then
  echo "No GitHub token (set GITHUB_TOKEN or gh auth login)" >&2
  exit 1
fi
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
QUERY="$(tr -d '\n' < "$ROOT/scripts/org_open_query.graphql" | sed 's/"/\\"/g')"
BODY="{\"query\":\"$QUERY\",\"variables\":{\"org\":\"springfield-ham-radio\",\"repoAfter\":null}}"
curl -sS -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -H "User-Agent: agent-dashboard-measure" \
  -d "$BODY" \
  https://api.github.com/graphql | jq '.data.rateLimit, .errors'

use crate::config::{AppConfig, OrgConfig};
use crate::model::{CiSnapshot, ItemKind, RawItem};

const DASHBOARD_QUERY: &str = r"
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
";

#[derive(Debug, thiserror::Error)]
pub enum GraphqlError {
    #[error("http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("graphql: {0}")]
    GraphQl(String),
    #[error("network: {0}")]
    Network(String),
    #[error("unauthorized")]
    Unauthorized,
}

pub struct RateLimitInfo {
    pub cost: i32,
    pub remaining: i32,
    pub reset_at: String,
}

pub struct FetchResult {
    pub items: Vec<RawItem>,
    pub rate_limit: Option<RateLimitInfo>,
    pub warning: Option<String>,
}

pub async fn fetch_org_open_items(
    client: &reqwest::Client,
    token: &str,
    org: &OrgConfig,
) -> Result<FetchResult, GraphqlError> {
    let search = format!("org:{} is:open archived:false", org.name);
    let mut after: Option<String> = None;
    let mut all_items = Vec::new();
    let mut last_rate: Option<RateLimitInfo> = None;
    let mut warning: Option<String> = None;

    loop {
        let body = serde_json::json!({
            "query": DASHBOARD_QUERY,
            "variables": {
                "searchQuery": search,
                "after": after,
            }
        });
        let response = client
            .post("https://api.github.com/graphql")
            .bearer_auth(token)
            .header("User-Agent", "agent-dashboard")
            .json(&body)
            .send()
            .await
            .map_err(|e| GraphqlError::Network(e.to_string()))?;

        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .map_err(|e| GraphqlError::Network(e.to_string()))?;

        if status == 401 {
            return Err(GraphqlError::Unauthorized);
        }
        if status == 403 || status == 429 {
            return Err(GraphqlError::Http { status, body: text });
        }
        if !(200..300).contains(&status) {
            return Err(GraphqlError::Http { status, body: text });
        }

        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| GraphqlError::GraphQl(e.to_string()))?;

        if let Some(errors) = json.get("errors") {
            if json.get("data").and_then(|d| d.get("search")).is_none() {
                return Err(GraphqlError::GraphQl(errors.to_string()));
            }
            warning = Some(errors.to_string());
        }

        if let Some(rl) = json.pointer("/data/rateLimit") {
            last_rate = Some(RateLimitInfo {
                cost: rl.get("cost").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                remaining: rl.get("remaining").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                reset_at: rl
                    .get("resetAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }

        let search = json
            .pointer("/data/search")
            .ok_or_else(|| GraphqlError::GraphQl("missing data.search".to_string()))?;

        let nodes = search
            .get("nodes")
            .and_then(|n| n.as_array())
            .cloned()
            .unwrap_or_default();

        for node in nodes {
            if let Some(item) = parse_node(&node) {
                if item_matches_org_filter(&item, org) {
                    all_items.push(item);
                }
            }
        }

        let has_next = search
            .pointer("/pageInfo/hasNextPage")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !has_next {
            break;
        }
        after = search
            .pointer("/pageInfo/endCursor")
            .and_then(|v| v.as_str())
            .map(String::from);
        if after.is_none() {
            break;
        }
    }

    Ok(FetchResult {
        items: all_items,
        rate_limit: last_rate,
        warning,
    })
}

pub async fn fetch_all_orgs(
    client: &reqwest::Client,
    token: &str,
    config: &AppConfig,
) -> Result<FetchResult, GraphqlError> {
    let mut combined = Vec::new();
    let mut rate_limit = None;
    let mut warning = None;

    for org in &config.orgs {
        let result = fetch_org_open_items(client, token, org).await?;
        combined.extend(result.items);
        rate_limit = result.rate_limit;
        if result.warning.is_some() {
            warning = result.warning;
        }
    }

    Ok(FetchResult {
        items: combined,
        rate_limit,
        warning,
    })
}

fn item_matches_org_filter(item: &RawItem, org: &OrgConfig) -> bool {
    if !org.include.is_empty() && !org.include.contains(&item.repo) {
        return false;
    }
    if org.exclude.contains(&item.repo) {
        return false;
    }
    true
}

fn parse_node(node: &serde_json::Value) -> Option<RawItem> {
    let typename = node.get("__typename").and_then(|v| v.as_str())?;
    let repo = node
        .pointer("/repository/name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let name_with_owner = node
        .pointer("/repository/nameWithOwner")
        .and_then(|v| v.as_str())
        .unwrap_or(repo);
    let org = name_with_owner
        .split('/')
        .next()
        .unwrap_or("unknown")
        .to_string();
    let labels: Vec<String> = node
        .pointer("/labels/nodes")
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let author = node
        .pointer("/author/login")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let (kind, is_draft, ci) = match typename {
        "Issue" => (ItemKind::Issue, false, None),
        "PullRequest" => {
            let is_draft = node.get("isDraft").and_then(|v| v.as_bool()).unwrap_or(false);
            let rollup = node.pointer("/commits/nodes/0/commit/statusCheckRollup");
            let ci = rollup.map(parse_rollup).unwrap_or(None);
            (ItemKind::Pr, is_draft, ci)
        }
        _ => return None,
    };

    Some(RawItem {
        kind,
        org,
        repo: repo.to_string(),
        number: node.get("number").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        title: node
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        url: node
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        author,
        updated_at: node
            .get("updatedAt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        is_draft,
        labels,
        ci,
    })
}

fn parse_rollup(rollup: &serde_json::Value) -> Option<CiSnapshot> {
    let state = rollup
        .get("state")
        .and_then(|v| v.as_str())
        .map(String::from);
    let mut failed_checks = Vec::new();
    if let Some(nodes) = rollup
        .pointer("/contexts/nodes")
        .and_then(|n| n.as_array())
    {
        for ctx in nodes {
            if let Some(name) = ctx.get("name").and_then(|v| v.as_str()) {
                let conclusion = ctx.get("conclusion").and_then(|v| v.as_str());
                if matches!(conclusion, Some("FAILURE") | Some("CANCELLED") | Some("TIMED_OUT")) {
                    failed_checks.push(name.to_string());
                }
            } else if let Some(context) = ctx.get("context").and_then(|v| v.as_str()) {
                let state = ctx.get("state").and_then(|v| v.as_str());
                if state == Some("FAILURE") {
                    failed_checks.push(context.to_string());
                }
            }
        }
    }
    Some(CiSnapshot {
        state,
        failed_checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pull_request_node() {
        let node: serde_json::Value = serde_json::json!({
            "__typename": "PullRequest",
            "number": 84,
            "title": "draft",
            "url": "https://github.com/o/r/pull/84",
            "updatedAt": "2026-10-10T12:00:00Z",
            "isDraft": true,
            "author": { "login": "human" },
            "labels": { "nodes": [] },
            "repository": { "name": "r", "nameWithOwner": "springfield-ham-radio/r" },
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "SUCCESS", "contexts": { "nodes": [] } } } }] }
        });
        let item = parse_node(&node).expect("item");
        assert_eq!(item.kind, ItemKind::Pr);
        assert!(item.is_draft);
    }
}

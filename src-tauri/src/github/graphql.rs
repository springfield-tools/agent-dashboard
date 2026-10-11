use reqwest::header::{HeaderMap, RETRY_AFTER};
use reqwest::Response;

use crate::config::{AppConfig, OrgConfig};
use crate::model::{CiSnapshot, ItemKind, RawItem};

/// Legacy search query (OAuth / classic tokens). Fine-grained PATs often cannot search orgs.
pub fn legacy_search_query_string(org: &OrgConfig) -> String {
    format!("org:{} is:open archived:false", org.name)
}

/// Cheap org page: small `first` limits, rollup `state` only (no contexts on hot path).
pub(crate) const ORG_OPEN_QUERY: &str = r"
query OrgOpenItems($org: String!, $repoAfter: String) {
  rateLimit { cost remaining resetAt }
  organization(login: $org) {
    repositories(first: 15, after: $repoAfter, orderBy: {field: PUSHED_AT, direction: DESC}, isFork: false) {
      totalCount
      pageInfo { hasNextPage endCursor }
      nodes {
        name
        isArchived
        pullRequests(states: OPEN, first: 10) {
          pageInfo { hasNextPage endCursor }
          nodes {
            number title url updatedAt isDraft
            author { login }
            labels(first: 5) { nodes { name } }
            commits(last: 1) {
              nodes { commit { statusCheckRollup { state } } }
            }
          }
        }
        issues(states: OPEN, first: 5) {
          pageInfo { hasNextPage endCursor }
          nodes {
            number title url updatedAt
            author { login }
            labels(first: 5) { nodes { name } }
          }
        }
      }
    }
  }
}
";

const REPO_PR_PAGE_QUERY: &str = r"
query RepoPrPage($owner: String!, $name: String!, $after: String) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    pullRequests(states: OPEN, first: 10, after: $after) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number title url updatedAt isDraft
        author { login }
        labels(first: 5) { nodes { name } }
        commits(last: 1) {
          nodes { commit { statusCheckRollup { state } } }
        }
      }
    }
  }
}
";

const REPO_ISSUE_PAGE_QUERY: &str = r"
query RepoIssuePage($owner: String!, $name: String!, $after: String) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    issues(states: OPEN, first: 5, after: $after) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number title url updatedAt
        author { login }
        labels(first: 5) { nodes { name } }
      }
    }
  }
}
";

const PR_FAILURE_CONTEXTS_QUERY: &str = r"
query PrFailureContexts($owner: String!, $name: String!, $number: Int!) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              contexts(first: 10) {
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
";

#[allow(dead_code)]
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
    #[error("graphql: {0}")]
    GraphQl(String),
    #[error("network: {0}")]
    Network(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("rate limited")]
    RateLimited {
        retry_after_secs: Option<u64>,
        reset_at: Option<String>,
        /// True when the primary GraphQL budget is exhausted (remaining 0).
        primary_exhausted: bool,
        message: String,
    },
    #[error("forbidden")]
    Forbidden { status: u16, message: String },
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
    /// Shown when the fetch succeeded but returned no items for a suspicious reason.
    pub empty_hint: Option<String>,
    pub repos_visible: u32,
}

pub(crate) struct SearchPage {
    pub items: Vec<RawItem>,
    pub rate_limit: Option<RateLimitInfo>,
    pub warning: Option<String>,
    pub has_next: bool,
    pub end_cursor: Option<String>,
    pub issue_count: Option<u64>,
}

#[derive(Clone)]
pub(crate) enum RepoItemPageKind {
    PullRequests,
    Issues,
}

pub(crate) struct PendingRepoPagination {
    pub repo: String,
    pub kind: RepoItemPageKind,
    pub after: String,
}

pub(crate) struct OrgRepoPage {
    pub items: Vec<RawItem>,
    pub rate_limit: Option<RateLimitInfo>,
    pub warning: Option<String>,
    pub repos_in_page: u32,
    pub total_repo_count: Option<u32>,
    pub has_next: bool,
    pub end_cursor: Option<String>,
    pub organization_missing: bool,
    pub pending: Vec<PendingRepoPagination>,
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn parse_retry_after(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn header_reset_at(headers: &HeaderMap) -> Option<String> {
    header_u64(headers, "x-ratelimit-reset").and_then(|epoch| {
        chrono::DateTime::from_timestamp(epoch as i64, 0).map(|t| {
            t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        })
    })
}

fn classify_http_error(status: u16, headers: &HeaderMap, body: &str) -> GraphqlError {
    let remaining = header_u64(headers, "x-ratelimit-remaining");
    let retry_after = parse_retry_after(headers);
    let rate_limited = status == 429
        || remaining == Some(0)
        || body.contains("RATE_LIMITED")
        || body.contains("rate limit");

    if rate_limited {
        return GraphqlError::RateLimited {
            retry_after_secs: retry_after,
            reset_at: header_reset_at(headers),
            primary_exhausted: remaining == Some(0),
            message: truncate_body(body),
        };
    }

    GraphqlError::Forbidden {
        status,
        message: truncate_body(body),
    }
}

fn truncate_body(body: &str) -> String {
    if body.len() > 500 {
        format!("{}…", &body[..500])
    } else {
        body.to_string()
    }
}

fn graphql_errors_fatal(errors: &serde_json::Value) -> Option<GraphqlError> {
    let arr = errors.as_array()?;
    for entry in arr {
        let err_type = entry.get("type").and_then(|v| v.as_str());
        if err_type == Some("RATE_LIMITED") {
            return Some(GraphqlError::RateLimited {
                retry_after_secs: None,
                reset_at: None,
                primary_exhausted: true,
                message: errors.to_string(),
            });
        }
    }
    None
}

pub(crate) fn suspicious_search_result(json: &serde_json::Value, items_len: usize) -> Option<String> {
    if items_len > 0 {
        return None;
    }
    if let Some(errors) = json.get("errors") {
        let msg = errors.to_string();
        if msg.contains("cannot be searched") || msg.contains("FORBIDDEN") {
            return Some(
                "GitHub search is not available for this token (common with fine-grained PATs). The app uses organization.repositories instead.".to_string(),
            );
        }
        if json.pointer("/data/search").is_some() {
            let count = json
                .pointer("/data/search/issueCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if count > 0 {
                return Some(format!(
                    "GitHub search reported {count} open items but returned none. Check token permissions."
                ));
            }
            return Some("GraphQL returned errors and no searchable items.".to_string());
        }
    }
    let count = json
        .pointer("/data/search/issueCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if count > 0 {
        return Some(format!(
            "GitHub search reported {count} open items but returned none."
        ));
    }
    None
}

pub(crate) fn ingest_search_response(
    json: &serde_json::Value,
    org: &OrgConfig,
) -> Result<SearchPage, GraphqlError> {
    if let Some(errors) = json.get("errors") {
        if let Some(fatal) = graphql_errors_fatal(errors) {
            return Err(fatal);
        }
        if json.get("data").and_then(|d| d.get("search")).is_none() {
            return Err(GraphqlError::GraphQl(errors.to_string()));
        }
    }

    let rate_limit = json.pointer("/data/rateLimit").map(parse_rate_limit);

    let warning = json.get("errors").map(|errors| errors.to_string());

    let search = json
        .pointer("/data/search")
        .ok_or_else(|| GraphqlError::GraphQl("missing data.search".to_string()))?;

    let issue_count = search.get("issueCount").and_then(|v| v.as_u64());

    let mut items = Vec::new();
    let nodes = search
        .get("nodes")
        .and_then(|n| n.as_array())
        .cloned()
        .unwrap_or_default();

    for node in nodes {
        if let Some(item) = parse_node(&node) {
            if item_matches_org_filter(&item, org) {
                items.push(item);
            }
        }
    }

    let has_next = search
        .pointer("/pageInfo/hasNextPage")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let end_cursor = search
        .pointer("/pageInfo/endCursor")
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok(SearchPage {
        items,
        rate_limit,
        warning,
        has_next,
        end_cursor,
        issue_count,
    })
}

pub(crate) fn ingest_org_repositories_response(
    json: &serde_json::Value,
    org: &OrgConfig,
) -> Result<OrgRepoPage, GraphqlError> {
    if let Some(errors) = json.get("errors") {
        if let Some(fatal) = graphql_errors_fatal(errors) {
            return Err(fatal);
        }
        if json.pointer("/data/organization").is_none()
            && json.get("data").and_then(|d| d.get("organization")).is_none()
        {
            return Err(GraphqlError::GraphQl(errors.to_string()));
        }
    }

    let rate_limit = json.pointer("/data/rateLimit").map(parse_rate_limit);
    let warning = json.get("errors").map(|errors| errors.to_string());

    let organization_missing = match json.pointer("/data/organization") {
        None => true,
        Some(v) => v.is_null(),
    };
    if organization_missing {
        return Ok(OrgRepoPage {
            items: vec![],
            rate_limit,
            warning,
            repos_in_page: 0,
            total_repo_count: None,
            has_next: false,
            end_cursor: None,
            organization_missing: true,
            pending: vec![],
        });
    }

    let organization = json.pointer("/data/organization").filter(|v| !v.is_null());

    let repos = organization
        .and_then(|o| o.pointer("/repositories"))
        .ok_or_else(|| GraphqlError::GraphQl("missing organization.repositories".to_string()))?;

    let total_repo_count = repos
        .get("totalCount")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let mut items = Vec::new();
    let mut repos_in_page = 0u32;
    let mut pending = Vec::new();
    let nodes = repos
        .get("nodes")
        .and_then(|n| n.as_array())
        .cloned()
        .unwrap_or_default();

    for repo_node in nodes {
        let repo_name = repo_node
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if repo_name.is_empty() {
            continue;
        }
        if repo_node.get("isArchived").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        if !repo_include_exclude(repo_name, org) {
            continue;
        }
        repos_in_page += 1;

        let pr_conn = repo_node.get("pullRequests");
        if let Some(pr_nodes) = pr_conn
            .and_then(|c| c.pointer("/nodes"))
            .and_then(|n| n.as_array())
        {
            for pr in pr_nodes {
                if let Some(item) = parse_pr_in_repo(pr, &org.name, repo_name) {
                    items.push(item);
                }
            }
            if pr_conn
                .and_then(|c| c.pointer("/pageInfo/hasNextPage"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                if let Some(cursor) = pr_conn
                    .and_then(|c| c.pointer("/pageInfo/endCursor"))
                    .and_then(|v| v.as_str())
                {
                    pending.push(PendingRepoPagination {
                        repo: repo_name.to_string(),
                        kind: RepoItemPageKind::PullRequests,
                        after: cursor.to_string(),
                    });
                }
            }
        }
        let issue_conn = repo_node.get("issues");
        if let Some(issue_nodes) = issue_conn
            .and_then(|c| c.pointer("/nodes"))
            .and_then(|n| n.as_array())
        {
            for issue in issue_nodes {
                if let Some(item) = parse_issue_in_repo(issue, &org.name, repo_name) {
                    items.push(item);
                }
            }
            if issue_conn
                .and_then(|c| c.pointer("/pageInfo/hasNextPage"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                if let Some(cursor) = issue_conn
                    .and_then(|c| c.pointer("/pageInfo/endCursor"))
                    .and_then(|v| v.as_str())
                {
                    pending.push(PendingRepoPagination {
                        repo: repo_name.to_string(),
                        kind: RepoItemPageKind::Issues,
                        after: cursor.to_string(),
                    });
                }
            }
        }
    }

    let has_next = repos
        .pointer("/pageInfo/hasNextPage")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let end_cursor = repos
        .pointer("/pageInfo/endCursor")
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok(OrgRepoPage {
        items,
        rate_limit,
        warning,
        repos_in_page,
        total_repo_count,
        has_next,
        end_cursor,
        organization_missing: false,
        pending,
    })
}

fn accumulate_rate_limit(total: &mut Option<RateLimitInfo>, part: Option<RateLimitInfo>) {
    match (total, part) {
        (None, Some(p)) => *total = Some(p),
        (Some(t), Some(p)) => {
            t.cost += p.cost;
            t.remaining = p.remaining;
            t.reset_at = p.reset_at;
        }
        _ => {}
    }
}

fn parse_rate_limit(rl: &serde_json::Value) -> RateLimitInfo {
    RateLimitInfo {
        cost: rl.get("cost").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        remaining: rl.get("remaining").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        reset_at: rl
            .get("resetAt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

fn repo_include_exclude(repo_name: &str, org: &OrgConfig) -> bool {
    if !org.include.is_empty() && !org.include.contains(&repo_name.to_string()) {
        return false;
    }
    if org.exclude.contains(&repo_name.to_string()) {
        return false;
    }
    true
}

async fn read_graphql_response(response: Response) -> Result<(u16, HeaderMap, String), GraphqlError> {
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let text = response
        .text()
        .await
        .map_err(|e| GraphqlError::Network(e.to_string()))?;
    Ok((status, headers, text))
}

async fn post_graphql(
    client: &reqwest::Client,
    token: &str,
    query: &str,
    variables: serde_json::Value,
) -> Result<serde_json::Value, GraphqlError> {
    let body = serde_json::json!({ "query": query, "variables": variables });
    let response = client
        .post("https://api.github.com/graphql")
        .bearer_auth(token)
        .header("User-Agent", "agent-dashboard")
        .json(&body)
        .send()
        .await
        .map_err(|e| GraphqlError::Network(e.to_string()))?;

    let (status, headers, text) = read_graphql_response(response).await?;

    if status == 401 {
        return Err(GraphqlError::Unauthorized);
    }
    if status == 403 || status == 429 {
        return Err(classify_http_error(status, &headers, &text));
    }
    if status >= 500 && status < 600 {
        return Err(GraphqlError::Network(format!(
            "GitHub GraphQL HTTP {status}: {}",
            truncate_body(&text)
        )));
    }
    if !(200..300).contains(&status) {
        return Err(GraphqlError::Forbidden {
            status,
            message: truncate_body(&text),
        });
    }

    serde_json::from_str(&text).map_err(|e| GraphqlError::GraphQl(e.to_string()))
}

async fn fetch_repo_item_page(
    client: &reqwest::Client,
    token: &str,
    org: &OrgConfig,
    pending: &PendingRepoPagination,
) -> Result<(Vec<RawItem>, Option<RateLimitInfo>, Vec<PendingRepoPagination>), GraphqlError> {
    let (query, variables) = match pending.kind {
        RepoItemPageKind::PullRequests => (
            REPO_PR_PAGE_QUERY,
            serde_json::json!({
                "owner": org.name,
                "name": pending.repo,
                "after": pending.after,
            }),
        ),
        RepoItemPageKind::Issues => (
            REPO_ISSUE_PAGE_QUERY,
            serde_json::json!({
                "owner": org.name,
                "name": pending.repo,
                "after": pending.after,
            }),
        ),
    };
    let json = post_graphql(client, token, query, variables).await?;
    let rate_limit = json.pointer("/data/rateLimit").map(parse_rate_limit);
    let warning = json.get("errors").map(|e| e.to_string());
    let mut items = Vec::new();
    let mut more = Vec::new();
    let conn_path = match pending.kind {
        RepoItemPageKind::PullRequests => "/data/repository/pullRequests",
        RepoItemPageKind::Issues => "/data/repository/issues",
    };
    let conn = json.pointer(conn_path).ok_or_else(|| {
        GraphqlError::GraphQl(format!("missing repository connection for {}", pending.repo))
    })?;
    if let Some(nodes) = conn.pointer("/nodes").and_then(|n| n.as_array()) {
        for node in nodes {
            let item = match pending.kind {
                RepoItemPageKind::PullRequests => parse_pr_in_repo(node, &org.name, &pending.repo),
                RepoItemPageKind::Issues => parse_issue_in_repo(node, &org.name, &pending.repo),
            };
            if let Some(item) = item {
                items.push(item);
            }
        }
    }
    if conn
        .pointer("/pageInfo/hasNextPage")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if let Some(cursor) = conn
            .pointer("/pageInfo/endCursor")
            .and_then(|v| v.as_str())
        {
            more.push(PendingRepoPagination {
                repo: pending.repo.clone(),
                kind: pending.kind.clone(),
                after: cursor.to_string(),
            });
        }
    }
    if warning.is_some() {
        log::debug!("github graphql repo page warning: {:?}", warning);
    }
    Ok((items, rate_limit, more))
}

fn pr_needs_failure_contexts(item: &RawItem) -> bool {
    item.kind == ItemKind::Pr
        && matches!(
            item.ci.as_ref().and_then(|c| c.state.as_deref()),
            Some("FAILURE") | Some("ERROR")
        )
}

async fn enrich_failure_contexts(
    client: &reqwest::Client,
    token: &str,
    org: &str,
    items: &mut [RawItem],
    total_rate: &mut Option<RateLimitInfo>,
) -> Result<(), GraphqlError> {
    for item in items.iter_mut() {
        if !pr_needs_failure_contexts(item) {
            continue;
        }
        let variables = serde_json::json!({
            "owner": org,
            "name": item.repo,
            "number": item.number,
        });
        let json = post_graphql(client, token, PR_FAILURE_CONTEXTS_QUERY, variables).await?;
        accumulate_rate_limit(
            total_rate,
            json.pointer("/data/rateLimit").map(parse_rate_limit),
        );
        if let Some(errors) = json.get("errors") {
            log::debug!(
                "failure contexts skipped for {}/{}#{}: {}",
                org,
                item.repo,
                item.number,
                errors
            );
            continue;
        }
        let rollup = json.pointer(
            "/data/repository/pullRequest/commits/nodes/0/commit/statusCheckRollup",
        );
        if let Some(rollup) = rollup {
            if let Some(ci) = parse_rollup(rollup) {
                item.ci = Some(ci);
            }
        }
    }
    Ok(())
}

pub async fn fetch_org_open_items(
    client: &reqwest::Client,
    token: &str,
    org: &OrgConfig,
) -> Result<FetchResult, GraphqlError> {
    log::debug!(
        "github graphql strategy=organization.repositories org={} include={} exclude={}",
        org.name,
        org.include.len(),
        org.exclude.len()
    );

    let mut after: Option<String> = None;
    let mut all_items = Vec::new();
    let mut total_rate: Option<RateLimitInfo> = None;
    let mut warning: Option<String> = None;
    let mut repos_visible = 0u32;
    let mut total_repos: Option<u32> = None;
    let mut organization_missing = false;
    let mut pending_pages: Vec<PendingRepoPagination> = Vec::new();

    loop {
        let variables = serde_json::json!({
            "org": org.name,
            "repoAfter": after,
        });
        log::debug!(
            "github graphql org={} repo_after={}",
            org.name,
            after.as_deref().unwrap_or("null")
        );

        let json = post_graphql(client, token, ORG_OPEN_QUERY, variables).await?;

        let page = ingest_org_repositories_response(&json, org)?;
        if page.organization_missing {
            organization_missing = true;
            break;
        }
        all_items.extend(page.items);
        pending_pages.extend(page.pending);
        repos_visible += page.repos_in_page;
        if page.total_repo_count.is_some() {
            total_repos = page.total_repo_count;
        }
        accumulate_rate_limit(&mut total_rate, page.rate_limit);
        if page.warning.is_some() {
            warning = page.warning;
        }

        if !page.has_next {
            break;
        }
        after = page.end_cursor;
        if after.is_none() {
            break;
        }
    }

    while let Some(pending) = pending_pages.pop() {
        let (items, rate, more) = fetch_repo_item_page(client, token, org, &pending).await?;
        all_items.extend(items);
        pending_pages.extend(more);
        accumulate_rate_limit(&mut total_rate, rate);
    }

    enrich_failure_contexts(client, token, &org.name, &mut all_items, &mut total_rate).await?;

    if let Some(rl) = &total_rate {
        log::info!(
            "github graphql org={} poll_cost={} remaining={}",
            org.name,
            rl.cost,
            rl.remaining
        );
    }

    let last_rate = total_rate;

    if organization_missing {
        return Err(GraphqlError::GraphQl(format!(
            "Organization \"{}\" was not found or this token cannot read it. Check resource owner, org approval, and repository access on the fine-grained PAT.",
            org.name
        )));
    }

    let empty_hint = if all_items.is_empty() {
        if let Some(total) = total_repos {
            if total == 0 {
                Some(format!(
                    "No repositories visible for org \"{}\". Check PAT repository access.",
                    org.name
                ))
            } else if repos_visible == 0 {
                Some(format!(
                    "Repositories exist for org \"{}\" but none matched include/exclude filters or all are archived.",
                    org.name
                ))
            } else {
                Some(format!(
                    "No open issues or pull requests in the {} visible repositories for org \"{}\".",
                    repos_visible,
                    org.name
                ))
            }
        } else {
            Some(format!(
                "No open items returned for org \"{}\".",
                org.name
            ))
        }
    } else {
        None
    };

    if let Some(w) = &warning {
        log::debug!("github graphql warning (no token): {}", w);
    }

    Ok(FetchResult {
        items: all_items,
        rate_limit: last_rate,
        warning,
        empty_hint,
        repos_visible,
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
    let mut empty_hint = None;
    let mut repos_visible = 0u32;

    for org in &config.orgs {
        let result = fetch_org_open_items(client, token, org).await?;
        combined.extend(result.items);
        accumulate_rate_limit(&mut rate_limit, result.rate_limit);
        repos_visible += result.repos_visible;
        if result.warning.is_some() {
            warning = result.warning;
        }
        if result.empty_hint.is_some() && combined.is_empty() {
            empty_hint = result.empty_hint;
        }
    }

    Ok(FetchResult {
        items: combined,
        rate_limit,
        warning,
        empty_hint,
        repos_visible,
    })
}

fn item_matches_org_filter(item: &RawItem, org: &OrgConfig) -> bool {
    repo_include_exclude(&item.repo, org)
}

fn parse_pr_in_repo(node: &serde_json::Value, org: &str, repo: &str) -> Option<RawItem> {
    let mut wrapped = node.clone();
    if let Some(obj) = wrapped.as_object_mut() {
        obj.insert(
            "__typename".to_string(),
            serde_json::Value::String("PullRequest".to_string()),
        );
        obj.insert(
            "repository".to_string(),
            serde_json::json!({
                "name": repo,
                "nameWithOwner": format!("{}/{}", org, repo),
            }),
        );
    }
    parse_node(&wrapped)
}

fn parse_issue_in_repo(node: &serde_json::Value, org: &str, repo: &str) -> Option<RawItem> {
    let mut wrapped = node.clone();
    if let Some(obj) = wrapped.as_object_mut() {
        obj.insert(
            "__typename".to_string(),
            serde_json::Value::String("Issue".to_string()),
        );
        obj.insert(
            "repository".to_string(),
            serde_json::json!({
                "name": repo,
                "nameWithOwner": format!("{}/{}", org, repo),
            }),
        );
    }
    parse_node(&wrapped)
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
    fn legacy_search_query_includes_archived_false() {
        let org = OrgConfig {
            name: "springfield-ham-radio".to_string(),
            include: vec![],
            exclude: vec![],
        };
        assert_eq!(
            legacy_search_query_string(&org),
            "org:springfield-ham-radio is:open archived:false"
        );
    }

    #[test]
    fn search_fixture_errors_with_issue_count_is_suspicious() {
        let json: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/graphql/search_errors_empty_nodes.json"
        ))
        .expect("json");
        let org = OrgConfig {
            name: "springfield-ham-radio".to_string(),
            include: vec![],
            exclude: vec![],
        };
        let page = ingest_search_response(&json, &org).expect("partial search");
        assert_eq!(page.items.len(), 0);
        assert_eq!(page.issue_count, Some(42));
        let hint = suspicious_search_result(&json, page.items.len());
        assert!(hint.is_some());
    }

    #[test]
    fn http_502_is_not_forbidden() {
        let err = {
            let status = 502u16;
            if status >= 500 && status < 600 {
                GraphqlError::Network("x".to_string())
            } else {
                GraphqlError::Forbidden {
                    status,
                    message: "x".to_string(),
                }
            }
        };
        assert!(matches!(err, GraphqlError::Network(_)));
    }

    #[test]
    fn accumulate_rate_limit_sums_cost() {
        let mut total = None;
        accumulate_rate_limit(
            &mut total,
            Some(RateLimitInfo {
                cost: 4,
                remaining: 100,
                reset_at: "t".to_string(),
            }),
        );
        accumulate_rate_limit(
            &mut total,
            Some(RateLimitInfo {
                cost: 1,
                remaining: 99,
                reset_at: "t".to_string(),
            }),
        );
        assert_eq!(total.as_ref().map(|t| t.cost), Some(5));
    }

    #[test]
    fn org_fixture_yields_pr_and_issue() {
        let json: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/graphql/org_repositories_sample.json"
        ))
        .expect("json");
        let org = OrgConfig {
            name: "springfield-ham-radio".to_string(),
            include: vec![],
            exclude: vec![],
        };
        let page = ingest_org_repositories_response(&json, &org).expect("org page");
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.repos_in_page, 2);
        assert!(!page.organization_missing);
    }

    #[test]
    fn org_include_filter_excludes_repo() {
        let json: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/graphql/org_repositories_sample.json"
        ))
        .expect("json");
        let org = OrgConfig {
            name: "springfield-ham-radio".to_string(),
            include: vec!["ham-radio-ui".to_string()],
            exclude: vec![],
        };
        let page = ingest_org_repositories_response(&json, &org).expect("org page");
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.repos_in_page, 1);
    }

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

    #[test]
    fn failure_rollup_without_check_names_still_parses() {
        let node: serde_json::Value = serde_json::json!({
            "__typename": "PullRequest",
            "number": 1,
            "title": "fail",
            "url": "https://github.com/o/r/pull/1",
            "updatedAt": "2026-10-10T12:00:00Z",
            "isDraft": false,
            "author": { "login": "human" },
            "labels": { "nodes": [] },
            "repository": { "name": "r", "nameWithOwner": "springfield-ham-radio/r" },
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "FAILURE", "contexts": { "nodes": [] } } } }] }
        });
        let item = parse_node(&node).expect("item");
        let ci = item.ci.expect("ci");
        assert_eq!(ci.state.as_deref(), Some("FAILURE"));
        assert!(ci.failed_checks.is_empty());
    }

    #[test]
    fn paginates_search_results() {
        let org = OrgConfig {
            name: "springfield-ham-radio".to_string(),
            include: vec![],
            exclude: vec![],
        };
        let page1: serde_json::Value = serde_json::json!({
            "data": {
                "rateLimit": { "cost": 1, "remaining": 4999, "resetAt": "2026-10-10T13:00:00Z" },
                "search": {
                    "issueCount": 2,
                    "pageInfo": { "hasNextPage": true, "endCursor": "cursor-2" },
                    "nodes": [{
                        "__typename": "PullRequest",
                        "number": 1,
                        "title": "one",
                        "url": "https://github.com/o/r/pull/1",
                        "updatedAt": "2026-10-10T12:00:00Z",
                        "isDraft": false,
                        "author": { "login": "human" },
                        "labels": { "nodes": [] },
                        "repository": { "name": "r", "nameWithOwner": "springfield-ham-radio/r" },
                        "commits": { "nodes": [] }
                    }]
                }
            }
        });
        let page2: serde_json::Value = serde_json::json!({
            "data": {
                "rateLimit": { "cost": 1, "remaining": 4998, "resetAt": "2026-10-10T13:00:00Z" },
                "search": {
                    "issueCount": 2,
                    "pageInfo": { "hasNextPage": false, "endCursor": null },
                    "nodes": [{
                        "__typename": "Issue",
                        "number": 2,
                        "title": "two",
                        "url": "https://github.com/o/r/issues/2",
                        "updatedAt": "2026-10-10T12:00:00Z",
                        "author": { "login": "human" },
                        "labels": { "nodes": [] },
                        "repository": { "name": "r", "nameWithOwner": "springfield-ham-radio/r" }
                    }]
                }
            }
        });

        let p1 = ingest_search_response(&page1, &org).expect("page1");
        assert!(p1.has_next);
        assert_eq!(p1.end_cursor.as_deref(), Some("cursor-2"));
        assert_eq!(p1.items.len(), 1);

        let p2 = ingest_search_response(&page2, &org).expect("page2");
        assert!(!p2.has_next);
        assert_eq!(p2.items.len(), 1);

        let mut all = p1.items;
        all.extend(p2.items);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn classifies_rate_limit_403() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        let err = classify_http_error(403, &headers, "secondary rate limit");
        assert!(matches!(err, GraphqlError::RateLimited { .. }));
    }

    #[test]
    fn classifies_permission_403() {
        let headers = HeaderMap::new();
        let err = classify_http_error(403, &headers, "Resource not accessible by integration");
        assert!(matches!(err, GraphqlError::Forbidden { .. }));
    }
}

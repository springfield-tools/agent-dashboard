use reqwest::header::{HeaderMap, RETRY_AFTER};
use reqwest::Response;

use std::collections::HashMap;

use crate::config::{AppConfig, OrgConfig};
use crate::credential::graphql_budget_key;
use crate::model::{CiSnapshot, GraphqlBudgetLine, ItemKind, OrgSectionNotice, RawItem};

#[path = "graphql_partial.rs"]
mod graphql_partial;

use graphql_partial::{
    build_org_notice, merge_parsed_errors, only_rollup_access_errors, parse_graphql_errors,
    rollup_forbidden_at, ParsedGraphqlErrors,
};

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

#[derive(Debug, Clone, thiserror::Error)]
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

#[derive(Clone)]
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
    pub section_notice: Option<OrgSectionNotice>,
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
    pub parsed_errors: ParsedGraphqlErrors,
    pub repos_in_page: u32,
    pub total_repo_count: Option<u32>,
    pub has_next: bool,
    pub end_cursor: Option<String>,
    pub organization_missing: bool,
    pub pending: Vec<PendingRepoPagination>,
}

fn graphql_partial_warning(errors: &serde_json::Value) -> Option<String> {
    if only_rollup_access_errors(errors) {
        return None;
    }
    Some("GitHub returned partial GraphQL errors.".to_string())
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
    let parsed_errors = json
        .get("errors")
        .map(parse_graphql_errors)
        .unwrap_or_default();
    let warning = json
        .get("errors")
        .and_then(|errors| graphql_partial_warning(errors));

    let organization_missing = match json.pointer("/data/organization") {
        None => true,
        Some(v) => v.is_null(),
    };
    if organization_missing {
        return Ok(OrgRepoPage {
            items: vec![],
            rate_limit,
            warning,
            parsed_errors,
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

    for (repo_index, repo_node) in nodes.iter().enumerate() {
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
            for (pr_index, pr) in pr_nodes.iter().enumerate() {
                let rollup_forbidden =
                    rollup_forbidden_at(&parsed_errors, repo_index, pr_index);
                if let Some(item) =
                    parse_pr_in_repo(pr, &org.name, repo_name, rollup_forbidden)
                {
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
        parsed_errors,
        repos_in_page,
        total_repo_count,
        has_next,
        end_cursor,
        organization_missing: false,
        pending,
    })
}

fn accumulate_rate_limit(total: &mut Option<RateLimitInfo>, part: Option<RateLimitInfo>) {
    if let Some(p) = part {
        match total {
            None => *total = Some(p),
            Some(t) => {
                t.cost += p.cost;
                t.remaining = p.remaining;
                t.reset_at = p.reset_at;
            }
        }
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
) -> Result<
    (
        Vec<RawItem>,
        Option<RateLimitInfo>,
        Vec<PendingRepoPagination>,
        ParsedGraphqlErrors,
    ),
    GraphqlError,
> {
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
    let parsed_errors = json
        .get("errors")
        .map(parse_graphql_errors)
        .unwrap_or_default();
    let warning = json
        .get("errors")
        .and_then(|errors| graphql_partial_warning(errors));
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
        for (pr_index, node) in nodes.iter().enumerate() {
            let item = match pending.kind {
                RepoItemPageKind::PullRequests => {
                    let rollup_forbidden =
                        rollup_forbidden_at(&parsed_errors, 0, pr_index);
                    parse_pr_in_repo(node, &org.name, &pending.repo, rollup_forbidden)
                }
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
    if warning.is_some() || !parsed_errors.rollup_forbidden_paths.is_empty() {
        log::debug!(
            "github graphql repo page warning={:?} rollup_forbidden={}",
            warning,
            parsed_errors.rollup_forbidden_paths.len()
        );
    }
    Ok((items, rate_limit, more, parsed_errors))
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
    let mut parsed_errors = ParsedGraphqlErrors::default();

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
        merge_parsed_errors(&mut parsed_errors, page.parsed_errors);
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
        let (items, rate, more, page_errors) =
            fetch_repo_item_page(client, token, org, &pending).await?;
        all_items.extend(items);
        pending_pages.extend(more);
        accumulate_rate_limit(&mut total_rate, rate);
        merge_parsed_errors(&mut parsed_errors, page_errors);
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

    let section_notice = build_org_notice(&org.name, &parsed_errors);

    Ok(FetchResult {
        items: all_items,
        rate_limit: last_rate,
        warning,
        empty_hint,
        section_notice,
    })
}

pub struct MultiOrgFetchResult {
    pub items: Vec<RawItem>,
    pub rate_limit: Option<RateLimitInfo>,
    pub budgets: Vec<GraphqlBudgetLine>,
    pub org_errors: HashMap<String, String>,
    pub org_graphql_errors: HashMap<String, GraphqlError>,
    pub org_section_notices: HashMap<String, OrgSectionNotice>,
    pub warning: Option<String>,
    pub empty_hint: Option<String>,
}

fn merge_budget_line(
    budgets: &mut HashMap<String, GraphqlBudgetLine>,
    key: &str,
    rate: Option<RateLimitInfo>,
) {
    if let Some(rl) = rate {
        budgets
            .entry(key.to_string())
            .and_modify(|b| {
                b.poll_cost += rl.cost;
                b.remaining = b.remaining.min(rl.remaining);
            })
            .or_insert(GraphqlBudgetLine {
                credential_key: key.to_string(),
                poll_cost: rl.cost,
                remaining: rl.remaining,
            });
    }
}

pub async fn fetch_all_orgs_multi(
    client: &reqwest::Client,
    config: &AppConfig,
    token_for_org: impl Fn(&OrgConfig) -> Result<String, String>,
) -> MultiOrgFetchResult {
    let mut combined = Vec::new();
    let mut rate_limit = None;
    let mut budget_map: HashMap<String, GraphqlBudgetLine> = HashMap::new();
    let mut org_errors = HashMap::new();
    let mut org_graphql_errors = HashMap::new();
    let mut org_section_notices = HashMap::new();
    let mut warning = None;
    let mut empty_hint = None;

    for org in &config.orgs {
        let token = match token_for_org(org) {
            Ok(t) => t,
            Err(message) => {
                org_errors.insert(org.name.clone(), message);
                continue;
            }
        };
        let budget_key = graphql_budget_key(org, config);
        match fetch_org_open_items(client, &token, org).await {
            Ok(result) => {
                combined.extend(result.items);
                accumulate_rate_limit(&mut rate_limit, result.rate_limit.clone());
                merge_budget_line(&mut budget_map, &budget_key, result.rate_limit);
                if result.warning.is_some() {
                    warning = result.warning;
                }
                if result.empty_hint.is_some() && combined.is_empty() {
                    empty_hint = result.empty_hint;
                }
                if let Some(notice) = result.section_notice {
                    org_section_notices.insert(org.name.clone(), notice);
                }
            }
            Err(e) => {
                let message = org_fetch_error_message(org, &e);
                org_errors.insert(org.name.clone(), message);
                org_graphql_errors.insert(org.name.clone(), e.clone());
                log::warn!("org {} fetch failed: {}", org.name, e);
            }
        }
    }

    let budgets: Vec<GraphqlBudgetLine> = budget_map.values().cloned().collect();

    if !budget_map.is_empty() {
        let reset_at = rate_limit
            .as_ref()
            .map(|r| r.reset_at.clone())
            .unwrap_or_default();
        rate_limit = Some(RateLimitInfo {
            cost: budget_map.values().map(|b| b.poll_cost).sum(),
            remaining: budget_map.values().map(|b| b.remaining).min().unwrap_or(0),
            reset_at,
        });
    }

    MultiOrgFetchResult {
        items: combined,
        rate_limit,
        budgets,
        org_errors,
        org_graphql_errors,
        org_section_notices,
        warning,
        empty_hint,
    }
}

fn org_fetch_error_message(org: &OrgConfig, e: &GraphqlError) -> String {
    match e {
        GraphqlError::Unauthorized => format!(
            "GitHub rejected the token for \"{}\" (401). Update the PAT in Settings or run `gh auth login`.",
            org.name
        ),
        GraphqlError::Forbidden { status, .. } => format!(
            "GitHub denied access to \"{}\" ({status}). Check PAT permissions and org approval.",
            org.name
        ),
        GraphqlError::RateLimited { message, .. } => format!(
            "Rate limited while fetching \"{}\": {message}",
            org.name
        ),
        GraphqlError::Network(msg) => format!("Network error for \"{}\": {msg}", org.name),
        GraphqlError::GraphQl(msg) => msg.clone(),
    }
}

fn parse_pr_in_repo(
    node: &serde_json::Value,
    org: &str,
    repo: &str,
    rollup_forbidden: bool,
) -> Option<RawItem> {
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
    parse_node(&wrapped, rollup_forbidden)
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
    parse_node(&wrapped, false)
}

fn parse_node(node: &serde_json::Value, rollup_forbidden: bool) -> Option<RawItem> {
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
            let (ci, ci_unknown) = if rollup_forbidden {
                (None, true)
            } else {
                let rollup = node.pointer("/commits/nodes/0/commit/statusCheckRollup");
                (rollup.and_then(|r| parse_rollup(r)), false)
            };
            return Some(RawItem {
                kind: ItemKind::Pr,
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
                ci_unknown,
            });
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
        ci_unknown: false,
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
    fn org_partial_rollup_errors_keep_data_and_mark_ci_unknown() {
        let json: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/graphql/org_repositories_partial_rollup_errors.json"
        ))
        .expect("json");
        let org = OrgConfig {
            name: "springfield-tools".to_string(),
            credential: None,
            include: vec![],
            exclude: vec![],
        };
        let page = ingest_org_repositories_response(&json, &org).expect("org page");
        assert_eq!(page.items.len(), 2);
        assert!(page.warning.is_none());
        let denied = page
            .items
            .iter()
            .find(|i| i.number == 11)
            .expect("pr 11");
        assert!(denied.ci_unknown);
        assert!(denied.ci.is_none());
        let ok = page.items.iter().find(|i| i.number == 10).expect("pr 10");
        assert!(!ok.ci_unknown);
        assert_eq!(ok.ci.as_ref().and_then(|c| c.state.as_deref()), Some("SUCCESS"));
        let notice =
            build_org_notice(&org.name, &page.parsed_errors).expect("notice");
        assert!(notice.summary.contains("springfield-tools"));
        assert!(notice.summary.contains("1 PR"));
        assert_eq!(notice.details.len(), 1);
    }

    #[test]
    fn org_fixture_yields_pr_and_issue() {
        let json: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/graphql/org_repositories_sample.json"
        ))
        .expect("json");
        let org = OrgConfig {
            name: "springfield-ham-radio".to_string(),
            credential: None,
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
            credential: None,
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
        let item = parse_node(&node, false).expect("item");
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
        let item = parse_node(&node, false).expect("item");
        let ci = item.ci.expect("ci");
        assert_eq!(ci.state.as_deref(), Some("FAILURE"));
        assert!(ci.failed_checks.is_empty());
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

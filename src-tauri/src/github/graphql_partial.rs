use std::collections::HashMap;

use crate::model::OrgSectionNotice;

#[derive(Debug, Clone, Default)]
pub struct ParsedGraphqlErrors {
    /// `(repository nodes index, pullRequests nodes index)` on an org page, or `(0, pr_index)` on a single-repo page.
    pub rollup_forbidden_paths: Vec<(usize, usize)>,
    pub message_counts: HashMap<String, u32>,
}

pub fn is_rollup_access_error(entry: &serde_json::Value) -> bool {
    let message = entry
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let err_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if err_type != "FORBIDDEN"
        && !message.contains("Resource not accessible by personal access token")
    {
        return false;
    }
    let path = entry.get("path").and_then(|v| v.as_array());
    path.map(|segments| {
        segments
            .iter()
            .filter_map(|v| v.as_str())
            .any(|s| s == "statusCheckRollup" || s == "commits" || s == "commit")
    })
    .unwrap_or(false)
}

pub fn only_rollup_access_errors(errors: &serde_json::Value) -> bool {
    errors
        .as_array()
        .map(|arr| !arr.is_empty() && arr.iter().all(is_rollup_access_error))
        .unwrap_or(false)
}

pub fn extract_repo_pr_indices_from_path(path: &[serde_json::Value]) -> Option<(usize, usize)> {
    let mut repo_idx: Option<usize> = None;
    let mut pr_idx: Option<usize> = None;
    let mut i = 0;
    while i + 2 < path.len() {
        if path[i].as_str() == Some("repositories")
            && path[i + 1].as_str() == Some("nodes")
            && let Some(n) = path[i + 2].as_u64()
        {
            repo_idx = Some(n as usize);
        }
        if path[i].as_str() == Some("repository") {
            repo_idx = Some(0);
        }
        if path[i].as_str() == Some("pullRequests")
            && path[i + 1].as_str() == Some("nodes")
            && let Some(n) = path[i + 2].as_u64()
        {
            pr_idx = Some(n as usize);
        }
        i += 1;
    }
    match (repo_idx, pr_idx) {
        (Some(r), Some(p)) => Some((r, p)),
        _ => None,
    }
}

pub fn parse_graphql_errors(errors: &serde_json::Value) -> ParsedGraphqlErrors {
    let mut out = ParsedGraphqlErrors::default();
    let Some(arr) = errors.as_array() else {
        return out;
    };
    for entry in arr {
        let message = entry
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("GraphQL error")
            .to_string();
        if is_rollup_access_error(entry) {
            if let Some(path) = entry.get("path").and_then(|v| v.as_array()) {
                if let Some(pair) = extract_repo_pr_indices_from_path(path) {
                    out.rollup_forbidden_paths.push(pair);
                }
            }
            *out.message_counts.entry(message).or_insert(0) += 1;
        } else if entry.get("type").and_then(|v| v.as_str()) == Some("RATE_LIMITED") {
            // handled elsewhere
        } else {
            *out.message_counts.entry(message).or_insert(0) += 1;
        }
    }
    out
}

pub fn rollup_forbidden_at(parsed: &ParsedGraphqlErrors, repo_index: usize, pr_index: usize) -> bool {
    parsed
        .rollup_forbidden_paths
        .iter()
        .any(|(r, p)| *r == repo_index && *p == pr_index)
}

pub fn rollup_forbidden_count(parsed: &ParsedGraphqlErrors) -> u32 {
    if !parsed.rollup_forbidden_paths.is_empty() {
        parsed.rollup_forbidden_paths.len() as u32
    } else {
        parsed
            .message_counts
            .keys()
            .filter(|m| m.contains("Resource not accessible by personal access token"))
            .map(|m| parsed.message_counts.get(m).copied().unwrap_or(0))
            .sum()
    }
}

pub fn build_org_notice(org_name: &str, parsed: &ParsedGraphqlErrors) -> Option<OrgSectionNotice> {
    let n = rollup_forbidden_count(parsed);
    if n == 0 && parsed.message_counts.is_empty() {
        return None;
    }
    let summary = if n > 0 {
        let pr_word = if n == 1 { "PR" } else { "PRs" };
        format!(
            "{org_name}: token can't read CI status for {n} {pr_word} — add Commit statuses (Read) or Contents (Read) to this token"
        )
    } else {
        format!("{org_name}: GitHub returned partial errors for this org")
    };
    let mut details: Vec<String> = parsed
        .message_counts
        .iter()
        .map(|(msg, count)| format!("{msg} ({count})"))
        .collect();
    details.sort();
    Some(OrgSectionNotice { summary, details })
}

pub fn merge_parsed_errors(into: &mut ParsedGraphqlErrors, more: ParsedGraphqlErrors) {
    into.rollup_forbidden_paths.extend(more.rollup_forbidden_paths);
    for (msg, count) in more.message_counts {
        *into.message_counts.entry(msg).or_insert(0) += count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rollup_path_indices() {
        let path = serde_json::json!([
            "organization",
            "repositories",
            "nodes",
            1,
            "pullRequests",
            "nodes",
            3,
            "commits",
            "nodes",
            0,
            "commit",
            "statusCheckRollup"
        ]);
        let indices = extract_repo_pr_indices_from_path(path.as_array().unwrap());
        assert_eq!(indices, Some((1, 3)));
    }
}

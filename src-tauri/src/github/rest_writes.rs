use reqwest::Client;
use serde::Deserialize;

use crate::row_actions::COMMENT_FOOTER;

const USER_AGENT: &str = "agent-dashboard";

#[derive(Debug, thiserror::Error)]
pub enum RestWriteError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden: {message}")]
    Forbidden { message: String },
    #[error("network: {0}")]
    Network(String),
    #[error("github: {status} {message}")]
    GitHub { status: u16, message: String },
}

pub fn permission_banner() -> String {
    "GitHub returned 403. Row actions need Issues and Pull requests (Read and write) on your token. \
For a fine-grained PAT, enable both under Repository permissions for your org (e.g. springfield-ham-radio). \
Contents can stay off. Or set credential = \"gh\" in config and run gh auth login. \
The poller can stay read-only; only confirmed row actions use write scopes."
        .to_string()
}

fn auth_headers(token: &str) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {token}").parse().expect("auth header"),
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        USER_AGENT.parse().expect("ua"),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        "application/vnd.github+json".parse().expect("accept"),
    );
    headers
}

fn classify(status: u16, body: &str) -> RestWriteError {
    if status == 401 {
        return RestWriteError::Unauthorized;
    }
    if status == 403 {
        return RestWriteError::Forbidden {
            message: permission_banner(),
        };
    }
    RestWriteError::GitHub {
        status,
        message: body.chars().take(500).collect(),
    }
}

#[derive(Debug, Deserialize)]
struct CommentResponse {
    id: u64,
}

pub async fn post_issue_comment(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    body: &str,
) -> Result<u64, RestWriteError> {
    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/issues/{number}/comments"
    );
    let res = client
        .post(&url)
        .headers(auth_headers(token))
        .json(&serde_json::json!({ "body": body }))
        .send()
        .await
        .map_err(|e| RestWriteError::Network(e.to_string()))?;
    let status = res.status().as_u16();
    let text = res.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(classify(status, &text));
    }
    let parsed: CommentResponse = serde_json::from_str(&text)
        .map_err(|e| RestWriteError::Network(format!("parse comment response: {e}")))?;
    Ok(parsed.id)
}

pub async fn delete_issue_comment(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    comment_id: u64,
) -> Result<(), RestWriteError> {
    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/issues/comments/{comment_id}"
    );
    let res = client
        .delete(&url)
        .headers(auth_headers(token))
        .send()
        .await
        .map_err(|e| RestWriteError::Network(e.to_string()))?;
    let status = res.status().as_u16();
    if status == 404 {
        return Ok(());
    }
    if !(200..300).contains(&status) {
        let text = res.text().await.unwrap_or_default();
        return Err(classify(status, &text));
    }
    Ok(())
}

pub async fn add_labels(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    names: &[String],
) -> Result<(), RestWriteError> {
    if names.is_empty() {
        return Ok(());
    }
    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/issues/{number}/labels"
    );
    let res = client
        .post(&url)
        .headers(auth_headers(token))
        .json(names)
        .send()
        .await
        .map_err(|e| RestWriteError::Network(e.to_string()))?;
    let status = res.status().as_u16();
    if !(200..300).contains(&status) {
        let text = res.text().await.unwrap_or_default();
        return Err(classify(status, &text));
    }
    Ok(())
}

pub async fn remove_label(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    name: &str,
) -> Result<(), RestWriteError> {
    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/issues/{number}/labels/{name}"
    );
    let res = client
        .delete(&url)
        .headers(auth_headers(token))
        .send()
        .await
        .map_err(|e| RestWriteError::Network(e.to_string()))?;
    let status = res.status().as_u16();
    if status == 404 {
        return Ok(());
    }
    if !(200..300).contains(&status) {
        let text = res.text().await.unwrap_or_default();
        return Err(classify(status, &text));
    }
    Ok(())
}

pub async fn apply_label_diff(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    to_add: &[String],
    to_remove: &[String],
) -> Result<(), RestWriteError> {
    for name in to_remove {
        remove_label(client, token, owner, repo, number, name).await?;
    }
    add_labels(client, token, owner, repo, number, to_add).await?;
    Ok(())
}

/// Undo restores labels to `previous`; only status-label mutations are reversed via diff from `current`.
pub async fn restore_labels(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    current: &[String],
    previous: &[String],
    status_names: &[String],
) -> Result<(), RestWriteError> {
    let mut to_remove = Vec::new();
    let mut to_add = Vec::new();
    for name in status_names {
        let had = previous.iter().any(|l| l == name);
        let has = current.iter().any(|l| l == name);
        if had && !has {
            to_add.push(name.clone());
        } else if !had && has {
            to_remove.push(name.clone());
        }
    }
    apply_label_diff(client, token, owner, repo, number, &to_add, &to_remove).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[test]
    fn footer_constant_matches_row_actions() {
        assert_eq!(COMMENT_FOOTER, "— Agent Dashboard");
    }

    #[tokio::test]
    async fn post_comment_then_labels() {
        let server = MockServer::start();
        let comment_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/repos/o/r/issues/7/comments");
            then.status(201)
                .json_body(serde_json::json!({ "id": 99 }));
        });
        let label_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/repos/o/r/issues/7/labels");
            then.status(200);
        });

        let base = server.base_url();
        let client = Client::builder().build().unwrap();

        async fn post_with_base(
            client: &Client,
            base: &str,
            token: &str,
            body: &str,
        ) -> Result<u64, RestWriteError> {
            let url = format!("{base}/repos/o/r/issues/7/comments");
            let res = client
                .post(&url)
                .headers(auth_headers(token))
                .json(&serde_json::json!({ "body": body }))
                .send()
                .await
                .map_err(|e| RestWriteError::Network(e.to_string()))?;
            let status = res.status().as_u16();
            let text = res.text().await.unwrap_or_default();
            if !(200..300).contains(&status) {
                return Err(classify(status, &text));
            }
            Ok(99)
        }

        let id = post_with_base(&client, &base, "tok", "hi").await.expect("comment");
        assert_eq!(id, 99);
        comment_mock.assert();

        let url = format!("{base}/repos/o/r/issues/7/labels");
        let res = client
            .post(&url)
            .headers(auth_headers("tok"))
            .json(&serde_json::json!(["ready-to-merge"]))
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success());
        label_mock.assert();
    }

    #[tokio::test]
    async fn forbidden_maps_to_permission_banner() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/repos/o/r/issues/1/comments");
            then.status(403).body("nope");
        });
        let client = Client::builder().build().unwrap();
        let url = format!("{}/repos/o/r/issues/1/comments", server.base_url());
        let res = client
            .post(&url)
            .headers(auth_headers("tok"))
            .json(&serde_json::json!({ "body": "x" }))
            .send()
            .await
            .unwrap();
        let err = classify(res.status().as_u16(), "nope");
        assert!(matches!(err, RestWriteError::Forbidden { .. }));
        assert!(permission_banner().contains("403"));
    }
}

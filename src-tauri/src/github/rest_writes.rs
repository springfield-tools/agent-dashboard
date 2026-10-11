use reqwest::Client;
use serde::Deserialize;

use crate::row_actions::COMMENT_FOOTER;

const USER_AGENT: &str = "agent-dashboard";
const GITHUB_API: &str = "https://api.github.com";

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

fn github_message_from_body(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        })
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
    let message = github_message_from_body(body)
        .map(|m| format!("GitHub error {status}: {m}"))
        .unwrap_or_else(|| format!("GitHub error {status}."));
    RestWriteError::GitHub { status, message }
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
    post_issue_comment_at(client, GITHUB_API, token, owner, repo, number, body).await
}

pub async fn post_issue_comment_at(
    client: &Client,
    api_base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    body: &str,
) -> Result<u64, RestWriteError> {
    let url = format!("{api_base}/repos/{owner}/{repo}/issues/{number}/comments");
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
    delete_issue_comment_at(client, GITHUB_API, token, owner, repo, comment_id).await
}

pub async fn delete_issue_comment_at(
    client: &Client,
    api_base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    comment_id: u64,
) -> Result<(), RestWriteError> {
    let url = format!("{api_base}/repos/{owner}/{repo}/issues/comments/{comment_id}");
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
    add_labels_at(client, GITHUB_API, token, owner, repo, number, names).await
}

pub async fn add_labels_at(
    client: &Client,
    api_base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    names: &[String],
) -> Result<(), RestWriteError> {
    if names.is_empty() {
        return Ok(());
    }
    let url = format!("{api_base}/repos/{owner}/{repo}/issues/{number}/labels");
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
    remove_label_at(client, GITHUB_API, token, owner, repo, number, name).await
}

pub async fn remove_label_at(
    client: &Client,
    api_base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    name: &str,
) -> Result<(), RestWriteError> {
    let url = format!("{api_base}/repos/{owner}/{repo}/issues/{number}/labels/{name}");
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
    apply_label_diff_at(client, GITHUB_API, token, owner, repo, number, to_add, to_remove).await
}

pub async fn apply_label_diff_at(
    client: &Client,
    api_base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    to_add: &[String],
    to_remove: &[String],
) -> Result<(), RestWriteError> {
    for name in to_remove {
        remove_label_at(client, api_base, token, owner, repo, number, name).await?;
    }
    add_labels_at(client, api_base, token, owner, repo, number, to_add).await?;
    Ok(())
}

/// Inverse of a successful write label diff (undo).
pub async fn apply_undo_label_diff_at(
    client: &Client,
    api_base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    number: u32,
    undo_labels_add: &[String],
    undo_labels_remove: &[String],
) -> Result<(), RestWriteError> {
    apply_label_diff_at(
        client,
        api_base,
        token,
        owner,
        repo,
        number,
        undo_labels_add,
        undo_labels_remove,
    )
    .await
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

    #[test]
    fn github_error_uses_message_field_not_raw_body() {
        let err = classify(422, r#"{"message":"Validation Failed","errors":[]}"#);
        match err {
            RestWriteError::GitHub { status, message } => {
                assert_eq!(status, 422);
                assert_eq!(message, "GitHub error 422: Validation Failed");
                assert!(!message.contains("errors"));
            }
            _ => panic!("expected GitHub error"),
        }
    }

    #[tokio::test]
    async fn label_remove_failure_returns_github_message() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(DELETE)
                .path("/repos/o/r/issues/3/labels/needs-testing");
            then.status(500)
                .json_body(serde_json::json!({ "message": "boom" }));
        });
        let client = Client::builder().build().unwrap();
        let url = format!(
            "{}/repos/o/r/issues/3/labels/needs-testing",
            server.base_url()
        );
        let res = client
            .delete(&url)
            .headers(auth_headers("tok"))
            .send()
            .await
            .unwrap();
        let err = classify(res.status().as_u16(), r#"{"message":"boom"}"#);
        match err {
            RestWriteError::GitHub { message, .. } => {
                assert_eq!(message, "GitHub error 500: boom");
            }
            _ => panic!("expected label failure"),
        }
    }

    #[tokio::test]
    async fn apply_label_diff_at_remove_then_add() {
        let server = MockServer::start();
        let base = server.base_url();
        let remove_mock = server.mock(|when, then| {
            when.method(DELETE)
                .path("/repos/o/r/issues/5/labels/needs-testing");
            then.status(200);
        });
        let add_mock = server.mock(|when, then| {
            when.method(POST).path("/repos/o/r/issues/5/labels");
            then.status(200);
        });
        let client = Client::builder().build().unwrap();
        let to_add = vec!["ready-to-merge".to_string()];
        let to_remove = vec!["needs-testing".to_string()];
        apply_label_diff_at(
            &client,
            &base,
            "tok",
            "o",
            "r",
            5,
            &to_add,
            &to_remove,
        )
        .await
        .expect("label diff");
        remove_mock.assert();
        add_mock.assert();
    }

    #[tokio::test]
    async fn testing_passed_write_then_undo_restores_labels_and_deletes_comment() {
        use crate::model::{CiSnapshot, DashboardRow, ItemKind};
        use crate::row_actions::{label_diff_for_action, RowActionKind};
        use crate::write_state::{make_undo_record, RowKey};

        let server = MockServer::start();
        let base = server.base_url();
        let client = Client::builder().build().unwrap();
        let token = "tok";
        let owner = "o";
        let repo = "r";
        let number = 5u32;

        let labels = crate::config::LabelsConfig::default();
        let row = DashboardRow {
            kind: ItemKind::Pr,
            draft: false,
            org: owner.to_string(),
            repo: repo.to_string(),
            number,
            title: "Test".to_string(),
            url: "https://example.com".to_string(),
            author: "human".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            labels: vec![
                "needs-testing".to_string(),
                "dependencies".to_string(),
            ],
            ci: Some(CiSnapshot {
                state: Some("SUCCESS".to_string()),
                failed_checks: vec![],
            }),
            chips: vec![],
        };
        let (write_add, write_remove) = label_diff_for_action(
            RowActionKind::TestingPassedReady,
            &row.labels,
            &labels,
        );
        assert_eq!(write_remove, vec!["needs-testing".to_string()]);
        assert_eq!(write_add, vec!["ready-to-merge".to_string()]);

        let body = "Testing passed. Ready to merge.\n\n— Agent Dashboard";

        // Write: remove needs-testing, add ready-to-merge (Testing passed).
        let write_remove_needs_testing = server.mock(|when, then| {
            when.method(DELETE)
                .path("/repos/o/r/issues/5/labels/needs-testing");
            then.status(200);
        });
        let write_add_ready = server.mock(|when, then| {
            when.method(POST).path("/repos/o/r/issues/5/labels");
            then.status(200);
        });
        let write_labels_to_add = vec!["ready-to-merge".to_string()];
        remove_label_at(
            &client,
            &base,
            token,
            owner,
            repo,
            number,
            "needs-testing",
        )
        .await
        .expect("write remove needs-testing");
        write_remove_needs_testing.assert();
        add_labels_at(
            &client,
            &base,
            token,
            owner,
            repo,
            number,
            &write_labels_to_add,
        )
        .await
        .expect("write add ready-to-merge");
        write_add_ready.assert();

        let comment_id = 42u64;
        let undo = make_undo_record(
            RowKey {
                org: owner.to_string(),
                repo: repo.to_string(),
                number,
            },
            &row,
            Some(comment_id),
            body,
            &write_add,
            &write_remove,
        );
        assert_eq!(undo.undo_labels_remove, vec!["ready-to-merge".to_string()]);
        assert_eq!(undo.undo_labels_add, vec!["needs-testing".to_string()]);

        // Undo: remove ready-to-merge, re-add needs-testing, then delete audit comment.
        let undo_remove_ready = server.mock(|when, then| {
            when.method(DELETE)
                .path("/repos/o/r/issues/5/labels/ready-to-merge");
            then.status(200);
        });
        let undo_add_needs_testing = server.mock(|when, then| {
            when.method(POST).path("/repos/o/r/issues/5/labels");
            then.status(200);
        });
        let undo_delete_comment = server.mock(|when, then| {
            when.method(DELETE)
                .path("/repos/o/r/issues/comments/42");
            then.status(204);
        });
        remove_label_at(
            &client,
            &base,
            token,
            owner,
            repo,
            number,
            "ready-to-merge",
        )
        .await
        .expect("undo remove ready-to-merge");
        undo_remove_ready.assert();
        let undo_labels_to_add = vec!["needs-testing".to_string()];
        add_labels_at(
            &client,
            &base,
            token,
            owner,
            repo,
            number,
            &undo_labels_to_add,
        )
        .await
        .expect("undo add needs-testing");
        undo_add_needs_testing.assert();

        delete_issue_comment_at(&client, &base, token, owner, repo, comment_id)
            .await
            .expect("undo delete comment");
        undo_delete_comment.assert();
    }

    #[tokio::test]
    async fn delete_comment_mock_returns_success() {
        let server = MockServer::start();
        let delete_mock = server.mock(|when, then| {
            when.method(DELETE)
                .path("/repos/o/r/issues/comments/55");
            then.status(204);
        });
        let client = Client::builder().build().unwrap();
        let url = format!("{}/repos/o/r/issues/comments/55", server.base_url());
        let res = client
            .delete(&url)
            .headers(auth_headers("tok"))
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success());
        delete_mock.assert();
    }
}

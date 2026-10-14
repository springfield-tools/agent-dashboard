use std::sync::Arc;

use reqwest::Client;
use tauri::{AppHandle, Emitter};

use crate::config::{org_name_is_enabled, AppConfig, OrgConfig};
use crate::credential::obtain_github_token_for_org;
use crate::github::rest_writes::{
    apply_label_diff, delete_issue_comment, post_issue_comment, RestWriteError,
};
use crate::model::DashboardRow;
use crate::row_actions::{
    actions_for_group, build_comment_body, label_diff_for_action, preview_action, RowActionKind,
};
use crate::state::AppState;
use crate::write_state::{
    apply_optimistic_labels, labels_after_diff, make_undo_record, merged_snapshot,
    RowKey,
};

fn find_row(snapshot: &crate::model::DashboardSnapshot, key: &RowKey) -> Option<DashboardRow> {
    for group in &snapshot.groups {
        for row in &group.items {
            if row.org == key.org && row.repo == key.repo && row.number == key.number {
                return Some(row.clone());
            }
        }
    }
    for section in &snapshot.org_sections {
        for group in &section.groups {
            for row in &group.items {
                if row.org == key.org && row.repo == key.repo && row.number == key.number {
                    return Some(row.clone());
                }
            }
        }
    }
    None
}

fn github_token_for_org(state: &AppState, org_name: &str) -> Result<String, String> {
    let config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    let org = config
        .orgs
        .iter()
        .find(|o| o.name == org_name)
        .cloned()
        .unwrap_or_else(|| OrgConfig {
            name: org_name.to_string(),
            enabled: true,
            credential: None,
            include: vec![],
            exclude: vec![],
        });
    obtain_github_token_for_org(&config, &org, state.poll_runtime.gh_token_cache(), false)
        .map_err(|opt| opt.unwrap_or_else(|| "GitHub token unavailable.".to_string()))
}

fn lock_dashboard(state: &AppState) -> Result<crate::model::DashboardSnapshot, String> {
    state
        .dashboard
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "dashboard lock poisoned".to_string())
}

fn map_write_error(e: RestWriteError) -> String {
    match e {
        RestWriteError::Forbidden { message } => message,
        RestWriteError::Unauthorized => {
            "GitHub rejected the token (401). Update your PAT or gh auth.".to_string()
        }
        RestWriteError::Network(m) => m,
        RestWriteError::GitHub { status, message } => format!("GitHub error {status}: {message}"),
    }
}

pub fn preview_row_action(
    state: &AppState,
    group_id: &str,
    org: &str,
    repo: &str,
    number: u32,
    action: RowActionKind,
    note: &str,
) -> Result<crate::row_actions::RowActionPreview, String> {
    if state.fixture_mode {
        return Err("Row actions are disabled in fixture mode.".to_string());
    }
    if !actions_for_group(group_id).contains(&action) {
        return Err("Action is not available for this row.".to_string());
    }
    let config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    if !org_name_is_enabled(&config, org) {
        return Err("That project is turned off in Settings.".to_string());
    }
    let snapshot = state
        .dashboard
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "dashboard lock poisoned".to_string())?;
    let write = state
        .write_state
        .lock()
        .map_err(|_| "write lock poisoned".to_string())?;
    let merged = merged_snapshot(&snapshot, &config, &write.optimistic_labels, &write.pending);
    let key = RowKey {
        org: org.to_string(),
        repo: repo.to_string(),
        number,
    };
    let row = find_row(&merged, &key).ok_or_else(|| "Row not found in dashboard.".to_string())?;
    preview_action(action, &row, &config.labels, note)
}

async fn execute_inner(
    app: &AppHandle,
    state: &Arc<AppState>,
    client: &Client,
    group_id: &str,
    key: RowKey,
    action: RowActionKind,
    note: &str,
) -> Result<(), String> {
    if state.fixture_mode {
        return Err("Row actions are disabled in fixture mode.".to_string());
    }
    if !actions_for_group(group_id).contains(&action) {
        return Err("Action is not available for this row.".to_string());
    }

    let config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    if !org_name_is_enabled(&config, &key.org) {
        return Err("That project is turned off in Settings.".to_string());
    }
    let snapshot = lock_dashboard(state.as_ref())?;
    let row_for_token = {
        let write_guard = state.write_state.lock().map_err(|_| "write lock")?;
        let merged = merged_snapshot(
            &snapshot,
            &config,
            &write_guard.optimistic_labels,
            &write_guard.pending,
        );
        find_row(&merged, &key).ok_or_else(|| "Row not found.".to_string())?
    };
    let token = github_token_for_org(state.as_ref(), &row_for_token.org)?;

    let (row, to_add, to_remove, body) = {
        let mut write_guard = state.write_state.lock().map_err(|_| "write lock")?;
        let merged = merged_snapshot(
            &snapshot,
            &config,
            &write_guard.optimistic_labels,
            &write_guard.pending,
        );
        let row = find_row(&merged, &key).ok_or_else(|| "Row not found.".to_string())?;

        let (to_add, to_remove) = label_diff_for_action(action, &row.labels, &config.labels);
        let body = build_comment_body(action, &row, &config.labels, note, true)?;

        let new_labels = labels_after_diff(&row.labels, &to_add, &to_remove);
        write_guard.pending.insert(key.clone());
        write_guard.optimistic_labels.insert(key.clone(), new_labels.clone());
        write_guard.partial = None;
        write_guard.toast = None;

        let optimistic_snapshot = apply_optimistic_labels(&snapshot, &config, &key, new_labels);
        state.set_dashboard(optimistic_snapshot);
        (row, to_add, to_remove, body)
    };
    let _ = app.emit("dashboard-updated", ());

    let comment_result =
        post_issue_comment(client, &token, &row.org, &row.repo, row.number, &body).await;

    let mut write_guard = state.write_state.lock().map_err(|_| "write lock")?;

    match comment_result {
        Err(e) => {
            write_guard.pending.remove(&key);
            write_guard.optimistic_labels.remove(&key);
            let msg = map_write_error(e);
            state.set_banner(Some(msg.clone()));
            revert_dashboard(state, &snapshot);
            let _ = app.emit("dashboard-updated", ());
            return Err(msg);
        }
        Ok(comment_id) => {
            drop(write_guard);
            let label_result = apply_label_diff(
                client,
                &token,
                &row.org,
                &row.repo,
                row.number,
                &to_add,
                &to_remove,
            )
            .await;

            let mut write_guard = state.write_state.lock().map_err(|_| "write lock")?;
            match label_result {
                Ok(()) => {
                    write_guard.pending.remove(&key);
                    write_guard.undo = Some(make_undo_record(
                        key.clone(),
                        &row,
                        Some(comment_id),
                        &body,
                        &to_add,
                        &to_remove,
                    ));
                    write_guard.toast = Some("Action applied.".to_string());
                    drop(write_guard);
                    let _ = app.emit("dashboard-updated", ());
                    Ok(())
                }
                Err(e) => {
                    let msg = map_write_error(e);
                    write_guard.partial = Some(crate::write_state::PartialFailure {
                        kind: crate::write_state::PartialFailureKind::RetryLabels,
                        message: msg.clone(),
                        key: key.clone(),
                        comment_id: Some(comment_id),
                        labels_to_add: to_add,
                        labels_to_remove: to_remove,
                        comment_body: body.clone(),
                        previous_labels: row.labels.clone(),
                        wakes_agent: crate::row_actions::comment_starts_with_cursor(&body),
                    });
                    write_guard.toast = None;
                    drop(write_guard);
                    state.set_banner(Some(
                        "Comment posted but labels failed. Use Retry labels — no automatic retry.".to_string(),
                    ));
                    let _ = app.emit("dashboard-updated", ());
                    Err(msg)
                }
            }
        }
    }
}

fn revert_dashboard(state: &AppState, previous: &crate::model::DashboardSnapshot) {
    state.set_dashboard(previous.clone());
}

pub async fn execute_row_action(
    app: AppHandle,
    state: Arc<AppState>,
    group_id: String,
    org: String,
    repo: String,
    number: u32,
    action: RowActionKind,
    note: String,
) -> Result<(), String> {
    let client = Client::builder()
        .user_agent("agent-dashboard")
        .build()
        .map_err(|e| e.to_string())?;
    let key = RowKey {
        org,
        repo,
        number,
    };
    execute_inner(&app, &state, &client, &group_id, key, action, &note).await
}

pub async fn undo_row_action(app: AppHandle, state: Arc<AppState>) -> Result<(), String> {
    let state_ref = state.as_ref();
    let client = Client::builder()
        .user_agent("agent-dashboard")
        .build()
        .map_err(|e| e.to_string())?;
    let config = state.config.lock().map(|c| c.clone()).unwrap_or_default();

    let undo_peek = {
        let write_guard = state.write_state.lock().map_err(|_| "write lock")?;
        write_guard
            .undo
            .as_ref()
            .ok_or_else(|| "Nothing to undo.".to_string())?
            .key
            .org
            .clone()
    };
    let token = github_token_for_org(state_ref, &undo_peek)?;

    let undo = {
        let mut write_guard = state.write_state.lock().map_err(|_| "write lock")?;
        write_guard
            .take_undo_if_valid()
            .ok_or_else(|| "Nothing to undo.".to_string())?
    };

    let snapshot = lock_dashboard(state_ref)?;

    let label_result = apply_label_diff(
        &client,
        &token,
        &undo.owner,
        &undo.repo,
        undo.number,
        &undo.undo_labels_add,
        &undo.undo_labels_remove,
    )
    .await;

    if let Err(e) = label_result {
        let mut write_guard = state.write_state.lock().map_err(|_| "write lock")?;
        write_guard.undo = Some(undo);
        return Err(map_write_error(e));
    }

    let toast = if undo.wakes_agent {
        "Labels restored. @cursor comment left in place.".to_string()
    } else if let Some(id) = undo.comment_id {
        match delete_issue_comment(&client, &token, &undo.owner, &undo.repo, id).await {
            Ok(()) => "Undo complete.".to_string(),
            Err(e) => {
                state.set_banner(Some(map_write_error(e)));
                "Labels restored but deleting the audit comment failed.".to_string()
            }
        }
    } else {
        "Undo complete.".to_string()
    };

    {
        let mut write_guard = state.write_state.lock().map_err(|_| "write lock")?;
        write_guard.toast = Some(toast);
        write_guard.optimistic_labels.remove(&undo.key);
        write_guard.pending.remove(&undo.key);
    }
    let rebuilt =
        apply_optimistic_labels(&snapshot, &config, &undo.key, undo.previous_labels.clone());
    state.set_dashboard(rebuilt);
    let _ = app.emit("dashboard-updated", ());
    Ok(())
}

pub async fn retry_partial_write(app: AppHandle, state: Arc<AppState>) -> Result<(), String> {
    let state_ref = state.as_ref();
    let client = Client::builder()
        .user_agent("agent-dashboard")
        .build()
        .map_err(|e| e.to_string())?;
    let org_for_token = {
        let write_guard = state.write_state.lock().map_err(|_| "write lock")?;
        write_guard
            .partial
            .as_ref()
            .ok_or_else(|| "No failed step to retry.".to_string())?
            .key
            .org
            .clone()
    };
    let token = github_token_for_org(state_ref, &org_for_token)?;

    let partial = {
        let mut write_guard = state.write_state.lock().map_err(|_| "write lock")?;
        write_guard
            .partial
            .take()
            .ok_or_else(|| "No failed step to retry.".to_string())?
    };

    apply_label_diff(
        &client,
        &token,
        &partial.key.org,
        &partial.key.repo,
        partial.key.number,
        &partial.labels_to_add,
        &partial.labels_to_remove,
    )
    .await
    .map_err(map_write_error)?;

    let row_for_undo = DashboardRow {
        kind: crate::model::ItemKind::Issue,
        draft: false,
        org: partial.key.org.clone(),
        repo: partial.key.repo.clone(),
        number: partial.key.number,
        title: String::new(),
        url: String::new(),
        author: String::new(),
        updated_at: String::new(),
        labels: partial.previous_labels.clone(),
        ci: None,
        chips: vec![],
    };
    let mut write_guard = state.write_state.lock().map_err(|_| "write lock")?;
    write_guard.undo = Some(make_undo_record(
        partial.key.clone(),
        &row_for_undo,
        partial.comment_id,
        &partial.comment_body,
        &partial.labels_to_add,
        &partial.labels_to_remove,
    ));
    write_guard.toast = Some("Labels applied.".to_string());
    state.set_banner(None);
    let _ = app.emit("dashboard-updated", ());
    Ok(())
}

pub fn write_ui_state(state: &AppState) -> crate::write_state::WriteUiState {
    state
        .write_state
        .lock()
        .map(|mut g| g.ui_state())
        .unwrap_or_else(|_| crate::write_state::WriteUiState {
            pending_keys: vec![],
            undo_available: false,
            undo_message: None,
            partial_failure: None,
            toast: None,
        })
}

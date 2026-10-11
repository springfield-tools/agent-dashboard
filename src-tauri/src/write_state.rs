use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::config::LabelsConfig;
use crate::model::{build_dashboard, DashboardRow, DashboardSnapshot, RawItem};
use crate::row_actions::{comment_starts_with_cursor, RowActionKind};

pub const UNDO_WINDOW_SECS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct RowKey {
    pub org: String,
    pub repo: String,
    pub number: u32,
}

impl RowKey {
    pub fn from_row(row: &DashboardRow) -> Self {
        Self {
            org: row.org.clone(),
            repo: row.repo.clone(),
            number: row.number,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UndoRecord {
    pub key: RowKey,
    pub previous_labels: Vec<String>,
    pub comment_id: Option<u64>,
    pub wakes_agent: bool,
    pub expires_at: Instant,
    pub owner: String,
    pub repo: String,
    pub number: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PartialFailureKind {
    RetryComment,
    RetryLabels,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialFailure {
    pub kind: PartialFailureKind,
    pub message: String,
    pub key: RowKey,
    pub action: RowActionKind,
    pub comment_id: Option<u64>,
    pub labels_to_add: Vec<String>,
    pub labels_to_remove: Vec<String>,
    pub comment_body: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteUiState {
    pub pending_keys: Vec<String>,
    pub undo_available: bool,
    pub undo_message: Option<String>,
    pub partial_failure: Option<PartialFailure>,
    pub toast: Option<String>,
}

pub fn row_key_string(key: &RowKey) -> String {
    format!("{}/{}#{}", key.org, key.repo, key.number)
}

pub struct WriteState {
    pub pending: HashSet<RowKey>,
    pub undo: Option<UndoRecord>,
    pub partial: Option<PartialFailure>,
    pub toast: Option<String>,
    /// Optimistic full label lists per row (overrides snapshot until poll reconciles).
    pub optimistic_labels: std::collections::HashMap<RowKey, Vec<String>>,
}

impl WriteState {
    pub fn new() -> Self {
        Self {
            pending: HashSet::new(),
            undo: None,
            partial: None,
            toast: None,
            optimistic_labels: std::collections::HashMap::new(),
        }
    }

    pub fn clear_on_poll_success(&mut self) {
        self.pending.clear();
        self.optimistic_labels.clear();
        if let Some(undo) = &self.undo {
            if Instant::now() >= undo.expires_at {
                self.undo = None;
            }
        }
    }

    pub fn expire_undo(&mut self) {
        if let Some(undo) = &self.undo {
            if Instant::now() >= undo.expires_at {
                self.undo = None;
            }
        }
    }

    pub fn ui_state(&self) -> WriteUiState {
        self.expire_undo();
        let undo_available = self.undo.is_some();
        let undo_message = if undo_available {
            if self.undo.as_ref().map(|u| u.wakes_agent).unwrap_or(false) {
                Some(
                    "Undo restores labels. The @cursor comment stays; a cloud agent may already be running."
                        .to_string(),
                )
            } else {
                Some("Undo restores labels and deletes the audit comment.".to_string())
            }
        } else {
            None
        };
        WriteUiState {
            pending_keys: self.pending.iter().map(row_key_string).collect(),
            undo_available,
            undo_message,
            partial_failure: self.partial.clone(),
            toast: self.toast.clone(),
        }
    }
}

pub fn snapshot_rows_to_raw(snapshot: &DashboardSnapshot) -> Vec<RawItem> {
    let mut items = Vec::new();
    for group in &snapshot.groups {
        for row in &group.items {
            items.push(raw_from_row(row));
        }
    }
    items
}

fn raw_from_row(row: &DashboardRow) -> RawItem {
    RawItem {
        kind: row.kind,
        org: row.org.clone(),
        repo: row.repo.clone(),
        number: row.number,
        title: row.title.clone(),
        url: row.url.clone(),
        author: row.author.clone(),
        updated_at: row.updated_at.clone(),
        is_draft: row.draft,
        labels: row.labels.clone(),
        ci: row.ci.clone(),
    }
}

pub fn apply_optimistic_labels(
    snapshot: &DashboardSnapshot,
    config: &crate::config::AppConfig,
    key: &RowKey,
    new_labels: Vec<String>,
) -> DashboardSnapshot {
    let mut items = snapshot_rows_to_raw(snapshot);
    for item in &mut items {
        if item.org == key.org && item.repo == key.repo && item.number == key.number {
            item.labels = new_labels;
            break;
        }
    }
    build_dashboard(
        items,
        config,
        &snapshot.fetched_at,
        snapshot.graphql_poll_cost,
        snapshot.graphql_remaining,
    )
}

pub fn merged_snapshot(
    snapshot: &DashboardSnapshot,
    config: &crate::config::AppConfig,
    optimistic: &std::collections::HashMap<RowKey, Vec<String>>,
    pending: &HashSet<RowKey>,
) -> DashboardSnapshot {
    if optimistic.is_empty() && pending.is_empty() {
        return snapshot.clone();
    }
    let mut items = snapshot_rows_to_raw(snapshot);
    for item in &mut items {
        let key = RowKey {
            org: item.org.clone(),
            repo: item.repo.clone(),
            number: item.number,
        };
        if let Some(labels) = optimistic.get(&key) {
            item.labels = labels.clone();
        }
    }
    let mut built = build_dashboard(
        items,
        config,
        &snapshot.fetched_at,
        snapshot.graphql_poll_cost,
        snapshot.graphql_remaining,
    );
    for group in &mut built.groups {
        for row in &mut group.items {
            let key = RowKey::from_row(row);
            if pending.contains(&key) {
                row.chips.push(crate::model::RowChip {
                    label: "Pending…".to_string(),
                });
            }
        }
    }
    built
}

pub fn labels_after_diff(current: &[String], to_add: &[String], to_remove: &[String]) -> Vec<String> {
    let mut set: HashSet<String> = current.iter().cloned().collect();
    for r in to_remove {
        set.remove(r);
    }
    for a in to_add {
        set.insert(a.clone());
    }
    set.into_iter().collect()
}

pub fn status_label_names(labels: &LabelsConfig) -> Vec<String> {
    vec![
        labels.needs_input.clone(),
        labels.needs_testing.clone(),
        labels.ready_to_merge.clone(),
        labels.agent_working.clone(),
    ]
}

pub fn make_undo_record(
    key: RowKey,
    row: &DashboardRow,
    comment_id: Option<u64>,
    body: &str,
) -> UndoRecord {
    UndoRecord {
        key,
        previous_labels: row.labels.clone(),
        comment_id,
        wakes_agent: comment_starts_with_cursor(body),
        expires_at: Instant::now() + Duration::from_secs(UNDO_WINDOW_SECS),
        owner: row.org.clone(),
        repo: row.repo.clone(),
        number: row.number,
    }
}

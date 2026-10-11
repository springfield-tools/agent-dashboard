use std::collections::HashSet;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::config::LabelsConfig;
use crate::model::{build_dashboard_simple, DashboardRow, DashboardSnapshot, RawItem};
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
    /// Full label list before the write (for optimistic UI).
    pub previous_labels: Vec<String>,
    /// Inverse of the write label diff: remove what the write added, add what it removed.
    pub undo_labels_remove: Vec<String>,
    pub undo_labels_add: Vec<String>,
    pub comment_id: Option<u64>,
    pub wakes_agent: bool,
    pub expires_at: Instant,
    pub write_finished_at: DateTime<Utc>,
    pub owner: String,
    pub repo: String,
    pub number: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PartialFailureKind {
    RetryLabels,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialFailure {
    pub kind: PartialFailureKind,
    pub message: String,
    pub key: RowKey,
    pub labels_to_add: Vec<String>,
    pub labels_to_remove: Vec<String>,
    pub previous_labels: Vec<String>,
    pub comment_id: Option<u64>,
    pub comment_body: String,
    pub wakes_agent: bool,
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

    /// Successful poll: drop optimistic overlay; end undo if this fetch is newer than the write or 60s elapsed.
    pub fn reconcile_poll(&mut self, poll_fetched_at: &str) {
        self.pending.clear();
        self.optimistic_labels.clear();
        self.expire_undo_by_time();
        if let Some(undo) = &self.undo {
            if poll_postdates_write(poll_fetched_at, undo.write_finished_at) {
                self.undo = None;
            }
        }
    }

    pub fn expire_undo_by_time(&mut self) {
        if let Some(undo) = &self.undo {
            if Instant::now() >= undo.expires_at {
                self.undo = None;
            }
        }
    }

    pub fn take_undo_if_valid(&mut self) -> Option<UndoRecord> {
        self.expire_undo_by_time();
        self.undo.take()
    }

    pub fn ui_state(&mut self) -> WriteUiState {
        self.expire_undo_by_time();
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

pub fn poll_postdates_write(poll_fetched_at: &str, write_finished_at: DateTime<Utc>) -> bool {
    match DateTime::parse_from_rfc3339(poll_fetched_at) {
        Ok(ts) => ts.with_timezone(&Utc) > write_finished_at,
        Err(_) => true,
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

fn preserve_poll_meta(mut built: DashboardSnapshot, from: &DashboardSnapshot) -> DashboardSnapshot {
    built.bots = from.bots.clone();
    built.graphql_poll_cost = from.graphql_poll_cost;
    built.graphql_remaining = from.graphql_remaining;
    built.graphql_budgets = from.graphql_budgets.clone();
    for section in &mut built.org_sections {
        if let Some(prev) = from.org_sections.iter().find(|s| s.org == section.org) {
            if section.auth_banner.is_none() {
                section.auth_banner = prev.auth_banner.clone();
            }
        }
    }
    built
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
    let built = build_dashboard_simple(
        items,
        config,
        &snapshot.fetched_at,
        snapshot.graphql_poll_cost,
        snapshot.graphql_remaining,
    );
    preserve_poll_meta(built, snapshot)
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
    let mut built = build_dashboard_simple(
        items,
        config,
        &snapshot.fetched_at,
        snapshot.graphql_poll_cost,
        snapshot.graphql_remaining,
    );
    built.bots = snapshot.bots.clone();
    for group in &mut built.groups {
        for row in &mut group.items {
            let key = RowKey::from_row(row);
            if pending.contains(&key) {
                row.chips.push(crate::model::RowChip {
                    label: "Pending…".to_string(),
                    variant: "pending".to_string(),
                });
            }
        }
    }
    for section in &mut built.org_sections {
        for group in &mut section.groups {
            for row in &mut group.items {
                let key = RowKey::from_row(row);
                if pending.contains(&key) {
                    row.chips.push(crate::model::RowChip {
                        label: "Pending…".to_string(),
                        variant: "pending".to_string(),
                    });
                }
            }
        }
    }
    built.graphql_budgets = snapshot.graphql_budgets.clone();
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
    write_labels_add: &[String],
    write_labels_remove: &[String],
) -> UndoRecord {
    UndoRecord {
        key,
        previous_labels: row.labels.clone(),
        undo_labels_remove: write_labels_add.to_vec(),
        undo_labels_add: write_labels_remove.to_vec(),
        comment_id,
        wakes_agent: comment_starts_with_cursor(body),
        expires_at: Instant::now() + Duration::from_secs(UNDO_WINDOW_SECS),
        write_finished_at: Utc::now(),
        owner: row.org.clone(),
        repo: row.repo.clone(),
        number: row.number,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_postdates_write_when_fetched_at_is_later() {
        let write_at = Utc::now();
        let later = (write_at + chrono::Duration::seconds(5)).to_rfc3339();
        assert!(poll_postdates_write(&later, write_at));
    }

    #[test]
    fn poll_does_not_postdate_when_fetched_at_is_older() {
        let write_at = Utc::now();
        let earlier = (write_at - chrono::Duration::seconds(5)).to_rfc3339();
        assert!(!poll_postdates_write(&earlier, write_at));
    }
}

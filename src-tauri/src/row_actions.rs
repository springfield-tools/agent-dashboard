//! Phase 5 row write actions: label diffs and comment bodies (PLAN § Phase 5).

use crate::config::LabelsConfig;
use crate::model::{DashboardRow, ItemKind};

pub const COMMENT_FOOTER: &str = "— Agent Dashboard";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowActionKind {
    TestingPassedReady,
    TestingPassedHandBack,
    SendBackToAgent,
    Answer,
    ApproveReadyToMerge,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowActionPreview {
    pub action: RowActionKind,
    pub labels_to_add: Vec<String>,
    pub labels_to_remove: Vec<String>,
    pub comment_body: String,
    pub wakes_agent: bool,
    pub note_required: bool,
    pub default_note: String,
}

pub fn actions_for_group(group_id: &str) -> Vec<RowActionKind> {
    match group_id {
        "needs-testing" => vec![
            RowActionKind::TestingPassedReady,
            RowActionKind::TestingPassedHandBack,
            RowActionKind::SendBackToAgent,
        ],
        "ci-failing" => vec![RowActionKind::SendBackToAgent],
        "needs-input" => vec![RowActionKind::Answer],
        "not-started" | "agent-working" => vec![RowActionKind::ApproveReadyToMerge],
        _ => vec![],
    }
}

pub fn label_diff_for_action(
    action: RowActionKind,
    current_labels: &[String],
    labels: &LabelsConfig,
) -> (Vec<String>, Vec<String>) {
    let mut remove = Vec::new();
    let mut add = Vec::new();

    let has = |name: &str| current_labels.iter().any(|l| l == name);

    match action {
        RowActionKind::TestingPassedReady => {
            if has(&labels.needs_testing) {
                remove.push(labels.needs_testing.clone());
            }
            if !has(&labels.ready_to_merge) {
                add.push(labels.ready_to_merge.clone());
            }
        }
        RowActionKind::TestingPassedHandBack => {
            if has(&labels.needs_testing) {
                remove.push(labels.needs_testing.clone());
            }
            if !has(&labels.agent_working) {
                add.push(labels.agent_working.clone());
            }
        }
        RowActionKind::SendBackToAgent => {
            for name in [
                labels.needs_testing.as_str(),
                labels.needs_input.as_str(),
                labels.ready_to_merge.as_str(),
            ] {
                if has(name) {
                    remove.push(name.to_string());
                }
            }
            if !has(&labels.agent_working) {
                add.push(labels.agent_working.clone());
            }
        }
        RowActionKind::Answer => {
            if has(&labels.needs_input) {
                remove.push(labels.needs_input.clone());
            }
            if !has(&labels.agent_working) {
                add.push(labels.agent_working.clone());
            }
        }
        RowActionKind::ApproveReadyToMerge => {
            if has(&labels.needs_testing) {
                remove.push(labels.needs_testing.clone());
            }
            if !has(&labels.ready_to_merge) {
                add.push(labels.ready_to_merge.clone());
            }
        }
    }

    (add, remove)
}

pub fn build_comment_body(
    action: RowActionKind,
    row: &DashboardRow,
    labels: &LabelsConfig,
    note: &str,
    for_confirm: bool,
) -> Result<String, String> {
    let trimmed_note = note.trim();
    let body = match action {
        RowActionKind::TestingPassedReady => audit_body("Testing passed. Ready to merge."),
        RowActionKind::ApproveReadyToMerge => audit_body("Approved. Ready to merge."),
        RowActionKind::TestingPassedHandBack | RowActionKind::SendBackToAgent => {
            let default_line = default_agent_line(row, labels);
            agent_body(&default_line, trimmed_note)
        }
        RowActionKind::Answer => {
            if trimmed_note.is_empty() {
                if for_confirm {
                    return Err("Answer text is required.".to_string());
                }
                agent_body("(type your answer above)", "")
            } else {
                agent_body(trimmed_note, "")
            }
        }
    };
    validate_comment_body(&body)?;
    Ok(body)
}

fn audit_body(line: &str) -> String {
    format!("{line}\n\n{COMMENT_FOOTER}")
}

fn agent_body(first_line: &str, optional_note: &str) -> String {
    let mut lines = vec![format!("@cursor {}", first_line.trim())];
    if !optional_note.is_empty() {
        lines.push(optional_note.to_string());
    }
    lines.push(String::new());
    lines.push(COMMENT_FOOTER.to_string());
    lines.join("\n")
}

fn default_agent_line(row: &DashboardRow, _labels: &LabelsConfig) -> String {
    if row.kind == ItemKind::Issue {
        return "Continue on this issue.".to_string();
    }
    if let Some(ci) = &row.ci {
        if !ci.failed_checks.is_empty() {
            let names = ci.failed_checks.join(", ");
            return format!(
                "Look at the failing checks: {names}. Continue on this pull request."
            );
        }
    }
    "Testing did not pass. Continue on this pull request.".to_string()
}

pub fn wakes_agent(action: RowActionKind) -> bool {
    !matches!(
        action,
        RowActionKind::TestingPassedReady | RowActionKind::ApproveReadyToMerge
    )
}

pub fn preview_action(
    action: RowActionKind,
    row: &DashboardRow,
    labels: &LabelsConfig,
    note: &str,
) -> Result<RowActionPreview, String> {
    let (add, remove) = label_diff_for_action(action, &row.labels, labels);
    let comment_body = build_comment_body(action, row, labels, note, false)?;
    Ok(RowActionPreview {
        action,
        labels_to_add: add,
        labels_to_remove: remove,
        comment_body,
        wakes_agent: wakes_agent(action),
        note_required: action == RowActionKind::Answer,
        default_note: if action == RowActionKind::SendBackToAgent
            || action == RowActionKind::TestingPassedHandBack
        {
            String::new()
        } else {
            String::new()
        },
    })
}

pub fn validate_comment_body(body: &str) -> Result<(), String> {
    const FORBIDDEN: [&str; 4] = [
        "cursor review",
        "bugbot run",
        "@cursor autofix off",
        "@cursor autofix on",
    ];
    let lower = body.to_lowercase();
    for needle in FORBIDDEN {
        if lower.contains(needle) {
            return Err(format!("comment must not contain `{needle}`"));
        }
    }
    if !body.ends_with(COMMENT_FOOTER) {
        return Err("comment must end with footer".to_string());
    }
    Ok(())
}

pub fn comment_starts_with_cursor(body: &str) -> bool {
    body.starts_with("@cursor ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CiSnapshot, ItemKind};

    fn sample_labels() -> LabelsConfig {
        LabelsConfig::default()
    }

    fn sample_pr() -> DashboardRow {
        DashboardRow {
            kind: ItemKind::Pr,
            draft: false,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-ui".to_string(),
            number: 1,
            title: "Test".to_string(),
            url: "https://github.com/x/y/pull/1".to_string(),
            author: "human".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            labels: vec![
                "needs-testing".to_string(),
                "dependencies".to_string(),
            ],
            ci: Some(CiSnapshot {
                state: Some("FAILURE".to_string()),
                failed_checks: vec!["ci/build".to_string()],
            }),
            chips: vec![],
        }
    }

    #[test]
    fn label_diff_testing_passed_ready() {
        let labels = sample_labels();
        let (add, remove) =
            label_diff_for_action(RowActionKind::TestingPassedReady, &sample_pr().labels, &labels);
        assert_eq!(remove, vec!["needs-testing"]);
        assert_eq!(add, vec!["ready-to-merge"]);
    }

    #[test]
    fn undo_inverse_swaps_write_label_diff() {
        let labels = sample_labels();
        let row = sample_pr();
        let (add, remove) =
            label_diff_for_action(RowActionKind::TestingPassedReady, &row.labels, &labels);
        let undo = crate::write_state::make_undo_record(
            crate::write_state::RowKey {
                org: row.org.clone(),
                repo: row.repo.clone(),
                number: row.number,
            },
            &row,
            Some(1),
            "Testing passed. Ready to merge.\n\n— Agent Dashboard",
            &add,
            &remove,
        );
        assert_eq!(undo.undo_labels_remove, add);
        assert_eq!(undo.undo_labels_add, remove);
        assert!(!undo.wakes_agent);
    }

    #[test]
    fn hand_back_only_swaps_testing_to_agent() {
        let labels = sample_labels();
        let row = sample_pr();
        let (add, remove) =
            label_diff_for_action(RowActionKind::TestingPassedHandBack, &row.labels, &labels);
        assert_eq!(remove, vec!["needs-testing"]);
        assert_eq!(add, vec!["agent-working"]);
    }

    #[test]
    fn label_diff_send_back_keeps_dependencies() {
        let labels = sample_labels();
        let row = sample_pr();
        let (add, remove) =
            label_diff_for_action(RowActionKind::SendBackToAgent, &row.labels, &labels);
        assert!(remove.contains(&"needs-testing".to_string()));
        assert!(add.contains(&"agent-working".to_string()));
        assert!(!remove.contains(&"dependencies".to_string()));
    }

    #[test]
    fn agent_comment_starts_with_cursor() {
        let labels = sample_labels();
        let body = build_comment_body(
            RowActionKind::SendBackToAgent,
            &sample_pr(),
            &labels,
            "fix pls",
            true,
        )
        .expect("body");
        assert!(body.starts_with("@cursor "));
        assert!(body.contains("fix pls"));
        assert!(body.ends_with(COMMENT_FOOTER));
    }

    #[test]
    fn audit_comment_has_no_cursor() {
        let labels = sample_labels();
        let body = build_comment_body(
            RowActionKind::TestingPassedReady,
            &sample_pr(),
            &labels,
            "",
            true,
        )
        .expect("body");
        assert!(!body.starts_with("@cursor"));
        assert!(body.contains("Testing passed. Ready to merge."));
    }

    #[test]
    fn answer_preview_allows_empty_note() {
        let labels = sample_labels();
        let body = build_comment_body(RowActionKind::Answer, &sample_pr(), &labels, "", false)
            .expect("preview");
        assert!(body.starts_with("@cursor "));
    }

    #[test]
    fn answer_requires_text() {
        let labels = sample_labels();
        let err = build_comment_body(RowActionKind::Answer, &sample_pr(), &labels, "  ", true)
            .unwrap_err();
        assert!(err.contains("required"));
    }

    #[test]
    fn ci_failing_prefill_in_default_line() {
        let labels = sample_labels();
        let line = default_agent_line(&sample_pr(), &labels);
        assert!(line.contains("ci/build"));
    }

    #[test]
    fn forbidden_strings_rejected() {
        let bad = "@cursor do thing\n\nbugbot run\n\n— Agent Dashboard";
        assert!(validate_comment_body(bad).is_err());
    }
}

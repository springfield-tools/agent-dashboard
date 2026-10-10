use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, BotFilterMode, LabelsConfig};

pub const GROUP_IDS: [&str; 6] = [
    "needs-input",
    "ci-failing",
    "needs-testing",
    "ready-to-merge",
    "agent-working",
    "not-started",
];

pub const TRAY_COUNT_IDS: [&str; 4] = [
    "needs-input",
    "needs-testing",
    "ci-failing",
    "ready-to-merge",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Issue,
    Pr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiSnapshot {
    pub state: Option<String>,
    #[serde(default)]
    pub failed_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawItem {
    pub kind: ItemKind,
    pub org: String,
    pub repo: String,
    pub number: u32,
    pub title: String,
    pub url: String,
    pub author: String,
    pub updated_at: String,
    #[serde(default)]
    pub is_draft: bool,
    #[serde(default)]
    pub labels: Vec<String>,
    pub ci: Option<CiSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureFile {
    pub items: Vec<RawItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowChip {
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardRow {
    pub kind: ItemKind,
    pub draft: bool,
    pub org: String,
    pub repo: String,
    pub number: u32,
    pub title: String,
    pub url: String,
    pub author: String,
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<String>,
    pub ci: Option<CiSnapshot>,
    #[serde(default)]
    pub chips: Vec<RowChip>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardGroup {
    pub id: String,
    pub title: String,
    pub items: Vec<DashboardRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotFooter {
    pub mode: String,
    pub green: u32,
    #[serde(rename = "noCi")]
    pub no_ci: u32,
    pub failing: u32,
    pub search_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub schema_version: u32,
    pub fetched_at: String,
    pub stale: bool,
    pub groups: Vec<DashboardGroup>,
    pub bots: Option<BotFooter>,
    pub tray_title: String,
    pub tray_tooltip: String,
}

fn group_title(id: &str) -> &'static str {
    match id {
        "needs-input" => "Needs input",
        "ci-failing" => "CI failing",
        "needs-testing" => "Needs testing",
        "ready-to-merge" => "Ready to merge",
        "agent-working" => "Agent working",
        "not-started" => "Not started",
        _ => "Unknown",
    }
}

pub fn is_bot_author(author: &str, bot_logins: &[String]) -> bool {
    let author_lower = author.to_lowercase();
    for login in bot_logins {
        let login_lower = login.to_lowercase();
        let stripped = login_lower.trim_end_matches("[bot]");
        if author_lower == login_lower || author_lower == stripped {
            return true;
        }
    }
    false
}

fn has_label(labels: &[String], name: &str) -> bool {
    labels.iter().any(|l| l == name)
}

fn ci_is_failing(ci: &Option<CiSnapshot>) -> bool {
    match ci {
        Some(ci) => matches!(ci.state.as_deref(), Some("FAILURE") | Some("ERROR")),
        None => false,
    }
}

/// Primary group for an item before bot filtering (ADR 0004 / PLAN §1).
pub fn assign_group(item: &RawItem, labels: &LabelsConfig) -> &'static str {
    if has_label(&item.labels, &labels.needs_input) {
        return "needs-input";
    }
    if item.kind == ItemKind::Pr && ci_is_failing(&item.ci) {
        return "ci-failing";
    }
    if has_label(&item.labels, &labels.needs_testing) {
        return "needs-testing";
    }
    if has_label(&item.labels, &labels.ready_to_merge) {
        return "ready-to-merge";
    }
    if has_label(&item.labels, &labels.agent_working) {
        return "agent-working";
    }
    "not-started"
}

fn status_label_names(labels: &LabelsConfig) -> [&str; 4] {
    [
        labels.needs_input.as_str(),
        labels.needs_testing.as_str(),
        labels.ready_to_merge.as_str(),
        labels.agent_working.as_str(),
    ]
}

pub fn row_chips(item: &RawItem, group_id: &str, labels: &LabelsConfig) -> Vec<RowChip> {
    let mut chips = Vec::new();
    let status = status_label_names(labels);

    for name in status {
        if has_label(&item.labels, name) && !group_matches_label(group_id, name, labels) {
            chips.push(RowChip {
                label: name.to_string(),
            });
        }
    }

    if let Some(ci) = &item.ci {
        if group_id != "ci-failing" {
            match ci.state.as_deref() {
                Some("PENDING") => chips.push(RowChip {
                    label: "CI running".to_string(),
                }),
                Some("FAILURE") | Some("ERROR") => chips.push(RowChip {
                    label: "CI failing".to_string(),
                }),
                _ => {}
            }
        }
    }

    chips
}

fn group_matches_label(group_id: &str, label: &str, labels: &LabelsConfig) -> bool {
    match group_id {
        "needs-input" => label == labels.needs_input,
        "needs-testing" => label == labels.needs_testing,
        "ready-to-merge" => label == labels.ready_to_merge,
        "agent-working" => label == labels.agent_working,
        _ => false,
    }
}

fn bot_search_url(org: &str) -> String {
    format!(
        "https://github.com/search?q=org%3A{}+is%3Apr+is%3Aopen+author%3Aapp%2Fdependabot",
        org
    )
}

/// `counts` must be in [`TRAY_COUNT_IDS`] order: input · testing · failing · merge.
pub fn format_tray_title(counts: &[(String, u32)]) -> String {
    if counts.len() != TRAY_COUNT_IDS.len() {
        return String::new();
    }
    for (expected, (id, _)) in TRAY_COUNT_IDS.iter().zip(counts.iter()) {
        if id != expected {
            return String::new();
        }
    }
    let parts: Vec<String> = counts.iter().map(|(_, n)| n.to_string()).collect();
    if parts.iter().all(|p| *p == "0") {
        return String::new();
    }
    parts.join("·")
}

fn tray_counts_in_adr_order(buckets: &[Vec<DashboardRow>]) -> Vec<(String, u32)> {
    TRAY_COUNT_IDS
        .iter()
        .map(|id| {
            let idx = GROUP_IDS
                .iter()
                .position(|g| g == id)
                .expect("tray slot maps to group id");
            (id.to_string(), buckets[idx].len() as u32)
        })
        .collect()
}

pub fn tray_tooltip() -> String {
    "Needs input · needs testing · CI failing · ready to merge".to_string()
}

pub fn empty_tray_tooltip() -> String {
    "Nothing waiting on you".to_string()
}

pub fn build_dashboard(items: Vec<RawItem>, config: &AppConfig, fetched_at: &str) -> DashboardSnapshot {
    let labels = &config.labels;
    let bot_mode = config.filters.bots;
    let surface_failing = config.filters.surface_failing_bots;

    let mut bot_green = 0u32;
    let mut bot_no_ci = 0u32;
    let bot_failing = 0u32;

    let mut buckets: Vec<Vec<DashboardRow>> = vec![vec![]; GROUP_IDS.len()];

    for item in items {
        let is_bot = is_bot_author(&item.author, &config.filters.bot_logins);
        let group_id = assign_group(&item, labels);
        let failing = item.kind == ItemKind::Pr && ci_is_failing(&item.ci);

        if is_bot {
            match bot_mode {
                BotFilterMode::Hide => {
                    if surface_failing && failing {
                        push_row(&mut buckets, group_id, item, labels);
                    }
                    continue;
                }
                BotFilterMode::Collapse => {
                    if surface_failing && failing {
                        bot_failing += 1;
                        push_row(&mut buckets, "ci-failing", item, labels);
                    } else {
                        match item.ci.as_ref().and_then(|c| c.state.as_deref()) {
                            Some("SUCCESS") => bot_green += 1,
                            None => bot_no_ci += 1,
                            Some("FAILURE") | Some("ERROR") => bot_failing += 1,
                            _ => {}
                        }
                    }
                    continue;
                }
                BotFilterMode::Show => {}
            }
        }

        push_row(&mut buckets, group_id, item, labels);
    }

    let mut groups = Vec::new();

    for (idx, id) in GROUP_IDS.iter().enumerate() {
        let rows = &mut buckets[idx];
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        if !rows.is_empty() {
            groups.push(DashboardGroup {
                id: id.to_string(),
                title: group_title(id).to_string(),
                items: rows.clone(),
            });
        }
    }

    let tray_counts = tray_counts_in_adr_order(&buckets);
    let tray_title = format_tray_title(&tray_counts);
    let tray_tooltip = if tray_title.is_empty() {
        empty_tray_tooltip()
    } else {
        tray_tooltip()
    };

    let bots = if bot_mode == BotFilterMode::Collapse && (bot_green > 0 || bot_no_ci > 0 || bot_failing > 0) {
        let org = config
            .orgs
            .first()
            .map(|o| o.name.as_str())
            .unwrap_or("springfield-ham-radio");
        Some(BotFooter {
            mode: "collapse".to_string(),
            green: bot_green,
            no_ci: bot_no_ci,
            failing: bot_failing,
            search_url: bot_search_url(org),
        })
    } else {
        None
    };

    DashboardSnapshot {
        schema_version: 1,
        fetched_at: fetched_at.to_string(),
        stale: false,
        groups,
        bots,
        tray_title,
        tray_tooltip,
    }
}

fn push_row(
    buckets: &mut [Vec<DashboardRow>],
    group_id: &str,
    item: RawItem,
    labels: &LabelsConfig,
) {
    let idx = GROUP_IDS
        .iter()
        .position(|id| *id == group_id)
        .unwrap_or(GROUP_IDS.len() - 1);
    let chips = row_chips(&item, group_id, labels);
    buckets[idx].push(DashboardRow {
        kind: item.kind,
        draft: item.is_draft,
        org: item.org,
        repo: item.repo,
        number: item.number,
        title: item.title,
        url: item.url,
        author: item.author,
        updated_at: item.updated_at,
        labels: item.labels,
        ci: item.ci,
        chips,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn sample_item(labels: Vec<&str>, ci_state: Option<&str>) -> RawItem {
        RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-ui".to_string(),
            number: 1,
            title: "test".to_string(),
            url: "https://example.com".to_string(),
            author: "human".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: labels.into_iter().map(String::from).collect(),
            ci: Some(CiSnapshot {
                state: ci_state.map(String::from),
                failed_checks: vec![],
            }),
        }
    }

    #[test]
    fn needs_input_wins_over_agent_working() {
        let labels = LabelsConfig::default();
        let item = sample_item(vec!["needs-input", "agent-working"], Some("SUCCESS"));
        assert_eq!(assign_group(&item, &labels), "needs-input");
        let snapshot = build_dashboard(vec![item], &AppConfig::default(), "now");
        assert_eq!(snapshot.groups[0].id, "needs-input");
        assert!(snapshot.groups[0].items[0].chips.iter().any(|c| c.label == "agent-working"));
    }

    #[test]
    fn ci_failing_wins_over_needs_testing_and_keeps_chip() {
        let labels = LabelsConfig::default();
        let item = sample_item(vec!["needs-testing"], Some("FAILURE"));
        assert_eq!(assign_group(&item, &labels), "ci-failing");
        let snapshot = build_dashboard(vec![item], &AppConfig::default(), "now");
        assert_eq!(snapshot.groups[0].id, "ci-failing");
        assert!(snapshot
            .groups[0]
            .items[0]
            .chips
            .iter()
            .any(|c| c.label == "needs-testing"));
    }

    #[test]
    fn collapses_green_dependabot_to_footer() {
        let config = AppConfig::default();
        let mut items = Vec::new();
        for n in 1..=29 {
            items.push(RawItem {
                kind: ItemKind::Pr,
                org: "springfield-ham-radio".to_string(),
                repo: "ham-radio-docs".to_string(),
                number: n,
                title: format!("chore(deps): bump {n}"),
                url: format!("https://github.com/springfield-ham-radio/ham-radio-docs/pull/{n}"),
                author: "dependabot[bot]".to_string(),
                updated_at: "2026-10-10T12:00:00Z".to_string(),
                is_draft: false,
                labels: vec![],
                ci: Some(CiSnapshot {
                    state: Some("SUCCESS".to_string()),
                    failed_checks: vec![],
                }),
            });
        }
        for n in 30..=31 {
            items.push(RawItem {
                kind: ItemKind::Pr,
                org: "springfield-ham-radio".to_string(),
                repo: "ham-radio-docs".to_string(),
                number: n,
                title: format!("chore(deps): no ci {n}"),
                url: format!("https://github.com/springfield-ham-radio/ham-radio-docs/pull/{n}"),
                author: "dependabot".to_string(),
                updated_at: "2026-10-10T12:00:00Z".to_string(),
                is_draft: false,
                labels: vec![],
                ci: None,
            });
        }
        items.push(RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-ui".to_string(),
            number: 84,
            title: "draft ui".to_string(),
            url: "https://github.com/springfield-ham-radio/ham-radio-ui/pull/84".to_string(),
            author: "BryanHunt".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: true,
            labels: vec![],
            ci: None,
        });
        items.push(RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: ".github".to_string(),
            number: 4,
            title: "draft github".to_string(),
            url: "https://github.com/springfield-ham-radio/.github/pull/4".to_string(),
            author: "BryanHunt".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: true,
            labels: vec![],
            ci: None,
        });

        let snapshot = build_dashboard(items, &config, "now");
        assert!(snapshot.bots.is_some());
        let bots = snapshot.bots.unwrap();
        assert_eq!(bots.green, 29);
        assert_eq!(bots.no_ci, 2);
        let not_started = snapshot
            .groups
            .iter()
            .find(|g| g.id == "not-started")
            .expect("not started");
        assert_eq!(not_started.items.len(), 2);
        assert!(not_started.items.iter().all(|r| r.draft));
    }

    #[test]
    fn tray_title_empty_when_all_quiet() {
        let snapshot = build_dashboard(vec![], &AppConfig::default(), "now");
        assert!(snapshot.tray_title.is_empty());
        assert_eq!(snapshot.tray_tooltip, empty_tray_tooltip());
    }

    #[test]
    fn tray_title_uses_adr_slot_order() {
        let counts = vec![
            ("needs-input".to_string(), 1),
            ("needs-testing".to_string(), 2),
            ("ci-failing".to_string(), 3),
            ("ready-to-merge".to_string(), 4),
        ];
        assert_eq!(format_tray_title(&counts), "1·2·3·4");
    }

    #[test]
    fn tray_title_from_groups_follows_input_testing_failing_merge() {
        let mut config = AppConfig::default();
        config.filters.bots = BotFilterMode::Show;

        let items = vec![
            sample_item(vec!["needs-input"], Some("SUCCESS")),
            sample_item(vec!["needs-testing"], Some("SUCCESS")),
            RawItem {
                kind: ItemKind::Pr,
                org: "o".to_string(),
                repo: "r".to_string(),
                number: 3,
                title: "fail".to_string(),
                url: "https://example.com".to_string(),
                author: "human".to_string(),
                updated_at: "2026-10-10T12:00:00Z".to_string(),
                is_draft: false,
                labels: vec![],
                ci: Some(CiSnapshot {
                    state: Some("FAILURE".to_string()),
                    failed_checks: vec![],
                }),
            },
            sample_item(vec!["ready-to-merge"], Some("SUCCESS")),
        ];

        let snapshot = build_dashboard(items, &config, "now");
        assert_eq!(snapshot.tray_title, "1·1·1·1");
    }

    #[test]
    fn hide_mode_drops_bots_from_groups_and_footer() {
        let mut config = AppConfig::default();
        config.filters.bots = BotFilterMode::Hide;
        let item = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-docs".to_string(),
            number: 9,
            title: "bot pr".to_string(),
            url: "https://example.com".to_string(),
            author: "dependabot[bot]".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![],
            ci: Some(CiSnapshot {
                state: Some("SUCCESS".to_string()),
                failed_checks: vec![],
            }),
        };
        let snapshot = build_dashboard(vec![item], &config, "now");
        assert!(snapshot.bots.is_none());
        assert!(snapshot.groups.is_empty());
    }

    #[test]
    fn collapse_without_surface_failing_keeps_red_bot_in_footer() {
        let mut config = AppConfig::default();
        config.filters.bots = BotFilterMode::Collapse;
        config.filters.surface_failing_bots = false;
        let item = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-docs".to_string(),
            number: 9,
            title: "bot fail".to_string(),
            url: "https://example.com".to_string(),
            author: "dependabot[bot]".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![],
            ci: Some(CiSnapshot {
                state: Some("FAILURE".to_string()),
                failed_checks: vec![],
            }),
        };
        let snapshot = build_dashboard(vec![item], &config, "now");
        assert!(snapshot.groups.is_empty());
        let bots = snapshot.bots.expect("footer");
        assert_eq!(bots.failing, 1);
        assert_eq!(bots.green, 0);
    }

    #[test]
    fn show_mode_lists_bots_in_groups() {
        let mut config = AppConfig::default();
        config.filters.bots = BotFilterMode::Show;
        let item = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-docs".to_string(),
            number: 9,
            title: "bot pr".to_string(),
            url: "https://example.com".to_string(),
            author: "dependabot[bot]".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![],
            ci: Some(CiSnapshot {
                state: Some("SUCCESS".to_string()),
                failed_checks: vec![],
            }),
        };
        let snapshot = build_dashboard(vec![item], &config, "now");
        assert!(snapshot.bots.is_none());
        assert_eq!(snapshot.groups[0].id, "not-started");
        assert_eq!(snapshot.groups[0].items.len(), 1);
    }
}

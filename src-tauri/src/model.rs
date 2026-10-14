use std::collections::{HashMap, HashSet};

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
pub struct OrgSectionNotice {
    pub summary: String,
    pub details: Vec<String>,
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
    /// CI rollup was omitted because the token cannot read commit/status data.
    #[serde(default)]
    pub ci_unknown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureFile {
    pub items: Vec<RawItem>,
}

fn default_chip_variant() -> String {
    "neutral".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowChip {
    pub label: String,
    #[serde(default = "default_chip_variant")]
    pub variant: String,
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
pub struct DashboardOrgSection {
    pub org: String,
    /// Tray-style counts for this org: input · testing · failing · merge.
    pub tray_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_banner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_notice: Option<OrgSectionNotice>,
    pub groups: Vec<DashboardGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphqlBudgetLine {
    /// Org name for a Keychain PAT bucket, or `gh` for the CLI session.
    pub credential_key: String,
    pub poll_cost: i32,
    pub remaining: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotFooter {
    pub mode: String,
    pub green: u32,
    #[serde(rename = "noCi")]
    pub no_ci: u32,
    pub failing: u32,
    pub pending: u32,
    pub search_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub schema_version: u32,
    pub fetched_at: String,
    pub stale: bool,
    pub groups: Vec<DashboardGroup>,
    #[serde(default)]
    pub org_sections: Vec<DashboardOrgSection>,
    pub bots: Option<BotFooter>,
    pub tray_title: String,
    pub tray_tooltip: String,
    /// Sum of `rateLimit.cost` across GraphQL calls in the last poll tick.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graphql_poll_cost: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graphql_remaining: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphql_budgets: Vec<GraphqlBudgetLine>,
    /// Orgs with `enabled = false` in config (shown in Settings; omitted from popover).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub disabled_org_count: u32,
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
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

fn chip_variant_for_status_label(name: &str, labels: &LabelsConfig) -> &'static str {
    if name == labels.needs_input {
        "needs-input"
    } else if name == labels.needs_testing {
        "needs-testing"
    } else if name == labels.ready_to_merge {
        "ready-to-merge"
    } else if name == labels.agent_working {
        "agent-working"
    } else {
        "neutral"
    }
}

pub fn row_chips(item: &RawItem, group_id: &str, labels: &LabelsConfig) -> Vec<RowChip> {
    let mut chips = Vec::new();
    let status = status_label_names(labels);

    for name in status {
        if has_label(&item.labels, name) && !group_matches_label(group_id, name, labels) {
            chips.push(RowChip {
                label: name.to_string(),
                variant: chip_variant_for_status_label(name, labels).to_string(),
            });
        }
    }

    if item.ci_unknown && item.kind == ItemKind::Pr {
        chips.push(RowChip {
            label: "CI ?".to_string(),
            variant: "ci-unknown".to_string(),
        });
    } else if let Some(ci) = &item.ci {
        if group_id == "ci-failing" {
            let names = &ci.failed_checks;
            const MAX: usize = 3;
            for (i, check) in names.iter().enumerate() {
                if i >= MAX {
                    let more = names.len() - MAX;
                    chips.push(RowChip {
                        label: format!("and {more} more"),
                        variant: "ci-failing".to_string(),
                    });
                    break;
                }
                chips.push(RowChip {
                    label: check.clone(),
                    variant: "ci-failing".to_string(),
                });
            }
        } else {
            match ci.state.as_deref() {
                Some("PENDING") => chips.push(RowChip {
                    label: "CI running".to_string(),
                    variant: "ci-running".to_string(),
                }),
                Some("FAILURE") | Some("ERROR") => chips.push(RowChip {
                    label: "CI failing".to_string(),
                    variant: "ci-failing".to_string(),
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

/// True when the snapshot is at least `interval_secs` old (or timestamp is unparseable).
pub fn snapshot_is_stale(fetched_at: &str, interval_secs: u64) -> bool {
    match chrono::DateTime::parse_from_rfc3339(fetched_at) {
        Ok(ts) => {
            let age = chrono::Utc::now() - ts.with_timezone(&chrono::Utc);
            age.num_seconds() >= interval_secs as i64
        }
        Err(_) => true,
    }
}

fn groups_from_buckets(buckets: &[Vec<DashboardRow>]) -> Vec<DashboardGroup> {
    let mut groups = Vec::new();
    for (idx, id) in GROUP_IDS.iter().enumerate() {
        let rows = &buckets[idx];
        if rows.is_empty() {
            continue;
        }
        let mut sorted = rows.clone();
        sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        groups.push(DashboardGroup {
            id: id.to_string(),
            title: group_title(id).to_string(),
            items: sorted,
        });
    }
    groups
}

fn build_buckets_for_items(
    items: Vec<RawItem>,
    config: &AppConfig,
    labels: &LabelsConfig,
    bot_mode: BotFilterMode,
    surface_failing: bool,
    bot_counters: &mut (u32, u32, u32, u32),
) -> Vec<Vec<DashboardRow>> {
    let mut buckets: Vec<Vec<DashboardRow>> = vec![vec![]; GROUP_IDS.len()];
    let (bot_green, bot_no_ci, bot_failing, bot_pending) = bot_counters;

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
                        *bot_failing += 1;
                        push_row(&mut buckets, "ci-failing", item, labels);
                    } else {
                        match item.ci.as_ref().and_then(|c| c.state.as_deref()) {
                            Some("SUCCESS") => *bot_green += 1,
                            None => *bot_no_ci += 1,
                            Some("FAILURE") | Some("ERROR") => *bot_failing += 1,
                            Some("PENDING") | Some("EXPECTED") => *bot_pending += 1,
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

    buckets
}

pub fn build_dashboard_simple(
    items: Vec<RawItem>,
    config: &AppConfig,
    fetched_at: &str,
    graphql_poll_cost: Option<i32>,
    graphql_remaining: Option<i32>,
) -> DashboardSnapshot {
    build_dashboard(
        items,
        config,
        fetched_at,
        graphql_poll_cost,
        graphql_remaining,
        vec![],
        HashMap::new(),
        HashMap::new(),
    )
}

pub fn build_dashboard(
    items: Vec<RawItem>,
    config: &AppConfig,
    fetched_at: &str,
    graphql_poll_cost: Option<i32>,
    graphql_remaining: Option<i32>,
    graphql_budgets: Vec<GraphqlBudgetLine>,
    org_auth_banners: HashMap<String, String>,
    org_section_notices: HashMap<String, OrgSectionNotice>,
) -> DashboardSnapshot {
    let disabled_org_count = crate::config::disabled_org_count(config) as u32;
    let enabled_org_names: HashSet<String> = config
        .orgs
        .iter()
        .filter(|o| o.enabled)
        .map(|o| o.name.clone())
        .collect();
    let items: Vec<RawItem> = items
        .into_iter()
        .filter(|i| enabled_org_names.contains(&i.org))
        .collect();
    let graphql_budgets: Vec<GraphqlBudgetLine> = graphql_budgets
        .into_iter()
        .filter(|b| {
            if b.credential_key == "gh" {
                config
                    .orgs
                    .iter()
                    .any(|o| o.enabled && crate::credential::org_uses_gh(o, config))
            } else {
                enabled_org_names.contains(&b.credential_key)
            }
        })
        .collect();

    let labels = &config.labels;
    let bot_mode = config.filters.bots;
    let surface_failing = config.filters.surface_failing_bots;

    let mut bot_counters = (0u32, 0u32, 0u32, 0u32);
    let buckets = build_buckets_for_items(
        items.clone(),
        config,
        labels,
        bot_mode,
        surface_failing,
        &mut bot_counters,
    );
    let (bot_green, bot_no_ci, bot_failing, bot_pending) = bot_counters;

    let groups = groups_from_buckets(&buckets);

    let mut org_sections = Vec::new();
    for org_cfg in config.orgs.iter().filter(|o| o.enabled) {
        let org_items: Vec<RawItem> = items
            .iter()
            .filter(|i| i.org == org_cfg.name)
            .cloned()
            .collect();
        let mut local_bot = (0u32, 0u32, 0u32, 0u32);
        let org_buckets = build_buckets_for_items(
            org_items.clone(),
            config,
            labels,
            bot_mode,
            surface_failing,
            &mut local_bot,
        );
        let org_tray = format_tray_title(&tray_counts_in_adr_order(&org_buckets));
        org_sections.push(DashboardOrgSection {
            org: org_cfg.name.clone(),
            tray_title: org_tray,
            auth_banner: org_auth_banners.get(&org_cfg.name).cloned(),
            section_notice: org_section_notices.get(&org_cfg.name).cloned(),
            groups: groups_from_buckets(&org_buckets),
        });
    }

    let tray_counts = tray_counts_in_adr_order(&buckets);
    let tray_title = format_tray_title(&tray_counts);
    let tray_tooltip = if tray_title.is_empty() {
        empty_tray_tooltip()
    } else {
        tray_tooltip()
    };

    let bots = if bot_mode == BotFilterMode::Collapse
        && (bot_green > 0 || bot_no_ci > 0 || bot_failing > 0 || bot_pending > 0)
    {
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
            pending: bot_pending,
            search_url: bot_search_url(org),
        })
    } else {
        None
    };

    DashboardSnapshot {
        schema_version: 2,
        fetched_at: fetched_at.to_string(),
        stale: false,
        groups,
        org_sections,
        bots,
        tray_title,
        tray_tooltip,
        graphql_poll_cost,
        graphql_remaining,
        graphql_budgets,
        disabled_org_count,
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
            ci_unknown: false,
        }
    }

    #[test]
    fn needs_input_wins_over_agent_working() {
        let labels = LabelsConfig::default();
        let item = sample_item(vec!["needs-input", "agent-working"], Some("SUCCESS"));
        assert_eq!(assign_group(&item, &labels), "needs-input");
        let snapshot = build_dashboard_simple(vec![item], &AppConfig::default(), "now", None, None);
        assert_eq!(snapshot.groups[0].id, "needs-input");
        assert!(snapshot.groups[0].items[0].chips.iter().any(|c| c.label == "agent-working"));
    }

    #[test]
    fn ci_failing_wins_over_needs_testing_and_keeps_chip() {
        let labels = LabelsConfig::default();
        let item = sample_item(vec!["needs-testing"], Some("FAILURE"));
        assert_eq!(assign_group(&item, &labels), "ci-failing");
        let snapshot = build_dashboard_simple(vec![item], &AppConfig::default(), "now", None, None);
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
                ci_unknown: false,
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
                ci_unknown: false,
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
            ci_unknown: false,
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
            ci_unknown: false,
        });

        let snapshot = build_dashboard_simple(items, &config, "now", None, None);
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
    fn ci_unknown_chip_on_pr() {
        let item = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-tools".to_string(),
            repo: "agent-dashboard".to_string(),
            number: 1,
            title: "x".to_string(),
            url: "https://example.com".to_string(),
            author: "human".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![],
            ci: None,
            ci_unknown: true,
        };
        let chips = row_chips(&item, "not-started", &AppConfig::default().labels);
        assert!(chips.iter().any(|c| c.label == "CI ?" && c.variant == "ci-unknown"));
    }

    #[test]
    fn tray_title_empty_when_all_quiet() {
        let snapshot = build_dashboard_simple(vec![], &AppConfig::default(), "now", None, None);
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
                ci_unknown: false,
            },
            sample_item(vec!["ready-to-merge"], Some("SUCCESS")),
        ];

        let snapshot = build_dashboard_simple(items, &config, "now", None, None);
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
            ci_unknown: false,
        };
        let snapshot = build_dashboard_simple(vec![item], &config, "now", None, None);
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
            ci_unknown: false,
        };
        let snapshot = build_dashboard_simple(vec![item], &config, "now", None, None);
        assert!(snapshot.groups.is_empty());
        let bots = snapshot.bots.expect("footer");
        assert_eq!(bots.failing, 1);
        assert_eq!(bots.green, 0);
    }

    #[test]
    fn collapse_counts_pending_bots_in_footer() {
        let mut config = AppConfig::default();
        config.filters.bots = BotFilterMode::Collapse;
        let item = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-docs".to_string(),
            number: 9,
            title: "bot pending".to_string(),
            url: "https://example.com".to_string(),
            author: "dependabot[bot]".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![],
            ci: Some(CiSnapshot {
                state: Some("PENDING".to_string()),
                failed_checks: vec![],
            }),
            ci_unknown: false,
        };
        let snapshot = build_dashboard_simple(vec![item], &config, "now", None, None);
        let bots = snapshot.bots.expect("footer");
        assert_eq!(bots.pending, 1);
        assert_eq!(bots.green, 0);
    }

    #[test]
    fn ci_failing_chip_includes_failed_check_names() {
        let config = AppConfig::default();
        let item = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-ui".to_string(),
            number: 1,
            title: "red".to_string(),
            url: "https://example.com".to_string(),
            author: "human".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![],
            ci: Some(CiSnapshot {
                state: Some("FAILURE".to_string()),
                failed_checks: vec!["build".to_string(), "lint".to_string()],
            }),
            ci_unknown: false,
        };
        let snapshot = build_dashboard_simple(vec![item], &config, "now", None, None);
        let row = &snapshot.groups[0].items[0];
        assert!(row.chips.iter().any(|c| c.label == "build" && c.variant == "ci-failing"));
    }

    #[test]
    fn ci_failing_without_check_names_still_groups() {
        let config = AppConfig::default();
        let item = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-ui".to_string(),
            number: 1,
            title: "red".to_string(),
            url: "https://example.com".to_string(),
            author: "human".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![],
            ci: Some(CiSnapshot {
                state: Some("FAILURE".to_string()),
                failed_checks: vec![],
            }),
            ci_unknown: false,
        };
        let snapshot = build_dashboard_simple(vec![item], &config, "now", None, None);
        assert_eq!(snapshot.groups[0].id, "ci-failing");
    }

    #[test]
    fn snapshot_stale_after_interval() {
        let old = (chrono::Utc::now() - chrono::Duration::seconds(120))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert!(snapshot_is_stale(&old, 60));
        let fresh = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert!(!snapshot_is_stale(&fresh, 60));
    }

    #[test]
    fn collapse_counts_expected_bots_with_pending() {
        let mut config = AppConfig::default();
        config.filters.bots = BotFilterMode::Collapse;
        let item = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-ham-radio".to_string(),
            repo: "ham-radio-docs".to_string(),
            number: 10,
            title: "bot expected".to_string(),
            url: "https://example.com".to_string(),
            author: "dependabot[bot]".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![],
            ci: Some(CiSnapshot {
                state: Some("EXPECTED".to_string()),
                failed_checks: vec![],
            }),
            ci_unknown: false,
        };
        let snapshot = build_dashboard_simple(vec![item], &config, "now", None, None);
        let bots = snapshot.bots.expect("footer");
        assert_eq!(bots.pending, 1);
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
            ci_unknown: false,
        };
        let snapshot = build_dashboard_simple(vec![item], &config, "now", None, None);
        assert!(snapshot.bots.is_none());
        assert_eq!(snapshot.groups[0].id, "not-started");
        assert_eq!(snapshot.groups[0].items.len(), 1);
    }

    #[test]
    fn disabled_org_excluded_from_sections_and_tray() {
        let mut config = AppConfig::default();
        config.orgs[2].enabled = false;
        let labels = config.labels.clone();
        let item_enabled = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-tools".to_string(),
            repo: "agent-dashboard".to_string(),
            number: 1,
            title: "tools pr".to_string(),
            url: "https://example.com/1".to_string(),
            author: "human".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![labels.needs_input.clone()],
            ci: None,
            ci_unknown: false,
        };
        let item_disabled = RawItem {
            kind: ItemKind::Pr,
            org: "springfield-usa".to_string(),
            repo: "usa".to_string(),
            number: 2,
            title: "usa pr".to_string(),
            url: "https://example.com/2".to_string(),
            author: "human".to_string(),
            updated_at: "2026-10-10T12:00:00Z".to_string(),
            is_draft: false,
            labels: vec![labels.needs_input.clone()],
            ci: None,
            ci_unknown: false,
        };
        let snapshot = build_dashboard_simple(
            vec![item_enabled, item_disabled],
            &config,
            "now",
            None,
            None,
        );
        assert_eq!(snapshot.disabled_org_count, 1);
        assert_eq!(snapshot.org_sections.len(), 2);
        assert!(
            snapshot
                .org_sections
                .iter()
                .all(|s| s.org != "springfield-usa")
        );
        assert_eq!(snapshot.tray_title, "1");
    }
}

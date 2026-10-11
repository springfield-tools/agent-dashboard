use crate::model::{FixtureFile, RawItem};

const FIXTURE_JSON: &str = include_str!("../fixtures/ham-radio-open.json");

pub fn load_checked_in_items() -> Vec<RawItem> {
    let file: FixtureFile =
        serde_json::from_str(FIXTURE_JSON).expect("checked-in fixture JSON must parse");
    file.items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    #[test]
    fn fixture_matches_phase2_acceptance_counts() {
        let items = load_checked_in_items();
        assert_eq!(items.len(), 33);
        let snapshot =
            crate::model::build_dashboard_simple(items, &AppConfig::default(), "2026-10-10T20:00:00Z", None, None);
        let bots = snapshot.bots.expect("collapsed bot footer");
        assert_eq!(bots.green, 29);
        assert_eq!(bots.no_ci, 2);
        assert!(snapshot.tray_title.is_empty());
        let not_started = snapshot
            .groups
            .iter()
            .find(|g| g.id == "not-started")
            .expect("drafts");
        assert_eq!(not_started.items.len(), 2);
        assert!(!snapshot.groups.iter().any(|g| g.id == "ci-failing"));
        assert!(!snapshot.groups.iter().any(|g| g.id == "needs-input"));
    }
}

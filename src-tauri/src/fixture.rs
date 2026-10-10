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
    use crate::model::build_dashboard;

    #[test]
    fn fixture_matches_phase2_acceptance_counts() {
        let items = load_checked_in_items();
        assert_eq!(items.len(), 35);
        let snapshot = build_dashboard(items, &AppConfig::default(), "2026-10-10T20:00:00Z");
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
        let ci = snapshot
            .groups
            .iter()
            .find(|g| g.id == "ci-failing")
            .expect("ci failing");
        assert_eq!(ci.items.len(), 1);
        assert!(ci.items[0].chips.iter().any(|c| c.label == "needs-testing"));
        let input = snapshot
            .groups
            .iter()
            .find(|g| g.id == "needs-input")
            .expect("needs input");
        assert_eq!(input.items.len(), 1);
    }
}

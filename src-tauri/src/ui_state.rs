use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    /// `true` means the org section is collapsed in the popover.
    #[serde(default)]
    pub org_collapsed: HashMap<String, bool>,
    /// Popover type filter: `all`, `issue`, or `pr`.
    #[serde(default = "default_item_kind_filter")]
    pub item_kind_filter: String,
}

fn default_item_kind_filter() -> String {
    "all".to_string()
}

pub fn load_ui_state(path: &Path) -> UiState {
    if !path.exists() {
        return UiState::default();
    }
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => UiState::default(),
    }
}

pub fn save_ui_state(path: &Path, state: &UiState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create ui state dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| format!("write ui state: {e}"))
}

pub fn set_org_collapsed(state: &mut UiState, org: &str, collapsed: bool) {
    state.org_collapsed.insert(org.to_string(), collapsed);
}

pub fn set_item_kind_filter(state: &mut UiState, filter: &str) -> Result<(), String> {
    let normalized = filter.trim().to_ascii_lowercase();
    if normalized != "all" && normalized != "issue" && normalized != "pr" {
        return Err(format!("unknown item kind filter: {filter}"));
    }
    state.item_kind_filter = normalized;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_item_kind_filter_values() {
        let mut state = UiState::default();
        set_item_kind_filter(&mut state, "issue").expect("issue");
        assert_eq!(state.item_kind_filter, "issue");
        set_item_kind_filter(&mut state, "PR").expect("pr");
        assert_eq!(state.item_kind_filter, "pr");
    }

    #[test]
    fn rejects_unknown_filter() {
        let mut state = UiState::default();
        assert!(set_item_kind_filter(&mut state, "draft").is_err());
    }
}

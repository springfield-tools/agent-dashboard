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

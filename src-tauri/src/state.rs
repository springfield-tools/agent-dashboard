use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Utc;

use crate::config::AppConfig;
use crate::fixture::load_checked_in_items;
use crate::model::{build_dashboard, DashboardSnapshot};

pub struct AppState {
    pub config: AppConfig,
    pub config_path: PathBuf,
    pub config_error: Option<String>,
    pub dashboard: Mutex<DashboardSnapshot>,
}

impl AppState {
    pub fn new(config: AppConfig, config_path: PathBuf, config_error: Option<String>) -> Self {
        let items = load_checked_in_items();
        let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let dashboard = build_dashboard(items, &config, &fetched_at);
        Self {
            config,
            config_path,
            config_error,
            dashboard: Mutex::new(dashboard),
        }
    }

    pub fn reload_from_fixture(&self) {
        let items = load_checked_in_items();
        let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let snapshot = build_dashboard(items, &self.config, &fetched_at);
        if let Ok(mut guard) = self.dashboard.lock() {
            *guard = snapshot;
        }
    }
}

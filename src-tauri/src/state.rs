use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::config::AppConfig;
use crate::fixture::load_checked_in_items;
use crate::model::{build_dashboard, DashboardSnapshot};
use crate::etag_probe::EtagStore;
use crate::poll::PollRuntime;
use crate::snapshot::load_snapshot_file;

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub config_path: PathBuf,
    pub config_error: Mutex<Option<String>>,
    pub dashboard: Mutex<DashboardSnapshot>,
    pub fixture_mode: bool,
    pub auth_error: Mutex<Option<String>>,
    pub offline: Mutex<bool>,
    pub banner: Mutex<Option<String>>,
    pub poll_runtime: Arc<PollRuntime>,
    pub etag_store: EtagStore,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        config_path: PathBuf,
        config_error: Option<String>,
        fixture_mode: bool,
        poll_runtime: Arc<PollRuntime>,
        snapshot_path: Option<PathBuf>,
    ) -> Self {
        let dashboard = if fixture_mode {
            let items = load_checked_in_items();
            let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            build_dashboard(items, &config, &fetched_at)
        } else if let Some(path) = snapshot_path.as_ref() {
            load_snapshot_file(path).unwrap_or_else(|| {
                let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
                build_dashboard(vec![], &config, &fetched_at)
            })
        } else {
            let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            build_dashboard(vec![], &config, &fetched_at)
        };
        let etag_store = EtagStore::load(
            config_path
                .parent()
                .map(|p| crate::etag_probe::etag_store_path(p))
                .unwrap_or_else(|| config_path.with_extension("etag-store.json")),
        );
        Self {
            config: Mutex::new(config),
            config_path,
            config_error: Mutex::new(config_error),
            dashboard: Mutex::new(dashboard),
            fixture_mode,
            auth_error: Mutex::new(None),
            offline: Mutex::new(false),
            banner: Mutex::new(None),
            poll_runtime,
            etag_store,
        }
    }

    pub fn set_dashboard(&self, snapshot: DashboardSnapshot) {
        if let Ok(mut guard) = self.dashboard.lock() {
            *guard = snapshot;
        }
    }

    pub fn set_auth_error(&self, message: Option<String>) {
        if let Ok(mut guard) = self.auth_error.lock() {
            *guard = message;
        }
    }

    pub fn set_offline(&self, offline: bool) {
        if let Ok(mut guard) = self.offline.lock() {
            *guard = offline;
        }
    }

    pub fn set_banner(&self, message: Option<String>) {
        if let Ok(mut guard) = self.banner.lock() {
            *guard = message;
        }
    }

    pub fn reload_from_fixture(&self) {
        let config = self.config.lock().map(|c| c.clone()).unwrap_or_default();
        let items = load_checked_in_items();
        let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let snapshot = build_dashboard(items, &config, &fetched_at);
        self.set_dashboard(snapshot);
    }
}

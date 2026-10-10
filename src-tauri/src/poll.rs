use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::config::{save_config, AppConfig};
use crate::github::{fetch_all_orgs, GraphqlError};
use crate::keychain::load_token;
use crate::model::build_dashboard;
use crate::pause::{clear_expired_timed_pause, is_polling_paused, seconds_until_timed_resume};
use crate::snapshot::write_snapshot_file;
use crate::state::AppState;
use crate::tray;

pub struct PollRuntime {
    pub wake: Notify,
    in_flight: AtomicBool,
    failure_backoff_secs: Mutex<u64>,
}

impl PollRuntime {
    pub fn new() -> Self {
        Self {
            wake: Notify::new(),
            in_flight: AtomicBool::new(false),
            failure_backoff_secs: Mutex::new(0),
        }
    }
}

pub fn start_background_poll(app: AppHandle, state: Arc<AppState>, runtime: Arc<PollRuntime>) {
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("http client");

        loop {
            let interval_secs = {
                state
                    .config
                    .lock()
                    .map(|c| c.poll_interval_seconds)
                    .unwrap_or(30)
            };

            let extra_backoff = runtime
                .failure_backoff_secs
                .lock()
                .map(|g| *g)
                .unwrap_or(0);

            if !state.fixture_mode {
                let mut resumed_from_timed_pause = false;
                if let Ok(mut config) = state.config.lock() {
                    if clear_expired_timed_pause(&mut config.poll_pause) {
                        if let Err(e) = save_config(&state.config_path, &config) {
                            log::warn!("config: {e}");
                        } else {
                            resumed_from_timed_pause = true;
                        }
                    }
                }

                let paused = state
                    .config
                    .lock()
                    .map(|c| is_polling_paused(&c.poll_pause))
                    .unwrap_or(false);
                if !paused {
                    if resumed_from_timed_pause {
                        let _ = run_poll_cycle(&app, &state, &client, true).await;
                    } else {
                        let _ = run_poll_cycle(&app, &state, &client, false).await;
                    }
                } else {
                    #[cfg(target_os = "macos")]
                    tray::sync_tray_paused(&app, true);
                }
            }

            let paused = state
                .config
                .lock()
                .map(|c| is_polling_paused(&c.poll_pause))
                .unwrap_or(false);
            let sleep_secs = if paused {
                state
                    .config
                    .lock()
                    .ok()
                    .and_then(|c| seconds_until_timed_resume(&c.poll_pause))
                    .unwrap_or(interval_secs)
            } else {
                interval_secs.max(extra_backoff)
            };
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(sleep_secs)) => {},
                _ = runtime.wake.notified() => {},
            }
        }
    });
}

pub async fn run_poll_cycle(
    app: &AppHandle,
    state: &Arc<AppState>,
    client: &reqwest::Client,
    force: bool,
) -> Result<(), String> {
    if state.fixture_mode {
        return Ok(());
    }

    let paused = state
        .config
        .lock()
        .map(|c| is_polling_paused(&c.poll_pause))
        .unwrap_or(false);
    if paused && !force {
        return Ok(());
    }

    if state
        .poll_runtime
        .in_flight
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }

    let result = async {
        let token = load_token().map_err(|e| e.to_string())?;
        let Some(token) = token else {
            state.set_auth_error(Some("Add a GitHub token in Settings".to_string()));
            return Ok(());
        };

        let config = state
            .config
            .lock()
            .map(|c| c.clone())
            .map_err(|_| "config lock poisoned".to_string())?;

        let fetch = fetch_all_orgs(client, &token, &config)
            .await
            .map_err(|e| match e {
                GraphqlError::Unauthorized => {
                    state.set_auth_error(Some(
                        "GitHub rejected the token (401). Paste a new fine-grained PAT in Settings."
                            .to_string(),
                    ));
                    "unauthorized".to_string()
                }
                GraphqlError::Network(msg) => {
                    state.set_offline(true);
                    msg
                }
                GraphqlError::Http { status, body } => {
                    if status == 403 || status == 429 {
                        state.set_banner(Some(format!(
                            "GitHub rate limit ({status}). Will retry on the next interval."
                        )));
                    }
                    format!("http {status}: {body}")
                }
                GraphqlError::GraphQl(msg) => msg,
            })?;

        state.set_auth_error(None);
        state.set_offline(false);
        if let Some(w) = fetch.warning {
            state.set_banner(Some(w));
        } else {
            state.set_banner(None);
        }

        let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let snapshot = build_dashboard(fetch.items, &config, &fetched_at);
        state.set_dashboard(snapshot.clone());
        #[cfg(target_os = "macos")]
        tray::sync_tray_from_snapshot(app, &snapshot);

        if let Ok(dir) = app.path().app_config_dir() {
            let path = dir.join("snapshot.json");
            if let Err(e) = write_snapshot_file(&path, &snapshot) {
                log::warn!("snapshot: {e}");
            }
        }

        if let Some(rl) = fetch.rate_limit {
            if rl.remaining < 500 {
                state.set_banner(Some(format!(
                    "GraphQL budget low ({rl.remaining} remaining until {rl.reset_at})"
                )));
            }
        }

        let _ = app.emit("dashboard-updated", ());
        Ok(())
    }
    .await;

    state.poll_runtime.in_flight.store(false, Ordering::SeqCst);

    match result {
        Ok(()) => {
            if let Ok(mut guard) = state.poll_runtime.failure_backoff_secs.lock() {
                *guard = 0;
            }
            Ok(())
        }
        Err(msg) => {
            if msg != "unauthorized" {
                log::warn!("poll failed: {msg}");
                if let Ok(mut guard) = state.poll_runtime.failure_backoff_secs.lock() {
                    let next = if *guard == 0 {
                        30
                    } else {
                        (*guard * 2).min(300)
                    };
                    *guard = next;
                }
            }
            Err(msg)
        }
    }
}

pub fn update_config_and_save(state: &AppState, config: AppConfig) -> Result<(), String> {
    save_config(&state.config_path, &config)?;
    if let Ok(mut guard) = state.config.lock() {
        *guard = config;
    }
    state.poll_runtime.wake.notify_one();
    Ok(())
}

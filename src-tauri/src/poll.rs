use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter, Manager, Runtime};
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
    rate_limit_until: Mutex<Option<DateTime<Utc>>>,
}

impl PollRuntime {
    pub fn new() -> Self {
        Self {
            wake: Notify::new(),
            in_flight: AtomicBool::new(false),
            failure_backoff_secs: Mutex::new(0),
            rate_limit_until: Mutex::new(None),
        }
    }
}

fn apply_jitter(base_secs: u64) -> u64 {
    if base_secs == 0 {
        return 0;
    }
    let jitter = (Utc::now().timestamp_subsec_nanos() as u64) % (base_secs / 4 + 1);
    base_secs + jitter
}

fn rate_limit_wait_secs(runtime: &PollRuntime) -> u64 {
    let guard = runtime.rate_limit_until.lock().ok();
    let Some(until) = guard.and_then(|g| *g) else {
        return 0;
    };
    let remaining = (until - Utc::now()).num_seconds();
    if remaining <= 0 {
        0
    } else {
        remaining as u64
    }
}

fn set_rate_limit_until(runtime: &PollRuntime, retry_after_secs: Option<u64>, reset_at: Option<&str>) {
    let until = if let Some(secs) = retry_after_secs {
        Utc::now() + chrono::Duration::seconds(secs as i64)
    } else if let Some(reset) = reset_at {
        DateTime::parse_from_rfc3339(reset)
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now() + chrono::Duration::minutes(5))
    } else {
        Utc::now() + chrono::Duration::minutes(5)
    };
    if let Ok(mut guard) = runtime.rate_limit_until.lock() {
        *guard = Some(until);
    }
}

async fn sleep_until_deadline(
    runtime: &Arc<PollRuntime>,
    deadline: DateTime<Utc>,
) -> bool {
    while Utc::now() < deadline {
        let remaining = (deadline - Utc::now()).num_seconds().max(1) as u64;
        let slice = remaining.min(5);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(slice)) => {},
            _ = runtime.wake.notified() => return true,
        }
    }
    false
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
            let rate_wait = rate_limit_wait_secs(&runtime);

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
                interval_secs.max(extra_backoff).max(rate_wait)
            };
            let sleep_secs = apply_jitter(sleep_secs);
            let deadline = Utc::now() + chrono::Duration::seconds(sleep_secs as i64);
            let _ = sleep_until_deadline(&runtime, deadline).await;
        }
    });
}

pub async fn run_poll_cycle<R: Runtime>(
    app: &AppHandle<R>,
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
                GraphqlError::RateLimited {
                    retry_after_secs,
                    reset_at,
                    message,
                } => {
                    set_rate_limit_until(
                        &state.poll_runtime,
                        retry_after_secs,
                        reset_at.as_deref(),
                    );
                    state.set_banner(Some(format!(
                        "GitHub rate limit. Will retry after the reset window."
                    )));
                    log::warn!("rate limited: {message}");
                    "rate_limited".to_string()
                }
                GraphqlError::Forbidden { status, message } => {
                    state.set_auth_error(Some(format!(
                        "GitHub denied access ({status}). Check fine-grained PAT permissions, org approval, and repository access."
                    )));
                    log::warn!("forbidden: {message}");
                    "forbidden".to_string()
                }
                GraphqlError::GraphQl(msg) => msg,
            })?;

        state.set_auth_error(None);
        state.set_offline(false);
        if let Ok(mut guard) = state.poll_runtime.rate_limit_until.lock() {
            *guard = None;
        }
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
            if rl.remaining < 100 {
                set_rate_limit_until(
                    &state.poll_runtime,
                    None,
                    Some(rl.reset_at.as_str()),
                );
                state.set_banner(Some(format!(
                    "GraphQL budget very low ({} remaining until {}). Polling pauses until reset.",
                    rl.remaining,
                    rl.reset_at
                )));
            } else if rl.remaining < 500 {
                state.set_banner(Some(format!(
                    "GraphQL budget low ({} remaining until {})",
                    rl.remaining,
                    rl.reset_at
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
            if msg != "unauthorized" && msg != "forbidden" && msg != "rate_limited" {
                log::warn!("poll failed: {msg}");
                if let Ok(mut guard) = state.poll_runtime.failure_backoff_secs.lock() {
                    let next = if *guard == 0 {
                        30
                    } else {
                        (*guard * 2).min(300)
                    };
                    *guard = apply_jitter(next);
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

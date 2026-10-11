use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::Notify;

use crate::backoff::{wait_seconds_for_failure, FailureKind};
use crate::brakes::{apply_brakes, brake_slow_interval_for_cost, LOW_REMAINING_STOP};
use crate::config::{save_config, AppConfig};
use crate::credential::{clear_gh_session_cache, obtain_github_token, uses_gh_credential};
use crate::etag_probe::{evaluate_probe, should_skip_graphql_after_org_probes};
use crate::github::{fetch_all_orgs, GraphqlError};
use crate::idle;
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
    gh_token_cache: Mutex<Option<String>>,
    last_poll_cost: Mutex<i32>,
    brake_slow_interval: Mutex<u64>,
    last_rate_remaining: Mutex<Option<i32>>,
    last_rate_reset_at: Mutex<Option<String>>,
    pub idle_pause_active: AtomicBool,
    /// Consumed by the poll loop for a single post-wake refresh.
    pub pending_wake: AtomicBool,
}

impl PollRuntime {
    pub fn new() -> Self {
        Self {
            wake: Notify::new(),
            in_flight: AtomicBool::new(false),
            failure_backoff_secs: Mutex::new(0),
            rate_limit_until: Mutex::new(None),
            gh_token_cache: Mutex::new(None),
            last_poll_cost: Mutex::new(0),
            brake_slow_interval: Mutex::new(0),
            last_rate_remaining: Mutex::new(None),
            last_rate_reset_at: Mutex::new(None),
            idle_pause_active: AtomicBool::new(false),
            pending_wake: AtomicBool::new(false),
        }
    }

    pub fn is_idle_paused(&self) -> bool {
        self.idle_pause_active.load(Ordering::SeqCst)
    }

    pub fn rate_limit_is_active(&self) -> bool {
        rate_limit_wait_secs(self) > 0
    }
}

fn apply_jitter(base_secs: u64) -> u64 {
    if base_secs == 0 {
        return 0;
    }
    let nanos = Utc::now().timestamp_subsec_nanos();
    let jitter = (nanos as u64) % (base_secs / 4 + 1);
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

fn set_primary_rate_limit_until(runtime: &PollRuntime, reset_at: &str, nanos: u32) {
    let until = DateTime::parse_from_rfc3339(reset_at)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now() + chrono::Duration::minutes(5));
    let extra = 1 + (nanos as u64 % 5);
    let until = until + chrono::Duration::seconds(extra as i64);
    if let Ok(mut guard) = runtime.rate_limit_until.lock() {
        *guard = Some(until);
    }
}

fn set_secondary_rate_limit_until(runtime: &PollRuntime, retry_after_secs: u64, nanos: u32) {
    let wait = crate::backoff::jitter_seconds(retry_after_secs, nanos);
    let until = Utc::now() + chrono::Duration::seconds(wait as i64);
    if let Ok(mut guard) = runtime.rate_limit_until.lock() {
        *guard = Some(until);
    }
}

async fn sleep_until_deadline(runtime: &Arc<PollRuntime>, deadline: DateTime<Utc>) -> bool {
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

#[derive(PartialEq)]
enum IdleTransition {
    None,
    EnteredIdle,
    ResumedFromIdle,
}

fn update_idle_pause<R: Runtime>(app: &AppHandle<R>, state: &Arc<AppState>) -> IdleTransition {
    let config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    if !config.pause_when_idle {
        if state.poll_runtime.idle_pause_active.swap(false, Ordering::SeqCst) {
            #[cfg(target_os = "macos")]
            tray::sync_tray_from_app(app);
        }
        return IdleTransition::None;
    }
    if is_polling_paused(&config.poll_pause) {
        return IdleTransition::None;
    }
    let idle_secs = idle::seconds_since_last_input();
    if idle_secs >= config.idle_pause_after_seconds as f64 {
        let was = state.poll_runtime.idle_pause_active.swap(true, Ordering::SeqCst);
        if !was {
            #[cfg(target_os = "macos")]
            tray::sync_tray_paused_with_tooltip(app, "Polling paused while idle");
            return IdleTransition::EnteredIdle;
        }
        return IdleTransition::None;
    }
    if state.poll_runtime.idle_pause_active.swap(false, Ordering::SeqCst) {
        #[cfg(target_os = "macos")]
        tray::sync_tray_from_app(app);
        return IdleTransition::ResumedFromIdle;
    }
    IdleTransition::None
}

fn spawn_idle_monitor<R: Runtime>(app: AppHandle<R>, state: Arc<AppState>, runtime: Arc<PollRuntime>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let pause_when_idle = state
                .config
                .lock()
                .map(|c| c.pause_when_idle)
                .unwrap_or(false);
            if !pause_when_idle {
                continue;
            }
            if state
                .config
                .lock()
                .map(|c| is_polling_paused(&c.poll_pause))
                .unwrap_or(false)
            {
                continue;
            }
            let transition = update_idle_pause(&app, &state);
            if transition == IdleTransition::ResumedFromIdle && !runtime.rate_limit_is_active() {
                runtime.pending_wake.store(true, Ordering::SeqCst);
                runtime.wake.notify_one();
            }
        }
    });
}

pub fn start_background_poll(app: AppHandle, state: Arc<AppState>, runtime: Arc<PollRuntime>) {
    spawn_idle_monitor(app.clone(), state.clone(), runtime.clone());
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

            let pause_when_idle = state
                .config
                .lock()
                .map(|c| c.pause_when_idle)
                .unwrap_or(false);
            let idle_transition = if pause_when_idle {
                IdleTransition::None
            } else {
                update_idle_pause(&app, &state)
            };
            let wake_refresh = state.poll_runtime.pending_wake.swap(false, Ordering::SeqCst);

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

                let user_paused = state
                    .config
                    .lock()
                    .map(|c| is_polling_paused(&c.poll_pause))
                    .unwrap_or(false);
                let idle_paused = state.poll_runtime.is_idle_paused();
                let rate_blocked = state.poll_runtime.rate_limit_is_active();

                if !user_paused && !rate_blocked {
                    let force = resumed_from_timed_pause
                        || idle_transition == IdleTransition::ResumedFromIdle;
                    let bypass_idle = wake_refresh;
                    let should_poll = if idle_paused {
                        bypass_idle || force
                    } else {
                        true
                    };
                    if should_poll {
                        let _ = run_poll_cycle(
                            &app,
                            &state,
                            &client,
                            force,
                            bypass_idle,
                        )
                        .await;
                    }
                } else if user_paused || idle_paused {
                    #[cfg(target_os = "macos")]
                    tray::sync_tray_paused(&app, true);
                }
            }

            let user_paused = state
                .config
                .lock()
                .map(|c| is_polling_paused(&c.poll_pause))
                .unwrap_or(false);
            let idle_paused = state.poll_runtime.is_idle_paused();

            let brake_slow = state
                .poll_runtime
                .brake_slow_interval
                .lock()
                .map(|g| *g)
                .unwrap_or(0);
            let last_cost = state
                .poll_runtime
                .last_poll_cost
                .lock()
                .map(|g| *g)
                .unwrap_or(0);
            let rate_remaining = state
                .poll_runtime
                .last_rate_remaining
                .lock()
                .ok()
                .and_then(|g| *g);
            let rate_reset = state
                .poll_runtime
                .last_rate_reset_at
                .lock()
                .ok()
                .and_then(|g| g.clone());

            let brakes = apply_brakes(
                interval_secs,
                last_cost,
                rate_remaining,
                rate_reset.as_deref(),
                brake_slow,
            );
            if brakes.log_high_cost {
                log::warn!("GraphQL poll cost {last_cost} exceeded warning threshold");
            }
            if let Some(banner) = brakes.banner {
                if rate_remaining.map_or(false, |r| r < LOW_REMAINING_STOP) {
                    state.set_banner(Some(banner));
                } else if state.banner.lock().map(|g| g.is_none()).unwrap_or(true) {
                    state.set_banner(Some(banner));
                }
            }

            let extra_backoff = state
                .poll_runtime
                .failure_backoff_secs
                .lock()
                .map(|g| *g)
                .unwrap_or(0);
            let rate_wait = rate_limit_wait_secs(&state.poll_runtime);

            let sleep_secs = if user_paused || idle_paused {
                state
                    .config
                    .lock()
                    .ok()
                    .and_then(|c| seconds_until_timed_resume(&c.poll_pause))
                    .unwrap_or(interval_secs)
            } else {
                brakes.sleep_secs.max(extra_backoff).max(rate_wait)
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
    bypass_idle: bool,
) -> Result<(), String> {
    if state.fixture_mode {
        return Ok(());
    }

    let user_paused = state
        .config
        .lock()
        .map(|c| is_polling_paused(&c.poll_pause))
        .unwrap_or(false);
    if user_paused && !force {
        return Ok(());
    }
    if state.poll_runtime.is_idle_paused() && !force && !bypass_idle {
        return Ok(());
    }
    if state.poll_runtime.rate_limit_is_active() && !force {
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
        let config = state
            .config
            .lock()
            .map(|c| c.clone())
            .map_err(|_| "config lock poisoned".to_string())?;

        let token = match obtain_github_token(
            &config,
            &state.poll_runtime.gh_token_cache,
            false,
        ) {
            Ok(token) => token,
            Err(Some(message)) => {
                state.set_auth_error(Some(message));
                return Ok(());
            }
            Err(None) => return Ok(()),
        };

        let last_fetched = state
            .dashboard
            .lock()
            .map(|d| d.fetched_at.clone())
            .unwrap_or_default();

        if config.use_etag_probe {
            let mut probe_outcomes: Vec<Result<crate::etag_probe::ProbeDecision, String>> =
                Vec::new();
            let mut combined_banner = None;
            for org in &config.orgs {
                let probe = match evaluate_probe(
                    client,
                    &token,
                    &config,
                    org,
                    &state.etag_store,
                    &last_fetched,
                    "https://api.github.com",
                )
                .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!("etag probe failed, falling back to GraphQL: {e}");
                        probe_outcomes.push(Err(e));
                        break;
                    }
                };
                probe_outcomes.push(Ok(probe.clone()));
                if !probe.skip_graphql {
                    break;
                }
                combined_banner = probe.banner.or(combined_banner);
            }
            let skip_all = should_skip_graphql_after_org_probes(
                config.orgs.len(),
                &probe_outcomes,
            );
            if skip_all {
                if let Some(msg) = combined_banner {
                    state.set_banner(Some(msg));
                }
                if let Ok(mut guard) = state.dashboard.lock() {
                    guard.stale = true;
                }
                let _ = app.emit("dashboard-updated", ());
                return Ok(());
            }
        }

        let fetch = fetch_all_orgs(client, &token, &config)
            .await
            .map_err(|e| match e {
                GraphqlError::Unauthorized => {
                    if uses_gh_credential(&config) {
                        clear_gh_session_cache(&state.poll_runtime.gh_token_cache);
                        state.set_auth_error(Some(
                            "GitHub rejected the gh token (401). Run `gh auth login` or refresh scopes; the next poll will call `gh auth token` again."
                                .to_string(),
                        ));
                    } else {
                        state.set_auth_error(Some(
                            "GitHub rejected the token (401). Paste a new fine-grained PAT in Settings."
                                .to_string(),
                        ));
                    }
                    "unauthorized".to_string()
                }
                GraphqlError::Network(msg) => {
                    state.set_offline(true);
                    msg
                }
                GraphqlError::RateLimited {
                    retry_after_secs,
                    reset_at,
                    primary_exhausted,
                    message,
                } => {
                    let nanos = Utc::now().timestamp_subsec_nanos();
                    if primary_exhausted {
                        if let Some(ref reset) = reset_at {
                            set_primary_rate_limit_until(
                                &state.poll_runtime,
                                reset,
                                nanos,
                            );
                        } else {
                            set_secondary_rate_limit_until(
                                &state.poll_runtime,
                                300,
                                nanos,
                            );
                        }
                    } else if let Some(retry) = retry_after_secs {
                        set_secondary_rate_limit_until(
                            &state.poll_runtime,
                            retry,
                            nanos,
                        );
                    } else if let Some(ref reset) = reset_at {
                        set_primary_rate_limit_until(&state.poll_runtime, reset, nanos);
                    } else {
                        set_secondary_rate_limit_until(&state.poll_runtime, 60, nanos);
                    }
                    state.set_banner(Some(
                        "GitHub rate limit. Will retry after the reset window.".to_string(),
                    ));
                    log::warn!("rate limited: {message}");
                    "rate_limited".to_string()
                }
                GraphqlError::Forbidden { status, message } => {
                    let hint = if uses_gh_credential(&config) {
                        "Check `gh auth status`, org access, and repository scopes."
                    } else {
                        "Check fine-grained PAT permissions, org approval, and repository access."
                    };
                    state.set_auth_error(Some(format!(
                        "GitHub denied access ({status}). {hint}"
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
        if fetch.items.is_empty() {
            if let Some(hint) = fetch.empty_hint {
                state.set_banner(Some(hint));
            } else if let Some(w) = fetch.warning {
                state.set_banner(Some(w));
            } else {
                state.set_banner(Some(
                    "GitHub returned no open items. Check token permissions and org config."
                        .to_string(),
                ));
            }
        } else if let Some(w) = fetch.warning {
            state.set_banner(Some(w));
        } else {
            state.set_banner(None);
        }

        let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let (poll_cost, poll_remaining) = fetch
            .rate_limit
            .as_ref()
            .map(|rl| (Some(rl.cost), Some(rl.remaining)))
            .unwrap_or((None, None));
        let snapshot = build_dashboard(
            fetch.items,
            &config,
            &fetched_at,
            poll_cost,
            poll_remaining,
        );
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
            if let Ok(mut guard) = state.poll_runtime.last_poll_cost.lock() {
                *guard = rl.cost;
            }
            if let Ok(mut guard) = state.poll_runtime.last_rate_remaining.lock() {
                *guard = Some(rl.remaining);
            }
            if let Ok(mut guard) = state.poll_runtime.last_rate_reset_at.lock() {
                *guard = Some(rl.reset_at.clone());
            }
            let slow = brake_slow_interval_for_cost(rl.cost, config.poll_interval_seconds);
            if let Ok(mut guard) = state.poll_runtime.brake_slow_interval.lock() {
                *guard = slow;
            }
            if rl.remaining < LOW_REMAINING_STOP {
                let nanos = Utc::now().timestamp_subsec_nanos();
                set_primary_rate_limit_until(
                    &state.poll_runtime,
                    rl.reset_at.as_str(),
                    nanos,
                );
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
                let nanos = Utc::now().timestamp_subsec_nanos();
                let wait = wait_seconds_for_failure(
                    FailureKind::TransientHttp,
                    state
                        .poll_runtime
                        .failure_backoff_secs
                        .lock()
                        .map(|g| *g)
                        .unwrap_or(0),
                    None,
                    None,
                    nanos,
                );
                if let Ok(mut guard) = state.poll_runtime.failure_backoff_secs.lock() {
                    *guard = wait;
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

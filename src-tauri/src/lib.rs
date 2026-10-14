mod backoff;
mod brakes;
mod config;
mod credential;
mod etag_probe;
mod fixture;
mod github;
mod idle;
mod keychain;
mod model;
mod pause;
mod poll;
mod popover_anchor;
mod power;
mod row_actions;
mod row_writes;
mod snapshot;
mod state;
mod ui_state;
mod write_state;

#[cfg(target_os = "macos")]
mod tray;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::config::{load_config, validate_poll_interval};
use crate::config::OrgConfig;
use crate::credential::{
    credential_is_ready, credential_source_label, org_credential_is_ready, org_credential_source_label,
};
use crate::keychain::{delete_token_for_org, store_token_for_org};
use crate::ui_state::{
    load_ui_state, save_ui_state, set_item_kind_filter, set_org_collapsed, UiState,
};
use crate::model::DashboardSnapshot;
use crate::pause::{
    is_polling_paused, pause_for_one_hour_value, pause_manual_value,
    pause_until_tomorrow_local_value, pause_tooltip,
};
use crate::poll::{
    rebuild_dashboard_after_org_disabled, run_poll_cycle, save_config_in_memory,
    start_background_poll, update_config_and_save, PollRuntime,
};
use crate::row_actions::RowActionKind;
use crate::row_writes::{
    execute_row_action, preview_row_action, retry_partial_write, undo_row_action, write_ui_state,
};
use crate::state::AppState;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OrgAuthStatus {
    name: String,
    enabled: bool,
    credential_source: String,
    ready: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigStatus {
    config_path: String,
    config_error: Option<String>,
    poll_interval_seconds: u64,
    bot_filter: String,
    fixture_mode: bool,
    polling_paused: bool,
    pause_tooltip: String,
    has_github_token: bool,
    credential_source: String,
    orgs: Vec<OrgConfig>,
    org_auth: Vec<OrgAuthStatus>,
    auth_error: Option<String>,
    offline: bool,
    banner: Option<String>,
}

#[tauri::command]
fn dashboard_snapshot(state: tauri::State<'_, Arc<AppState>>) -> Result<DashboardSnapshot, String> {
    let config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    let snapshot = state
        .dashboard
        .lock()
        .map(|guard| guard.clone())
        .map_err(|_| "dashboard lock poisoned".to_string())?;
    let write = state
        .write_state
        .lock()
        .map_err(|_| "write lock poisoned".to_string())?;
    Ok(crate::write_state::merged_snapshot(
        &snapshot,
        &config,
        &write.optimistic_labels,
        &write.pending,
    ))
}

#[tauri::command]
fn config_status(state: tauri::State<'_, Arc<AppState>>) -> ConfigStatus {
    let config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    let bot_filter = match config.filters.bots {
        crate::config::BotFilterMode::Hide => "hide",
        crate::config::BotFilterMode::Collapse => "collapse",
        crate::config::BotFilterMode::Show => "show",
    };
    let config_error = state
        .config_error
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let auth_error = state
        .auth_error
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let offline = state.offline.lock().map(|g| *g).unwrap_or(false);
    let banner = state.banner.lock().ok().and_then(|g| g.clone());
    let user_paused = is_polling_paused(&config.poll_pause);
    let idle_paused = state.poll_runtime.is_idle_paused();
    let polling_paused = user_paused || idle_paused;
    let pause_tooltip = if idle_paused && !user_paused {
        "Polling paused while idle".to_string()
    } else {
        pause_tooltip(&config.poll_pause)
    };
    let org_auth = config
        .orgs
        .iter()
        .map(|org| OrgAuthStatus {
            name: org.name.clone(),
            enabled: org.enabled,
            credential_source: if org.enabled {
                org_credential_source_label(org, &config).to_string()
            } else {
                "off".to_string()
            },
            ready: if org.enabled {
                org_credential_is_ready(org, &config)
            } else {
                true
            },
        })
        .collect();

    ConfigStatus {
        config_path: state.config_path.display().to_string(),
        config_error,
        poll_interval_seconds: config.poll_interval_seconds,
        bot_filter: bot_filter.to_string(),
        fixture_mode: state.fixture_mode,
        polling_paused,
        pause_tooltip,
        has_github_token: credential_is_ready(&config),
        credential_source: credential_source_label(&config).to_string(),
        orgs: config.orgs.clone(),
        org_auth,
        auth_error,
        offline,
        banner,
    }
}

#[tauri::command]
fn store_github_token(org: String, token: String) -> Result<(), String> {
    store_token_for_org(&org, &token).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_github_token(org: String) -> Result<(), String> {
    delete_token_for_org(&org).map_err(|e| e.to_string())
}

#[tauri::command]
fn ui_state_cmd(state: tauri::State<'_, Arc<AppState>>) -> UiState {
    state.ui_state.lock().map(|g| g.clone()).unwrap_or_default()
}

#[tauri::command]
fn set_org_section_collapsed(
    state: tauri::State<'_, Arc<AppState>>,
    org: String,
    collapsed: bool,
) -> Result<(), String> {
    let mut guard = state.ui_state.lock().map_err(|_| "ui state lock")?;
    set_org_collapsed(&mut *guard, &org, collapsed);
    save_ui_state(&state.ui_state_path, &*guard)?;
    Ok(())
}

#[tauri::command]
fn set_item_kind_filter_cmd(
    state: tauri::State<'_, Arc<AppState>>,
    filter: String,
) -> Result<(), String> {
    let mut guard = state.ui_state.lock().map_err(|_| "ui state lock")?;
    set_item_kind_filter(&mut *guard, &filter)?;
    save_ui_state(&state.ui_state_path, &*guard)?;
    Ok(())
}

#[tauri::command]
fn set_poll_interval(
    state: tauri::State<'_, Arc<AppState>>,
    seconds: u64,
) -> Result<u64, String> {
    validate_poll_interval(seconds).map_err(|e| e.to_string())?;
    let mut config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    config.poll_interval_seconds = seconds;
    update_config_and_save(state.inner(), config)?;
    Ok(seconds)
}

#[tauri::command]
fn pause_polling(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    mode: String,
) -> Result<(), String> {
    let value = match mode.as_str() {
        "manual" => pause_manual_value(),
        "hour" => pause_for_one_hour_value(),
        "tomorrow" => pause_until_tomorrow_local_value(),
        other => return Err(format!("unknown pause mode: {other}")),
    };
    let mut config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    config.poll_pause = Some(value);
    update_config_and_save(state.inner(), config)?;
    #[cfg(target_os = "macos")]
    tray::sync_tray_paused(&app, true);
    let _ = app.emit("dashboard-updated", ());
    Ok(())
}

#[tauri::command]
fn resume_polling(app: AppHandle, state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    config.poll_pause = None;
    update_config_and_save(state.inner(), config)?;
    #[cfg(target_os = "macos")]
    tray::sync_tray_paused(&app, false);
    let _ = app.emit("dashboard-updated", ());
    Ok(())
}

#[tauri::command]
fn preview_row_action_cmd(
    state: tauri::State<'_, Arc<AppState>>,
    group_id: String,
    org: String,
    repo: String,
    number: u32,
    action: RowActionKind,
    note: String,
) -> Result<crate::row_actions::RowActionPreview, String> {
    preview_row_action(state.inner().as_ref(), &group_id, &org, &repo, number, action, &note)
}

#[tauri::command]
async fn confirm_row_action(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    group_id: String,
    org: String,
    repo: String,
    number: u32,
    action: RowActionKind,
    note: String,
) -> Result<(), String> {
    execute_row_action(app, state.inner().clone(), group_id, org, repo, number, action, note).await
}

#[tauri::command]
async fn undo_row_action_cmd(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    undo_row_action(app, state.inner().clone()).await
}

#[tauri::command]
async fn retry_partial_write_cmd(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    retry_partial_write(app, state.inner().clone()).await
}

#[tauri::command]
fn write_ui_state_cmd(state: tauri::State<'_, Arc<AppState>>) -> crate::write_state::WriteUiState {
    write_ui_state(state.inner())
}

#[tauri::command]
async fn set_org_enabled(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    org: String,
    enabled: bool,
) -> Result<(), String> {
    let mut config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
    let entry = config
        .orgs
        .iter_mut()
        .find(|o| o.name == org)
        .ok_or_else(|| format!("Unknown org \"{org}\" in config."))?;
    if entry.enabled == enabled {
        return Ok(());
    }
    entry.enabled = enabled;
    save_config_in_memory(state.inner(), &config)?;
    if enabled {
        let runtime = &state.poll_runtime;
        if runtime
            .in_flight
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            runtime.pending_wake.store(true, std::sync::atomic::Ordering::SeqCst);
            runtime.wake.notify_one();
        } else {
            let app = app.clone();
            let state = state.inner().clone();
            tauri::async_runtime::spawn(async move {
                let client = reqwest::Client::new();
                let _ = run_poll_cycle(&app, &state, &client, true, true).await;
            });
        }
    } else {
        rebuild_dashboard_after_org_disabled(state.inner())?;
        let snap = state
            .dashboard
            .lock()
            .map(|d| d.clone())
            .unwrap_or_default();
        crate::poll::persist_snapshot_file(state.inner(), &snap);
        state.set_auth_error(None);
        #[cfg(target_os = "macos")]
        crate::tray::sync_tray_from_snapshot(&app, &snap);
        let _ = app.emit("dashboard-updated", ());
    }
    Ok(())
}

#[tauri::command]
fn refresh_now(app: AppHandle, state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    tray::trigger_refresh(&app, true);
    #[cfg(not(target_os = "macos"))]
    {
        let app = app.clone();
        let state = state.inner().clone();
        tauri::async_runtime::spawn(async move {
            let client = reqwest::Client::new();
            let _ = poll::run_poll_cycle(&app, &state, &client, true, true).await;
        });
    }
    Ok(())
}

pub fn run(fixture_mode: bool) {
    let mut builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            dashboard_snapshot,
            config_status,
            store_github_token,
            clear_github_token,
            set_poll_interval,
            pause_polling,
            resume_polling,
            refresh_now,
            preview_row_action_cmd,
            confirm_row_action,
            undo_row_action_cmd,
            retry_partial_write_cmd,
            write_ui_state_cmd,
            ui_state_cmd,
            set_org_section_collapsed,
            set_item_kind_filter_cmd,
            set_org_enabled,
        ]);

    builder = builder.setup(move |app| {
        let config_dir = app
            .path()
            .app_config_dir()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let config_path = config_dir.join("config.toml");
        let snapshot_path = config_dir.join("snapshot.json");
        let (config, config_error) = load_config(&config_path);
        let poll_runtime = Arc::new(PollRuntime::new());
        let app_state = Arc::new(
            AppState::new(
                config,
                config_path,
                config_error,
                fixture_mode,
                poll_runtime.clone(),
                if fixture_mode {
                    None
                } else {
                    Some(snapshot_path)
                },
            ),
        );
        app.manage(app_state.clone());

        #[cfg(target_os = "macos")]
        {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::setup(app.handle())?;
            if !fixture_mode {
                power::install_wake_listener(
                    app.handle().clone(),
                    app_state.clone(),
                    poll_runtime.clone(),
                );
                start_background_poll(app.handle().clone(), app_state.clone(), poll_runtime);
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            if !fixture_mode {
                start_background_poll(app.handle().clone(), app_state.clone(), poll_runtime);
            }
            if let Some(window) = app.get_webview_window("popover") {
                window.show()?;
            }
        }

        Ok(())
    });

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

mod config;
mod fixture;
mod github;
mod keychain;
mod model;
mod pause;
mod poll;
mod popover_anchor;
mod snapshot;
mod state;

#[cfg(target_os = "macos")]
mod tray;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::config::{load_config, validate_poll_interval};
use crate::keychain::{delete_token, has_token, store_token};
use crate::model::DashboardSnapshot;
use crate::pause::{
    is_polling_paused, pause_for_one_hour_value, pause_manual_value,
    pause_until_tomorrow_local_value, pause_tooltip,
};
use crate::poll::{start_background_poll, update_config_and_save, PollRuntime};
use crate::state::AppState;

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
    auth_error: Option<String>,
    offline: bool,
    banner: Option<String>,
}

#[tauri::command]
fn dashboard_snapshot(state: tauri::State<'_, Arc<AppState>>) -> Result<DashboardSnapshot, String> {
    state
        .dashboard
        .lock()
        .map(|guard| guard.clone())
        .map_err(|_| "dashboard lock poisoned".to_string())
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
    let polling_paused = is_polling_paused(&config.poll_pause);
    ConfigStatus {
        config_path: state.config_path.display().to_string(),
        config_error,
        poll_interval_seconds: config.poll_interval_seconds,
        bot_filter: bot_filter.to_string(),
        fixture_mode: state.fixture_mode,
        polling_paused,
        pause_tooltip: pause_tooltip(&config.poll_pause),
        has_github_token: has_token(),
        auth_error,
        offline,
        banner,
    }
}

#[tauri::command]
fn store_github_token(token: String) -> Result<(), String> {
    store_token(&token).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_github_token() -> Result<(), String> {
    delete_token().map_err(|e| e.to_string())
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
fn refresh_now(app: AppHandle, state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    tray::trigger_refresh(&app, true);
    #[cfg(not(target_os = "macos"))]
    {
        let app = app.clone();
        let state = state.inner().clone();
        tauri::async_runtime::spawn(async move {
            let client = reqwest::Client::new();
            let _ = poll::run_poll_cycle(&app, &state, &client, true).await;
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

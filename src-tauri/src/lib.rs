mod config;
mod fixture;
mod model;
mod popover_anchor;
mod state;

#[cfg(target_os = "macos")]
mod tray;

use tauri::Manager;

use crate::config::load_config;
use crate::model::DashboardSnapshot;
use crate::state::AppState;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigStatus {
    config_path: String,
    config_error: Option<String>,
    poll_interval_seconds: u64,
    bot_filter: String,
}

#[tauri::command]
fn dashboard_snapshot(state: tauri::State<'_, AppState>) -> Result<DashboardSnapshot, String> {
    state
        .dashboard
        .lock()
        .map(|guard| guard.clone())
        .map_err(|_| "dashboard lock poisoned".to_string())
}

#[tauri::command]
fn config_status(state: tauri::State<'_, AppState>) -> ConfigStatus {
    let bot_filter = match state.config.filters.bots {
        crate::config::BotFilterMode::Hide => "hide",
        crate::config::BotFilterMode::Collapse => "collapse",
        crate::config::BotFilterMode::Show => "show",
    };
    ConfigStatus {
        config_path: state.config_path.display().to_string(),
        config_error: state.config_error.clone(),
        poll_interval_seconds: state.config.poll_interval_seconds,
        bot_filter: bot_filter.to_string(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![dashboard_snapshot, config_status]);

    builder = builder.setup(|app| {
        let config_dir = app
            .path()
            .app_config_dir()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let config_path = config_dir.join("config.toml");
        let (config, config_error) = load_config(&config_path);
        let app_state = AppState::new(config, config_path, config_error);
        app.manage(app_state);

        #[cfg(target_os = "macos")]
        {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::setup(app.handle())?;
        }

        #[cfg(not(target_os = "macos"))]
        {
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

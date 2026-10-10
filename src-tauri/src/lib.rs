mod groups;

#[cfg(target_os = "macos")]
mod tray;

const PLAN_URL: &str =
    "https://github.com/springfield-tools/agent-dashboard/blob/main/docs/PLAN.md";

#[tauri::command]
fn placeholder_group_count() -> usize {
    groups::group_count()
}

#[tauri::command]
fn open_plan_url(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    app.opener()
        .open_url(PLAN_URL, None::<&str>)
        .map_err(|error| error.to_string())
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
        .invoke_handler(tauri::generate_handler![placeholder_group_count, open_plan_url]);

    #[cfg(target_os = "macos")]
    {
        builder = builder.setup(|app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory)?;
            tray::setup(app.handle())?;
            Ok(())
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.setup(|app| {
            if let Some(window) = app.get_webview_window("popover") {
                window.show()?;
            }
            Ok(())
        });
    }

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

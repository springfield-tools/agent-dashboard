mod groups;
mod popover_anchor;

#[cfg(target_os = "macos")]
mod tray;

#[tauri::command]
fn placeholder_group_count() -> usize {
    groups::group_count()
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
        .invoke_handler(tauri::generate_handler![placeholder_group_count]);

    #[cfg(target_os = "macos")]
    {
        builder = builder.setup(|app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
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

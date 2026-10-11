use std::sync::Arc;

use tauri::AppHandle;

use crate::pause::is_polling_paused;
use crate::poll::{run_poll_cycle, PollRuntime};
use crate::state::AppState;

/// One immediate refresh after wake when user pause is not active (PLAN § Sleep and offline).
pub fn handle_system_wake<R: tauri::Runtime>(app: &AppHandle<R>, state: &Arc<AppState>) {
    let user_paused = state
        .config
        .lock()
        .map(|c| is_polling_paused(&c.poll_pause))
        .unwrap_or(false);
    if user_paused {
        log::info!("wake: skipped refresh (user pause still active)");
        return;
    }
    state.poll_runtime.wake.notify_one();
    let app = app.clone();
    let state = state.clone();
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("http client");
        let _ = run_poll_cycle(&app, &state, &client, false).await;
    });
}

#[cfg(target_os = "macos")]
pub fn install_wake_listener<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: Arc<AppState>,
    _runtime: Arc<PollRuntime>,
) {
    use block2::RcBlock;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceDidWakeNotification};
    use objc2_foundation::{NSNotification, NSOperationQueue};

    let app = app.clone();
    let state = state.clone();
    let block = RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| {
        log::info!("wake: NSWorkspaceDidWakeNotification");
        handle_system_wake(&app, &state);
    });

    let workspace = NSWorkspace::sharedWorkspace();
    let center = workspace.notificationCenter();
    unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidWakeNotification),
            None,
            Some(&*NSOperationQueue::mainQueue()),
            &block,
        );
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install_wake_listener<R: tauri::Runtime>(
    _app: AppHandle<R>,
    _state: Arc<AppState>,
    _runtime: Arc<PollRuntime>,
) {
}

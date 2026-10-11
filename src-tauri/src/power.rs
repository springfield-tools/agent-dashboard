use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::AppHandle;

use crate::pause::is_polling_paused;
use crate::poll::PollRuntime;
use crate::state::AppState;

/// Queue one post-wake refresh on the poll loop (user pause only suppresses wake).
pub fn handle_system_wake(state: &Arc<AppState>) {
    let user_paused = state
        .config
        .lock()
        .map(|c| is_polling_paused(&c.poll_pause))
        .unwrap_or(false);
    if user_paused {
        log::info!("wake: skipped refresh (user pause still active)");
        return;
    }
    if state.poll_runtime.rate_limit_is_active() {
        log::info!("wake: skipped refresh (rate limit wait active)");
        return;
    }
    state.poll_runtime.pending_wake.store(true, Ordering::SeqCst);
    state.poll_runtime.wake.notify_one();
}

#[cfg(target_os = "macos")]
pub fn install_wake_listener<R: tauri::Runtime>(
    _app: AppHandle<R>,
    state: Arc<AppState>,
    _runtime: Arc<PollRuntime>,
) {
    use block2::RcBlock;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceDidWakeNotification};
    use objc2_foundation::{NSNotification, NSOperationQueue};

    let state = state.clone();
    let block = RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| {
        log::info!("wake: NSWorkspaceDidWakeNotification");
        handle_system_wake(&state);
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

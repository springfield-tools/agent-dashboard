use std::sync::Arc;

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, Monitor, Position, Rect, Runtime, WebviewWindow,
};

use crate::model::{snapshot_is_stale, DashboardSnapshot};
use crate::pause::{is_polling_paused, pause_tooltip};
use crate::popover_anchor::{
    logical_position_tauri, physical_tray_rect, popover_logical_position, PhysicalTrayRect,
};
use crate::poll::{run_poll_cycle, update_config_and_save};
use crate::state::AppState;

fn tray_pause_info<R: Runtime>(app: &tauri::AppHandle<R>) -> (bool, String) {
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        let user_paused = state
            .config
            .lock()
            .map(|guard| is_polling_paused(&guard.poll_pause))
            .unwrap_or(false);
        let idle_paused = state.poll_runtime.is_idle_paused();
        let paused = user_paused || idle_paused;
        let tip = if idle_paused && !user_paused {
            "Polling paused while idle".to_string()
        } else {
            state
                .config
                .lock()
                .map(|guard| pause_tooltip(&guard.poll_pause))
                .unwrap_or_else(|_| "Polling paused".to_string())
        };
        return (paused, tip);
    }
    (false, "Polling".to_string())
}

const ICON_NORMAL: Image<'_> = tauri::include_image!("icons/tray-template.png");
const ICON_PAUSED: Image<'_> = tauri::include_image!("icons/tray-pause-template.png");

pub fn setup<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    sync_tray_from_app(app);

    let refresh = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause polling", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume", "Resume polling", true, None::<&str>)?;
    let pause_hour =
        MenuItem::with_id(app, "pause_hour", "Pause for 1 hour", true, None::<&str>)?;
    let pause_tomorrow =
        MenuItem::with_id(app, "pause_tomorrow", "Pause until tomorrow", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let open_config = MenuItem::with_id(app, "open_config", "Open config", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &refresh,
            &pause,
            &resume,
            &pause_hour,
            &pause_tomorrow,
            &separator,
            &settings,
            &open_config,
            &separator,
            &quit,
        ],
    )?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(ICON_NORMAL.clone())
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_popover(tray.app_handle(), Some(rect));
            }
        })
        .build(app)?;

    if let Some(window) = app.get_webview_window("popover") {
        window.hide()?;
    }

    Ok(())
}

fn handle_menu_event<R: Runtime>(app: &tauri::AppHandle<R>, id: &str) {
    match id {
        "quit" => {
            app.exit(0);
        }
        "refresh" => {
            trigger_refresh(app, true);
        }
        "pause" => {
            set_pause_manual(app);
        }
        "resume" => {
            resume_polling(app);
        }
        "pause_hour" => {
            set_pause_timed(app, crate::pause::pause_for_one_hour_value());
        }
        "pause_tomorrow" => {
            set_pause_timed(app, crate::pause::pause_until_tomorrow_local_value());
        }
        "settings" => {
            let _ = app.emit("open-settings", ());
            if let Some(window) = app.get_webview_window("popover") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "open_config" => {
            if let Some(state) = app.try_state::<Arc<AppState>>() {
                let path = state.config_path.display().to_string();
                let _ = tauri_plugin_opener::OpenerExt::opener(app).open_path(path, None::<&str>);
            }
        }
        _ => {}
    }
}

fn set_pause_manual<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        let mut config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
        config.poll_pause = Some(crate::pause::pause_manual_value());
        if update_config_and_save(state.inner(), config).is_ok() {
            sync_tray_paused(app, true);
            let _ = app.emit("dashboard-updated", ());
        }
    }
}

fn set_pause_timed<R: Runtime>(app: &tauri::AppHandle<R>, until: String) {
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        let mut config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
        config.poll_pause = Some(until);
        if update_config_and_save(state.inner(), config).is_ok() {
            sync_tray_paused(app, true);
            let _ = app.emit("dashboard-updated", ());
        }
    }
}

fn resume_polling<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        let mut config = state.config.lock().map(|c| c.clone()).unwrap_or_default();
        config.poll_pause = None;
        if update_config_and_save(state.inner(), config).is_ok() {
            sync_tray_paused(app, false);
        }
    }
}

fn maybe_refresh_stale_snapshot<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        if state.fixture_mode {
            return;
        }
        let paused = state
            .config
            .lock()
            .map(|c| is_polling_paused(&c.poll_pause))
            .unwrap_or(false);
        if paused {
            return;
        }
        let interval = state
            .config
            .lock()
            .map(|c| c.poll_interval_seconds)
            .unwrap_or(30);
        let stale = state
            .dashboard
            .lock()
            .map(|d| snapshot_is_stale(&d.fetched_at, interval))
            .unwrap_or(true);
        if stale {
            trigger_refresh(app, false);
        }
    }
}

pub fn trigger_refresh<R: Runtime>(app: &tauri::AppHandle<R>, force: bool) {
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(state) = app_handle.try_state::<Arc<AppState>>() {
            let client = reqwest::Client::new();
            let _ = run_poll_cycle(&app_handle, state.inner(), &client, force).await;
        }
    });
}

pub fn sync_tray_from_snapshot<R: Runtime>(app: &tauri::AppHandle<R>, snapshot: &DashboardSnapshot) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let (paused, pause_tip) = tray_pause_info(app);
        if paused {
            let _ = tray.set_icon(Some(ICON_PAUSED.clone()));
            let _ = tray.set_icon_as_template(true);
            let _ = tray.set_title(Some("Paused"));
            let _ = tray.set_tooltip(Some(pause_tip));
        } else {
            let _ = tray.set_icon(Some(ICON_NORMAL.clone()));
            let _ = tray.set_icon_as_template(true);
            let _ = tray.set_title(if snapshot.tray_title.is_empty() {
                None
            } else {
                Some(snapshot.tray_title.clone())
            });
            let _ = tray.set_tooltip(Some(snapshot.tray_tooltip.clone()));
        }
    }
}

pub fn sync_tray_paused_with_tooltip<R: Runtime>(app: &tauri::AppHandle<R>, tooltip: &str) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_icon(Some(ICON_PAUSED.clone()));
        let _ = tray.set_icon_as_template(true);
        let _ = tray.set_title(Some("Paused"));
        let _ = tray.set_tooltip(Some(tooltip.to_string()));
    }
}

pub fn sync_tray_paused<R: Runtime>(app: &tauri::AppHandle<R>, paused: bool) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        if paused {
            let _ = tray.set_icon(Some(ICON_PAUSED.clone()));
            let _ = tray.set_icon_as_template(true);
            let _ = tray.set_title(Some("Paused"));
            let (_, pause_tip) = tray_pause_info(app);
            let _ = tray.set_tooltip(Some(pause_tip));
        } else if let Some(state) = app.try_state::<Arc<AppState>>() {
            if let Ok(guard) = state.dashboard.lock() {
                sync_tray_from_snapshot(app, &guard);
            }
        }
    }
}

pub fn sync_tray_from_app<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        if let Ok(guard) = state.dashboard.lock() {
            sync_tray_from_snapshot(app, &guard);
        }
    } else {
        sync_tray_title(app);
    }
}

pub fn sync_tray_title<R: Runtime>(app: &tauri::AppHandle<R>) {
    sync_tray_from_app(app);
}

fn monitor_for_tray<R: Runtime>(
    window: &WebviewWindow<R>,
    tray: &PhysicalTrayRect,
) -> Option<Monitor> {
    let center_x = tray.x as f64 + tray.width as f64 / 2.0;
    let center_y = tray.y as f64 + tray.height as f64 / 2.0;

    if let Ok(Some(monitor)) = window.monitor_from_point(center_x, center_y) {
        return Some(monitor);
    }

    if let Ok(monitors) = window.available_monitors() {
        for monitor in monitors {
            let pos = monitor.position();
            let size = monitor.size();
            let left = pos.x as f64;
            let top = pos.y as f64;
            let right = left + size.width as f64;
            let bottom = top + size.height as f64;
            if center_x >= left && center_x < right && center_y >= top && center_y < bottom {
                return Some(monitor);
            }
        }
    }

    window.primary_monitor().ok().flatten()
}

fn position_popover_below_tray<R: Runtime>(window: &WebviewWindow<R>, rect: &Rect) {
    let tray = physical_tray_rect(rect, 1.0);
    let Some(monitor) = monitor_for_tray(window, &tray) else {
        return;
    };

    let work_area = monitor.work_area();
    let work = crate::popover_anchor::MonitorWorkAreaPhysical {
        x: work_area.position.x,
        y: work_area.position.y,
        width: work_area.size.width,
        height: work_area.size.height,
        scale_factor: monitor.scale_factor(),
    };

    let logical = popover_logical_position(tray, work);
    let _ = window.set_position(Position::Logical(logical_position_tauri(logical)));
}

fn toggle_popover<R: Runtime>(app: &tauri::AppHandle<R>, tray_rect: Option<Rect>) {
    let Some(window) = app.get_webview_window("popover") else {
        return;
    };

    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        if let Some(rect) = tray_rect {
            position_popover_below_tray(&window, &rect);
        }

        let _ = window.show();
        let _ = window.set_focus();
        maybe_refresh_stale_snapshot(app);
    }
}

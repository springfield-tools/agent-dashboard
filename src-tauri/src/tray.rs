use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Monitor, Position, Rect, Runtime, WebviewWindow,
};

use crate::popover_anchor::{
    logical_position_tauri, physical_tray_rect, popover_logical_position, PhysicalTrayRect,
};

pub fn setup<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&quit])?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(tauri::include_image!("icons/tray-template.png"))
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id.as_ref() == "quit" {
                app.exit(0);
            }
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
    }
}

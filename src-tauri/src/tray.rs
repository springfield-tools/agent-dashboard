use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, PhysicalPosition, Position, Rect, Runtime,
};

const POPOVER_WIDTH: i32 = 400;

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

fn position_popover_below_tray<R: Runtime>(window: &tauri::WebviewWindow<R>, rect: &Rect) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let pos = rect.position.to_physical(scale);
    let size = rect.size.to_physical(scale);
    let icon_center_x: f64 = pos.x as f64 + size.width as f64 / 2.0;
    let x = (icon_center_x - POPOVER_WIDTH as f64 / 2.0).round() as i32;
    let y = (pos.y as f64 + size.height as f64).round() as i32;

    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
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

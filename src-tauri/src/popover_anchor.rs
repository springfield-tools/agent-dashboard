//! Pure math for anchoring the popover under the menu-bar tray icon.

use tauri::{LogicalPosition, PhysicalPosition, PhysicalSize, Position, Rect, Size};

/// Popover size in logical points (matches `tauri.conf.json`).
pub const POPOVER_WIDTH_LOGICAL: f64 = 400.0;
pub const POPOVER_HEIGHT_LOGICAL: f64 = 560.0;

/// Tray icon bounds in physical screen pixels (from `TrayIconEvent::Click` on macOS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalTrayRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Monitor work area in physical pixels plus scale for logical conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorWorkAreaPhysical {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

/// Popover top-left in logical points (what Tauri expects for `set_position`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalPoint {
    pub x: f64,
    pub y: f64,
}

/// Read tray bounds as physical pixels. macOS tray events are already physical; logical
/// variants are converted with `fallback_scale` only when needed.
pub fn physical_tray_rect(rect: &Rect, fallback_scale: f64) -> PhysicalTrayRect {
    let pos: PhysicalPosition<i32> = match rect.position {
        Position::Physical(p) => p,
        Position::Logical(l) => l.to_physical(fallback_scale),
    };
    let size: PhysicalSize<u32> = match rect.size {
        Size::Physical(s) => s,
        Size::Logical(l) => l.to_physical(fallback_scale),
    };
    PhysicalTrayRect {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    }
}

/// Center the popover horizontally under the tray icon (just below the menu bar) and clamp
/// to the monitor work area so the window stays on-screen.
pub fn popover_logical_position(
    tray: PhysicalTrayRect,
    work_area: MonitorWorkAreaPhysical,
) -> LogicalPoint {
    let scale = work_area.scale_factor.max(1.0);

    let icon_center_x_phys = tray.x as f64 + tray.width as f64 / 2.0;
    let below_menu_bar_y_phys = tray.y as f64 + tray.height as f64;

    let mut x = icon_center_x_phys / scale - POPOVER_WIDTH_LOGICAL / 2.0;
    let mut y = below_menu_bar_y_phys / scale;

    let wa_x = work_area.x as f64 / scale;
    let wa_y = work_area.y as f64 / scale;
    let wa_w = work_area.width as f64 / scale;
    let wa_h = work_area.height as f64 / scale;

    let min_x = wa_x;
    let max_x = wa_x + wa_w - POPOVER_WIDTH_LOGICAL;
    let min_y = wa_y;
    let max_y = wa_y + wa_h - POPOVER_HEIGHT_LOGICAL;

    x = clamp(x, min_x, max_x);
    y = clamp(y, min_y, max_y);

    LogicalPoint { x, y }
}

pub fn logical_position_tauri(point: LogicalPoint) -> LogicalPosition<f64> {
    LogicalPosition::new(point.x, point.y)
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if min > max {
        min
    } else {
        value.clamp(min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work_area_1080p(scale: f64) -> MonitorWorkAreaPhysical {
        let w = (1920.0 * scale).round() as u32;
        let h = (1080.0 * scale).round() as u32;
        MonitorWorkAreaPhysical {
            x: 0,
            y: 0,
            width: w,
            height: h,
            scale_factor: scale,
        }
    }

    #[test]
    fn centers_under_icon_at_1x() {
        let tray = PhysicalTrayRect {
            x: 1000,
            y: 0,
            width: 22,
            height: 22,
        };
        let pos = popover_logical_position(tray, work_area_1080p(1.0));
        assert!((pos.x - 811.0).abs() < 0.01);
        assert!((pos.y - 22.0).abs() < 0.01);
    }

    #[test]
    fn centers_under_icon_at_2x_retina() {
        let tray = PhysicalTrayRect {
            x: 2000,
            y: 0,
            width: 44,
            height: 44,
        };
        let pos = popover_logical_position(tray, work_area_1080p(2.0));
        assert!((pos.x - 811.0).abs() < 0.01);
        assert!((pos.y - 22.0).abs() < 0.01);
    }

    #[test]
    fn clamps_when_icon_near_right_edge() {
        let tray = PhysicalTrayRect {
            x: 3778,
            y: 0,
            width: 44,
            height: 44,
        };
        let pos = popover_logical_position(tray, work_area_1080p(2.0));
        // max x = 1920 - 400 = 1520
        assert!((pos.x - 1520.0).abs() < 0.01);
        assert!((pos.y - 22.0).abs() < 0.01);
    }

    #[test]
    fn clamps_when_popover_taller_than_work_area() {
        let work = MonitorWorkAreaPhysical {
            x: 0,
            y: 0,
            width: 800,
            height: 400,
            scale_factor: 1.0,
        };
        let tray = PhysicalTrayRect {
            x: 100,
            y: 0,
            width: 22,
            height: 22,
        };
        let pos = popover_logical_position(tray, work);
        assert!((pos.y - 0.0).abs() < 0.01);
    }
}

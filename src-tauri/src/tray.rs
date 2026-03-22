use crate::state::{AppStatus, ConnectionStatus};
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

/// Menu item IDs used for identifying tray menu actions and status display.
const MENU_ID_CLAUDE_STATUS: &str = "claude_status";
const MENU_ID_WECHAT_STATUS: &str = "wechat_status";
const MENU_ID_OPEN_WINDOW: &str = "open_window";
const MENU_ID_RESTART_CLAUDE: &str = "restart_claude";
const MENU_ID_RESTART_WECHAT: &str = "restart_wechat";
const MENU_ID_QUIT: &str = "quit";

/// Tray icon identifier.
const TRAY_ID: &str = "main_tray";

/// Icon size in pixels for the generated tray icons.
const ICON_SIZE: u32 = 32;

/// Create the system tray with menu and event handlers.
///
/// This sets up:
/// - A colored circle tray icon reflecting connection status
/// - A right-click context menu with status display and action items
/// - Left-click handler to show/focus the main window
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let claude_status = MenuItemBuilder::with_id(MENU_ID_CLAUDE_STATUS, "Claude Code: 未连接")
        .enabled(false)
        .build(app)?;

    let wechat_status = MenuItemBuilder::with_id(MENU_ID_WECHAT_STATUS, "微信: 未连接")
        .enabled(false)
        .build(app)?;

    let open_window =
        MenuItemBuilder::with_id(MENU_ID_OPEN_WINDOW, "打开窗口").build(app)?;

    let restart_claude =
        MenuItemBuilder::with_id(MENU_ID_RESTART_CLAUDE, "重启 Claude Code").build(app)?;

    let restart_wechat =
        MenuItemBuilder::with_id(MENU_ID_RESTART_WECHAT, "重启微信连接").build(app)?;

    let quit = MenuItemBuilder::with_id(MENU_ID_QUIT, "退出").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&claude_status)
        .item(&wechat_status)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&open_window)
        .item(&restart_claude)
        .item(&restart_wechat)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&quit)
        .build()?;

    let icon = generate_icon(TrayColor::Red);

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .tooltip("ClaudeWXTray")
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                id if id == MENU_ID_OPEN_WINDOW => {
                    show_main_window(app);
                }
                id if id == MENU_ID_RESTART_CLAUDE => {
                    let state = app.state::<crate::state::AppState>();
                    state.pty_pool.restart("claude", 120, 40);
                }
                id if id == MENU_ID_RESTART_WECHAT => {
                    let state = app.state::<crate::state::AppState>();
                    state.pty_pool.restart("wechat", 120, 40);
                }
                id if id == MENU_ID_QUIT => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Update the tray menu status text and icon color based on current connection state.
pub fn update_tray_status(app: &AppHandle, status: &AppStatus) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    // Determine the icon color based on overall status
    let color = match (status.claude, status.wechat) {
        (ConnectionStatus::Connected, ConnectionStatus::Connected) => TrayColor::Green,
        (ConnectionStatus::Disconnected, _) | (_, ConnectionStatus::Disconnected) => {
            // If either is disconnected, check if the other is reconnecting
            if status.claude == ConnectionStatus::Reconnecting
                || status.wechat == ConnectionStatus::Reconnecting
            {
                TrayColor::Yellow
            } else {
                TrayColor::Red
            }
        }
        _ => TrayColor::Yellow, // Any reconnecting state
    };

    // Update icon
    let icon = generate_icon(color);
    let _ = tray.set_icon(Some(icon));

    // Update tooltip
    let tooltip = format!(
        "ClaudeWXTray - Claude: {} | 微信: {}",
        status_text_short(status.claude),
        status_text_short(status.wechat)
    );
    let _ = tray.set_tooltip(Some(&tooltip));

    // Rebuild the menu with updated status text to reflect current connection state.
    // Tauri v2 menu items are immutable after creation, so we rebuild the entire menu.
    if let Ok(new_menu) = rebuild_menu(app, status) {
        let _ = tray.set_menu(Some(new_menu));
    }
}

/// Rebuild the tray menu with updated status strings.
fn rebuild_menu(
    app: &AppHandle,
    status: &AppStatus,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let claude_label = format!("Claude Code: {}", status_text(status.claude));
    let wechat_label = format!("微信: {}", status_text(status.wechat));

    let claude_status = MenuItemBuilder::with_id(MENU_ID_CLAUDE_STATUS, &claude_label)
        .enabled(false)
        .build(app)?;

    let wechat_status = MenuItemBuilder::with_id(MENU_ID_WECHAT_STATUS, &wechat_label)
        .enabled(false)
        .build(app)?;

    let open_window =
        MenuItemBuilder::with_id(MENU_ID_OPEN_WINDOW, "打开窗口").build(app)?;

    let restart_claude =
        MenuItemBuilder::with_id(MENU_ID_RESTART_CLAUDE, "重启 Claude Code").build(app)?;

    let restart_wechat =
        MenuItemBuilder::with_id(MENU_ID_RESTART_WECHAT, "重启微信连接").build(app)?;

    let quit = MenuItemBuilder::with_id(MENU_ID_QUIT, "退出").build(app)?;

    MenuBuilder::new(app)
        .item(&claude_status)
        .item(&wechat_status)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&open_window)
        .item(&restart_claude)
        .item(&restart_wechat)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&quit)
        .build()
}

/// Show and focus the main application window.
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // Unminimize if minimized, then show and bring to front
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Convert a ConnectionStatus to display text for menu items.
fn status_text(status: ConnectionStatus) -> &'static str {
    match status {
        ConnectionStatus::Connected => "已连接",
        ConnectionStatus::Reconnecting => "重连中...",
        ConnectionStatus::Disconnected => "未连接",
    }
}

/// Short status text for the tooltip.
fn status_text_short(status: ConnectionStatus) -> &'static str {
    match status {
        ConnectionStatus::Connected => "已连接",
        ConnectionStatus::Reconnecting => "重连中",
        ConnectionStatus::Disconnected => "未连接",
    }
}

// ---------------------------------------------------------------------------
// Programmatic icon generation
// ---------------------------------------------------------------------------

/// The three possible tray icon colors.
#[derive(Clone, Copy)]
enum TrayColor {
    Green,
    Yellow,
    Red,
}

/// Generate a chat-bubble shaped RGBA icon at `ICON_SIZE x ICON_SIZE`.
///
/// The bubble has a fixed dark gradient fill. A small colored status dot
/// in the top-right corner indicates the current connection state.
fn generate_icon(color: TrayColor) -> Image<'static> {
    // Fixed bubble gradient: dark blue-grey
    let bubble_hi: (u8, u8, u8) = (0x6B, 0x72, 0x82);
    let bubble_base: (u8, u8, u8) = (0x44, 0x4C, 0x5C);
    let bubble_shadow: (u8, u8, u8) = (0x2C, 0x32, 0x3E);

    // Status dot color
    let dot_color: (u8, u8, u8) = match color {
        TrayColor::Green => (0x34, 0xC7, 0x59),
        TrayColor::Yellow => (0xFF, 0xCC, 0x00),
        TrayColor::Red => (0xEF, 0x5B, 0x50),
    };

    let size = ICON_SIZE as usize;
    let s = size as f64;
    let mut rgba = vec![0u8; size * size * 4];

    // Chat bubble body (rounded rectangle)
    let pad = 1.5;
    let bx0 = pad;
    let by0 = pad;
    let bx1 = s - pad;
    let by1 = s - pad - 7.0;
    let corner = 6.0;

    // Tail triangle vertices (bottom-left of bubble)
    let tail_ax = bx0 + 4.0;
    let tail_ay = by1;
    let tail_bx = bx0 + 11.0;
    let tail_by = by1;
    let tail_cx = bx0 + 2.0;
    let tail_cy = s - pad;

    // Status dot (top-right corner)
    let dot_r = 4.0_f64;
    let dot_cx = bx1 - 1.5;
    let dot_cy = by0 + 1.5;

    for y in 0..size {
        for x in 0..size {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;

            // --- Layer 0: bubble ---
            let d_body = sdf_rounded_rect(px, py, bx0, by0, bx1, by1, corner);
            let d_tail = sdf_triangle(
                px, py, tail_ax, tail_ay, tail_bx, tail_by, tail_cx, tail_cy,
            );
            let d_bubble = d_body.min(d_tail);
            let bubble_a = (0.5 - d_bubble).clamp(0.0, 1.0);

            let idx = (y * size + x) * 4;

            if bubble_a > 0.0 {
                let t = ((py - by0) / (tail_cy - by0)).clamp(0.0, 1.0);
                let (cr, cg, cb) = if t < 0.35 {
                    let lt = t / 0.35;
                    (
                        lerp_u8(bubble_hi.0, bubble_base.0, lt),
                        lerp_u8(bubble_hi.1, bubble_base.1, lt),
                        lerp_u8(bubble_hi.2, bubble_base.2, lt),
                    )
                } else {
                    let lt = (t - 0.35) / 0.65;
                    (
                        lerp_u8(bubble_base.0, bubble_shadow.0, lt),
                        lerp_u8(bubble_base.1, bubble_shadow.1, lt),
                        lerp_u8(bubble_base.2, bubble_shadow.2, lt),
                    )
                };
                rgba[idx] = cr;
                rgba[idx + 1] = cg;
                rgba[idx + 2] = cb;
                rgba[idx + 3] = (bubble_a * 255.0) as u8;
            }

            // --- Layer 1: status dot (composited on top) ---
            let dx = px - dot_cx;
            let dy = py - dot_cy;
            let d_dot = (dx * dx + dy * dy).sqrt() - dot_r;
            let dot_a = (0.5 - d_dot).clamp(0.0, 1.0);

            if dot_a > 0.0 {
                let bg_a = rgba[idx + 3] as f64 / 255.0;
                let out_a = dot_a + bg_a * (1.0 - dot_a);
                if out_a > 0.0 {
                    let inv = bg_a * (1.0 - dot_a);
                    rgba[idx] =
                        ((dot_color.0 as f64 * dot_a + rgba[idx] as f64 * inv) / out_a) as u8;
                    rgba[idx + 1] =
                        ((dot_color.1 as f64 * dot_a + rgba[idx + 1] as f64 * inv) / out_a) as u8;
                    rgba[idx + 2] =
                        ((dot_color.2 as f64 * dot_a + rgba[idx + 2] as f64 * inv) / out_a) as u8;
                    rgba[idx + 3] = (out_a * 255.0) as u8;
                }
            }
        }
    }

    Image::new_owned(rgba, ICON_SIZE, ICON_SIZE)
}

/// Signed distance field for a rounded rectangle.
fn sdf_rounded_rect(px: f64, py: f64, x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> f64 {
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;
    let hw = (x1 - x0) / 2.0;
    let hh = (y1 - y0) / 2.0;

    let dx = (px - cx).abs() - (hw - r);
    let dy = (py - cy).abs() - (hh - r);

    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let inside = dx.max(dy).min(0.0);

    outside + inside - r
}

/// Signed distance field for a triangle (negative inside, positive outside).
fn sdf_triangle(
    px: f64, py: f64,
    x0: f64, y0: f64,
    x1: f64, y1: f64,
    x2: f64, y2: f64,
) -> f64 {
    let (e0x, e0y) = (x1 - x0, y1 - y0);
    let (e1x, e1y) = (x2 - x1, y2 - y1);
    let (e2x, e2y) = (x0 - x2, y0 - y2);
    let (v0x, v0y) = (px - x0, py - y0);
    let (v1x, v1y) = (px - x1, py - y1);
    let (v2x, v2y) = (px - x2, py - y2);

    fn clamp_project(vx: f64, vy: f64, ex: f64, ey: f64) -> f64 {
        let le = ex * ex + ey * ey;
        if le < 1e-12 {
            return 0.0;
        }
        (vx * ex + vy * ey).clamp(0.0, le) / le
    }

    let t0 = clamp_project(v0x, v0y, e0x, e0y);
    let t1 = clamp_project(v1x, v1y, e1x, e1y);
    let t2 = clamp_project(v2x, v2y, e2x, e2y);

    let (p0x, p0y) = (v0x - e0x * t0, v0y - e0y * t0);
    let (p1x, p1y) = (v1x - e1x * t1, v1y - e1y * t1);
    let (p2x, p2y) = (v2x - e2x * t2, v2y - e2y * t2);

    let d0 = p0x * p0x + p0y * p0y;
    let d1 = p1x * p1x + p1y * p1y;
    let d2 = p2x * p2x + p2y * p2y;

    let min_dist = d0.min(d1).min(d2).sqrt();

    // Inside/outside via cross product winding
    let c0 = e0x * v0y - e0y * v0x;
    let c1 = e1x * v1y - e1y * v1x;
    let c2 = e2x * v2y - e2y * v2x;

    let inside = (c0 >= 0.0 && c1 >= 0.0 && c2 >= 0.0)
        || (c0 <= 0.0 && c1 <= 0.0 && c2 <= 0.0);

    if inside { -min_dist } else { min_dist }
}

/// Linearly interpolate between two u8 values.
fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    let t = t.clamp(0.0, 1.0);
    (a as f64 * (1.0 - t) + b as f64 * t) as u8
}

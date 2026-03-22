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
const ICON_SIZE: u32 = 44;

/// Create the system tray with menu and event handlers.
///
/// This sets up:
/// - A colored circle tray icon reflecting connection status
/// - A right-click context menu with status display and action items
/// - Left-click handler to show/focus the main window
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let claude_status = MenuItemBuilder::with_id(MENU_ID_CLAUDE_STATUS, "Claude Code: 已连接")
        .enabled(false)
        .build(app)?;

    let wechat_status = MenuItemBuilder::with_id(MENU_ID_WECHAT_STATUS, "微信: 已连接")
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

    let icon = generate_icon(TrayColor::Green);

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

/// Generate a WeChat-style dual-bubble tray icon at `ICON_SIZE x ICON_SIZE`.
///
/// Two overlapping chat bubbles (like the WeChat logo) rendered as dark
/// silhouettes suitable for the macOS menu bar. When status is Yellow or Red,
/// a small colored dot is drawn in the top-right corner.
fn generate_icon(color: TrayColor) -> Image<'static> {
    let size = ICON_SIZE as usize;
    let s = size as f64;
    let mut rgba = vec![0u8; size * size * 4];

    // Bubble fill color — dark for macOS menu bar visibility
    let fill: (u8, u8, u8) = (0x2C, 0x2C, 0x2E);

    // ── Large bubble (left) ──
    // Ellipse center & radii
    let b1_cx = s * 0.38;
    let b1_cy = s * 0.44;
    let b1_rx = s * 0.32;
    let b1_ry = s * 0.28;
    // Tail
    let t1 = [
        (b1_cx - s * 0.22, b1_cy + b1_ry * 0.7),
        (b1_cx - s * 0.12, b1_cy + b1_ry * 0.85),
        (b1_cx - s * 0.28, b1_cy + b1_ry * 1.15),
    ];

    // ── Small bubble (right, overlapping) ──
    let b2_cx = s * 0.65;
    let b2_cy = s * 0.54;
    let b2_rx = s * 0.25;
    let b2_ry = s * 0.22;
    // Tail
    let t2 = [
        (b2_cx + s * 0.14, b2_cy + b2_ry * 0.7),
        (b2_cx + s * 0.06, b2_cy + b2_ry * 0.85),
        (b2_cx + s * 0.20, b2_cy + b2_ry * 1.15),
    ];

    // ── Eyes (white dots inside bubbles) ──
    let eye_r = s * 0.04;
    // Large bubble eyes
    let e1a = (b1_cx - s * 0.10, b1_cy);
    let e1b = (b1_cx + s * 0.06, b1_cy);
    // Small bubble eyes
    let e2a = (b2_cx - s * 0.08, b2_cy);
    let e2b = (b2_cx + s * 0.06, b2_cy);

    // ── Status dot (top-right, only for Yellow/Red) ──
    let show_dot = !matches!(color, TrayColor::Green);
    let dot_color: (u8, u8, u8) = match color {
        TrayColor::Green => (0, 0, 0),
        TrayColor::Yellow => (0xFF, 0xCC, 0x00),
        TrayColor::Red => (0xEF, 0x5B, 0x50),
    };
    let dot_r = s * 0.10;
    let dot_cx = s * 0.88;
    let dot_cy = s * 0.12;

    for y in 0..size {
        for x in 0..size {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let idx = (y * size + x) * 4;

            // SDF for ellipses: ((x-cx)/rx)^2 + ((y-cy)/ry)^2 - 1
            let d_b1 = sdf_ellipse(px, py, b1_cx, b1_cy, b1_rx, b1_ry);
            let d_t1 = sdf_triangle(
                px, py, t1[0].0, t1[0].1, t1[1].0, t1[1].1, t1[2].0, t1[2].1,
            );
            let d_big = d_b1.min(d_t1);

            let d_b2 = sdf_ellipse(px, py, b2_cx, b2_cy, b2_rx, b2_ry);
            let d_t2 = sdf_triangle(
                px, py, t2[0].0, t2[0].1, t2[1].0, t2[1].1, t2[2].0, t2[2].1,
            );
            let d_small = d_b2.min(d_t2);

            let d_bubble = d_big.min(d_small);
            let bubble_a = (0.5 - d_bubble).clamp(0.0, 1.0);

            if bubble_a > 0.0 {
                rgba[idx] = fill.0;
                rgba[idx + 1] = fill.1;
                rgba[idx + 2] = fill.2;
                rgba[idx + 3] = (bubble_a * 255.0) as u8;
            }

            // ── Eyes (white, composited on top of bubbles) ──
            let eyes = [e1a, e1b, e2a, e2b];
            for &(ecx, ecy) in &eyes {
                let dx = px - ecx;
                let dy = py - ecy;
                let d_eye = (dx * dx + dy * dy).sqrt() - eye_r;
                let eye_a = (0.5 - d_eye).clamp(0.0, 1.0);
                if eye_a > 0.0 {
                    composite_pixel(&mut rgba, idx, 0xFF, 0xFF, 0xFF, eye_a);
                }
            }

            // ── Status dot ──
            if show_dot {
                let dx = px - dot_cx;
                let dy = py - dot_cy;
                let d_dot = (dx * dx + dy * dy).sqrt() - dot_r;
                let dot_a = (0.5 - d_dot).clamp(0.0, 1.0);
                if dot_a > 0.0 {
                    composite_pixel(
                        &mut rgba,
                        idx,
                        dot_color.0,
                        dot_color.1,
                        dot_color.2,
                        dot_a,
                    );
                }
            }
        }
    }

    Image::new_owned(rgba, ICON_SIZE, ICON_SIZE)
}

/// Approximate signed distance for an axis-aligned ellipse.
fn sdf_ellipse(px: f64, py: f64, cx: f64, cy: f64, rx: f64, ry: f64) -> f64 {
    let nx = (px - cx) / rx;
    let ny = (py - cy) / ry;
    let len = (nx * nx + ny * ny).sqrt();
    // Approximate SDF: scale the unit-circle distance back by the average radius.
    (len - 1.0) * (rx.min(ry))
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

    let c0 = e0x * v0y - e0y * v0x;
    let c1 = e1x * v1y - e1y * v1x;
    let c2 = e2x * v2y - e2y * v2x;

    let inside =
        (c0 >= 0.0 && c1 >= 0.0 && c2 >= 0.0) || (c0 <= 0.0 && c1 <= 0.0 && c2 <= 0.0);

    if inside { -min_dist } else { min_dist }
}

/// Alpha-composite a foreground color onto an existing pixel in the RGBA buffer.
fn composite_pixel(rgba: &mut [u8], idx: usize, fr: u8, fg: u8, fb: u8, fa: f64) {
    let bg_a = rgba[idx + 3] as f64 / 255.0;
    let out_a = fa + bg_a * (1.0 - fa);
    if out_a > 0.0 {
        let inv = bg_a * (1.0 - fa);
        rgba[idx] = ((fr as f64 * fa + rgba[idx] as f64 * inv) / out_a) as u8;
        rgba[idx + 1] = ((fg as f64 * fa + rgba[idx + 1] as f64 * inv) / out_a) as u8;
        rgba[idx + 2] = ((fb as f64 * fa + rgba[idx + 2] as f64 * inv) / out_a) as u8;
        rgba[idx + 3] = (out_a * 255.0) as u8;
    }
}

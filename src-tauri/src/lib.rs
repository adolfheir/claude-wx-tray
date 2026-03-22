mod commands;
mod keepalive;
mod pty;
mod state;
mod tray;

use tauri::Manager;

use pty::{PtyManager, PtyPool};
use state::AppState;

/// Default command for the Claude Code PTY.
const CLAUDE_CMD: &str = "claude --permission-mode bypassPermissions --dangerously-skip-permissions --dangerously-load-development-channels server:wechat";

/// Default command for the WeChat login/status PTY.
/// Checks for existing credentials; runs setup if missing, otherwise shows status.
const WECHAT_CMD: &str = r#"if [ ! -f ~/.claude/channels/wechat/account.json ]; then echo '未检测到微信凭证，正在启动登录流程...'; echo ''; bun setup.ts; else echo '微信凭证已就绪'; echo ''; echo '账号信息：'; cat ~/.claude/channels/wechat/account.json; echo ''; echo ''; echo '如需重新登录，请删除 ~/.claude/channels/wechat/account.json 后重启应用'; fi; exec bash"#;

/// Default terminal dimensions (cols x rows).
const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 40;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Create the PTY pool with two named PTYs.
            let mut pool = PtyPool::new();

            // "wechat" PTY: runs the WeChat login/status check.
            let wechat_pty = PtyManager::new(
                handle.clone(),
                "wechat",
                WECHAT_CMD,
                DEFAULT_COLS,
                DEFAULT_ROWS,
            );
            pool.add("wechat", wechat_pty);

            // "claude" PTY: runs the Claude Code process.
            let claude_pty = PtyManager::new(
                handle.clone(),
                "claude",
                CLAUDE_CMD,
                DEFAULT_COLS,
                DEFAULT_ROWS,
            );
            pool.add("claude", claude_pty);

            // Store shared state so commands can access the PTY pool.
            app.manage(AppState { pty_pool: pool });

            // Initialize system tray
            tray::create_tray(app.handle())?;

            // Intercept the window close event: hide instead of quitting,
            // so the app keeps running in the system tray.
            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::pty_input,
            commands::pty_resize,
            commands::get_status,
            commands::restart_claude,
            commands::restart_wechat,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

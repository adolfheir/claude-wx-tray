use tauri::State;

use crate::state::{AppState, AppStatus, ConnectionStatus};

#[tauri::command]
pub fn pty_input(state: State<'_, AppState>, name: String, data: String) {
    state.pty_pool.write(&name, &data);
}

#[tauri::command]
pub fn pty_resize(state: State<'_, AppState>, name: String, cols: u16, rows: u16) {
    state.pty_pool.resize(&name, cols, rows);
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> AppStatus {
    let claude = if state.pty_pool.is_alive("claude") {
        ConnectionStatus::Connected
    } else {
        ConnectionStatus::Disconnected
    };

    let wechat = if state.pty_pool.is_alive("wechat") {
        ConnectionStatus::Connected
    } else {
        ConnectionStatus::Disconnected
    };

    AppStatus { claude, wechat }
}

#[tauri::command]
pub fn restart_claude(state: State<'_, AppState>) {
    // Use sensible default dimensions; the frontend will send a resize
    // immediately after reconnecting.
    state.pty_pool.restart("claude", 120, 40);
}

#[tauri::command]
pub fn restart_wechat(state: State<'_, AppState>) {
    state.pty_pool.restart("wechat", 120, 40);
}

use serde::{Deserialize, Serialize};

use crate::pty::PtyPool;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ConnectionStatus {
    #[serde(rename = "connected")]
    Connected,
    #[serde(rename = "connecting")]
    Reconnecting,
    #[serde(rename = "disconnected")]
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppStatus {
    pub claude: ConnectionStatus,
    pub wechat: ConnectionStatus,
}

/// Shared application state managed by Tauri.
///
/// Access it in commands via `tauri::State<'_, AppState>`.
pub struct AppState {
    pub pty_pool: PtyPool,
}

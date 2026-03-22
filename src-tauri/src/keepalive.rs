use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::task::JoinHandle;

use crate::state::{AppStatus, ConnectionStatus};

// ---------------------------------------------------------------------------
// Trait that PtyManager (implemented in pty.rs) must satisfy so keepalive can
// interact with it. The concrete PtyManager should implement this trait.
// ---------------------------------------------------------------------------

/// Trait abstracting PTY process operations needed by the keepalive monitor.
pub trait ProcessHealth: Send + Sync + 'static {
    /// Returns `true` if the underlying child process is still running.
    fn is_alive(&self) -> bool;

    /// Attempt to restart the child process.
    /// Returns `Ok(())` on success, or an error description on failure.
    fn restart(&self) -> Result<(), String>;

    /// Returns a snapshot of recent PTY output (since last call or last N bytes).
    /// The keepalive monitor inspects this for WeChat connection error keywords.
    fn recent_output(&self) -> String;
}

// ---------------------------------------------------------------------------
// Shared application state wrapper used by the keepalive monitor.
// ---------------------------------------------------------------------------

/// Shared state container that keepalive reads/writes.
pub struct SharedState {
    pub status: Mutex<AppStatus>,
    pub pty: Box<dyn ProcessHealth>,
}

impl SharedState {
    pub fn new(pty: Box<dyn ProcessHealth>) -> Self {
        Self {
            status: Mutex::new(AppStatus {
                claude: ConnectionStatus::Connected,
                wechat: ConnectionStatus::Connected,
            }),
            pty,
        }
    }
}

// ---------------------------------------------------------------------------
// KeepaliveMonitor
// ---------------------------------------------------------------------------

/// Monitors the Claude Code child process and WeChat connection, restarting
/// them with exponential backoff when they fail.
pub struct KeepaliveMonitor {
    /// Shared flag – set to `false` to gracefully stop the monitoring loop.
    running: Arc<AtomicBool>,
    /// Handle to the spawned tokio task so callers can await its completion.
    task_handle: Mutex<Option<JoinHandle<()>>>,
}

/// Backoff durations (in seconds) for retry attempts.
/// After the last entry the value is capped at 60 s.
const BACKOFF_SCHEDULE: [u64; 5] = [3, 6, 12, 30, 60];

/// Maximum consecutive failures before giving up and marking Disconnected.
const MAX_FAILURES: u32 = 5;

/// Keywords in PTY output that indicate a WeChat / ilink connection problem.
const WECHAT_ERROR_KEYWORDS: &[&str] = &[
    "error",
    "disconnected",
    "timeout",
    "ECONNREFUSED",
    "ECONNRESET",
    "ETIMEDOUT",
    "ilink",
    "websocket close",
    "connection lost",
];

impl KeepaliveMonitor {
    /// Create a new (idle) monitor. Call [`start`] to begin the loop.
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            task_handle: Mutex::new(None),
        }
    }

    // ------------------------------------------------------------------
    // Public API
    // ------------------------------------------------------------------

    /// Spawn the monitoring loop on the tokio runtime.
    ///
    /// * `app_handle` – used to emit Tauri events and send notifications.
    /// * `state`      – shared state holding PTY manager and connection status.
    pub fn start(&self, app_handle: AppHandle, state: Arc<SharedState>) {
        // Prevent double-start.
        if self.running.load(Ordering::SeqCst) {
            return;
        }
        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let handle = tokio::spawn(async move {
            monitor_loop(running, app_handle, state).await;
        });

        *self.task_handle.lock() = Some(handle);
    }

    /// Signal the monitoring loop to stop. Returns immediately; the loop will
    /// exit within one poll interval (~5 s).
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Internal monitoring loop
// ---------------------------------------------------------------------------

async fn monitor_loop(running: Arc<AtomicBool>, app_handle: AppHandle, state: Arc<SharedState>) {
    let mut claude_failures: u32 = 0;
    let mut wechat_failures: u32 = 0;

    // Track previous statuses so we only emit events on changes.
    let mut prev_claude = ConnectionStatus::Connected;
    let mut prev_wechat = ConnectionStatus::Connected;

    while running.load(Ordering::SeqCst) {
        // ---- Check Claude Code process health ----
        let claude_alive = state.pty.is_alive();

        if claude_alive {
            // Process is healthy – reset counter.
            if claude_failures > 0 {
                claude_failures = 0;
                set_claude_status(&state, ConnectionStatus::Connected);
                emit_if_changed(
                    &app_handle,
                    &state,
                    &mut prev_claude,
                    &mut prev_wechat,
                );
            }
        } else {
            // Process is down.
            claude_failures += 1;

            if claude_failures > MAX_FAILURES {
                // Exceeded retry budget – mark Disconnected.
                set_claude_status(&state, ConnectionStatus::Disconnected);
                emit_if_changed(
                    &app_handle,
                    &state,
                    &mut prev_claude,
                    &mut prev_wechat,
                );
                send_notification(
                    &app_handle,
                    "Claude Code 进程已断开连接",
                    &format!(
                        "连续 {} 次重启失败，请手动检查。",
                        claude_failures
                    ),
                );
            } else {
                // Still within budget – attempt restart with backoff.
                set_claude_status(&state, ConnectionStatus::Reconnecting);
                emit_if_changed(
                    &app_handle,
                    &state,
                    &mut prev_claude,
                    &mut prev_wechat,
                );

                let backoff = backoff_duration(claude_failures);
                tokio::time::sleep(backoff).await;

                match state.pty.restart() {
                    Ok(()) => {
                        claude_failures = 0;
                        set_claude_status(&state, ConnectionStatus::Connected);
                        emit_if_changed(
                            &app_handle,
                            &state,
                            &mut prev_claude,
                            &mut prev_wechat,
                        );
                    }
                    Err(_) => {
                        // Will retry next iteration.
                    }
                }
            }
        }

        // ---- Check WeChat connection via PTY output ----
        let output = state.pty.recent_output();
        let wechat_error_detected = detect_wechat_error(&output);

        if !wechat_error_detected {
            if wechat_failures > 0 {
                wechat_failures = 0;
                set_wechat_status(&state, ConnectionStatus::Connected);
                emit_if_changed(
                    &app_handle,
                    &state,
                    &mut prev_claude,
                    &mut prev_wechat,
                );
            }
        } else {
            wechat_failures += 1;

            if wechat_failures > MAX_FAILURES {
                set_wechat_status(&state, ConnectionStatus::Disconnected);
                emit_if_changed(
                    &app_handle,
                    &state,
                    &mut prev_claude,
                    &mut prev_wechat,
                );
                send_notification(
                    &app_handle,
                    "微信连接已断开",
                    &format!(
                        "连续 {} 次检测到连接异常，请手动检查。",
                        wechat_failures
                    ),
                );
            } else {
                set_wechat_status(&state, ConnectionStatus::Reconnecting);
                emit_if_changed(
                    &app_handle,
                    &state,
                    &mut prev_claude,
                    &mut prev_wechat,
                );

                let backoff = backoff_duration(wechat_failures);
                tokio::time::sleep(backoff).await;

                // For WeChat we also restart the PTY process, since the
                // relay bridge is the same process.
                match state.pty.restart() {
                    Ok(()) => {
                        wechat_failures = 0;
                        set_wechat_status(&state, ConnectionStatus::Connected);
                        emit_if_changed(
                            &app_handle,
                            &state,
                            &mut prev_claude,
                            &mut prev_wechat,
                        );
                    }
                    Err(_) => {
                        // Will retry next iteration.
                    }
                }
            }
        }

        // ---- Sleep until next check ----
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the backoff duration for a given failure count (1-indexed).
fn backoff_duration(failure_count: u32) -> Duration {
    let idx = (failure_count.saturating_sub(1) as usize).min(BACKOFF_SCHEDULE.len() - 1);
    Duration::from_secs(BACKOFF_SCHEDULE[idx])
}

/// Scan PTY output for WeChat / ilink connection error keywords.
fn detect_wechat_error(output: &str) -> bool {
    if output.is_empty() {
        return false;
    }
    let lower = output.to_lowercase();
    WECHAT_ERROR_KEYWORDS
        .iter()
        .any(|kw| lower.contains(&kw.to_lowercase()))
}

/// Update the Claude status inside shared state.
fn set_claude_status(state: &SharedState, status: ConnectionStatus) {
    state.status.lock().claude = status;
}

/// Update the WeChat status inside shared state.
fn set_wechat_status(state: &SharedState, status: ConnectionStatus) {
    state.status.lock().wechat = status;
}

/// Emit a `status-changed` Tauri event to the front-end **only** when either
/// the Claude or WeChat status has actually changed since the last emission.
fn emit_if_changed(
    app_handle: &AppHandle,
    state: &SharedState,
    prev_claude: &mut ConnectionStatus,
    prev_wechat: &mut ConnectionStatus,
) {
    let current = state.status.lock().clone();
    if current.claude != *prev_claude || current.wechat != *prev_wechat {
        *prev_claude = current.claude;
        *prev_wechat = current.wechat;
        // Best-effort: ignore emit errors (e.g. no listeners yet).
        let _ = app_handle.emit("status-changed", &current);
    }
}

/// Send a macOS system notification via tauri-plugin-notification.
fn send_notification(app_handle: &AppHandle, title: &str, body: &str) {
    // tauri-plugin-notification exposes a builder off the app handle.
    let _ = app_handle
        .notification()
        .builder()
        .title(format!("ClaudeWXTray - {}", title))
        .body(body)
        .show();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake ProcessHealth implementation for unit tests.
    struct FakePty {
        alive: AtomicBool,
        output: Mutex<String>,
        restart_result: Mutex<Result<(), String>>,
    }

    impl FakePty {
        fn new(alive: bool) -> Self {
            Self {
                alive: AtomicBool::new(alive),
                output: Mutex::new(String::new()),
                restart_result: Mutex::new(Ok(())),
            }
        }
    }

    impl ProcessHealth for FakePty {
        fn is_alive(&self) -> bool {
            self.alive.load(Ordering::SeqCst)
        }

        fn restart(&self) -> Result<(), String> {
            self.restart_result.lock().clone()
        }

        fn recent_output(&self) -> String {
            self.output.lock().clone()
        }
    }

    #[test]
    fn test_backoff_duration() {
        assert_eq!(backoff_duration(1), Duration::from_secs(3));
        assert_eq!(backoff_duration(2), Duration::from_secs(6));
        assert_eq!(backoff_duration(3), Duration::from_secs(12));
        assert_eq!(backoff_duration(4), Duration::from_secs(30));
        assert_eq!(backoff_duration(5), Duration::from_secs(60));
        // Beyond schedule length, stays capped at 60s.
        assert_eq!(backoff_duration(6), Duration::from_secs(60));
        assert_eq!(backoff_duration(100), Duration::from_secs(60));
    }

    #[test]
    fn test_detect_wechat_error_empty() {
        assert!(!detect_wechat_error(""));
    }

    #[test]
    fn test_detect_wechat_error_with_keyword() {
        assert!(detect_wechat_error("ilink API returned timeout"));
        assert!(detect_wechat_error("Connection DISCONNECTED from server"));
        assert!(detect_wechat_error("ECONNREFUSED 127.0.0.1:3000"));
    }

    #[test]
    fn test_detect_wechat_error_clean_output() {
        assert!(!detect_wechat_error("Everything is running fine"));
        assert!(!detect_wechat_error("Message sent successfully"));
    }

    #[test]
    fn test_shared_state_status_updates() {
        let pty = Box::new(FakePty::new(true));
        let state = SharedState::new(pty);

        assert_eq!(state.status.lock().claude, ConnectionStatus::Connected);

        set_claude_status(&state, ConnectionStatus::Reconnecting);
        assert_eq!(state.status.lock().claude, ConnectionStatus::Reconnecting);

        set_wechat_status(&state, ConnectionStatus::Disconnected);
        assert_eq!(state.status.lock().wechat, ConnectionStatus::Disconnected);
    }
}

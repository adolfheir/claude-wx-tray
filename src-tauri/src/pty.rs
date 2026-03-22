use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;

use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter};

/// Characters used in ASCII QR codes rendered by terminals.
const QR_BLOCK_CHARS: &[char] = &[
    '\u{2580}', // ▀ UPPER HALF BLOCK
    '\u{2584}', // ▄ LOWER HALF BLOCK
    '\u{2588}', // █ FULL BLOCK
    '\u{2591}', // ░ LIGHT SHADE
    '\u{2592}', // ▒ MEDIUM SHADE
    '\u{2593}', // ▓ DARK SHADE
    '\u{259A}', // ▚
    '\u{259E}', // ▞
];

/// Minimum number of QR block characters in a line to consider it part of a QR code.
const QR_CHAR_THRESHOLD: usize = 10;

/// Number of consecutive QR-like lines needed to trigger detection.
const QR_LINE_THRESHOLD: usize = 5;

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC [ ... (letter) sequences
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() || next == 'm' {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Checks whether a single line looks like part of an ASCII QR code.
fn line_looks_like_qr(line: &str) -> bool {
    let qr_char_count = line.chars().filter(|c| QR_BLOCK_CHARS.contains(c)).count();
    qr_char_count >= QR_CHAR_THRESHOLD
}

/// Internal mutable state that lives behind the lock.
struct PtyInner {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    cmd_string: String,
}

/// Thread-safe PTY manager for a single named PTY instance.
///
/// All public methods take `&self` so that the manager can be stored inside
/// Tauri's managed state without additional wrapping.
pub struct PtyManager {
    inner: Arc<Mutex<Option<PtyInner>>>,
    app_handle: AppHandle,
    /// The name of this PTY (e.g., "wechat" or "claude"), used for event routing.
    name: String,
}

impl PtyManager {
    /// Spawn a new PTY running `cmd_string` inside a shell.
    ///
    /// `name` is used to namespace Tauri events (e.g., `pty-output-wechat`).
    pub fn new(app_handle: AppHandle, name: &str, cmd_string: &str, cols: u16, rows: u16) -> Self {
        let inner = Arc::new(Mutex::new(None));
        let manager = Self {
            inner: inner.clone(),
            app_handle,
            name: name.to_string(),
        };
        manager.spawn_process(cmd_string, cols, rows);
        manager
    }

    // ── public API ─────────────────────────────────────────────────────

    /// Write raw bytes (typically key-strokes from xterm.js) into the PTY.
    pub fn write(&self, data: &str) {
        if let Some(ref mut inner) = *self.inner.lock() {
            let _ = inner.writer.write_all(data.as_bytes());
            let _ = inner.writer.flush();
        }
    }

    /// Resize the PTY to match the frontend terminal dimensions.
    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(ref inner) = *self.inner.lock() {
            let _ = inner.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    /// Returns `true` when the child process is still running.
    pub fn is_alive(&self) -> bool {
        if let Some(ref mut inner) = *self.inner.lock() {
            // try_wait returns Ok(Some(status)) when exited, Ok(None) when still running
            matches!(inner.child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    /// Kill the current process (if any) and start a fresh one with the
    /// same command.  Returns `true` on success.
    pub fn restart(&self, cols: u16, rows: u16) -> bool {
        let cmd_string = {
            let mut guard = self.inner.lock();
            if let Some(ref mut inner) = *guard {
                let _ = inner.child.kill();
                let _ = inner.child.try_wait();
                let cmd = inner.cmd_string.clone();
                *guard = None;
                cmd
            } else {
                return false;
            }
        };
        self.spawn_process(&cmd_string, cols, rows)
    }

    /// Kill the child process without restarting.
    pub fn kill(&self) {
        let mut guard = self.inner.lock();
        if let Some(ref mut inner) = *guard {
            let _ = inner.child.kill();
            let _ = inner.child.try_wait();
        }
        *guard = None;
    }

    // ── private helpers ────────────────────────────────────────────────

    /// Spawns the child process inside a new PTY and starts the reader task.
    fn spawn_process(&self, cmd_string: &str, cols: u16, rows: u16) -> bool {
        let pty_system = native_pty_system();

        let pty_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = match pty_system.openpty(pty_size) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[pty:{}] failed to open pty: {e}", self.name);
                return false;
            }
        };

        // Run through the user's default login shell so that PATH and
        // other env vars (e.g. from ~/.zshrc) are available.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.arg("-l");
        cmd.arg("-c");
        cmd.arg(cmd_string);

        // Set working directory to the channel/ folder so that `claude`
        // picks up the .mcp.json that configures the wechat channel plugin.
        let channel_dir = find_channel_dir();
        if let Some(dir) = channel_dir {
            cmd.cwd(dir);
        }

        let child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[pty:{}] failed to spawn command: {e}", self.name);
                return false;
            }
        };

        // We must drop the slave side in the parent – the child owns it.
        drop(pair.slave);

        let writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[pty:{}] failed to take writer: {e}", self.name);
                return false;
            }
        };

        // Get a reader for the background task *before* storing the master.
        let reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[pty:{}] failed to clone reader: {e}", self.name);
                return false;
            }
        };

        {
            let mut guard = self.inner.lock();
            *guard = Some(PtyInner {
                master: pair.master,
                child,
                writer,
                cmd_string: cmd_string.to_string(),
            });
        }

        // Spawn reader task.
        self.start_reader(reader);

        true
    }

    /// Spawns a background thread that reads from the PTY and emits Tauri
    /// events.  The thread exits automatically when the PTY reader returns
    /// EOF (i.e. the child exits or the master is dropped).
    fn start_reader(&self, mut reader: Box<dyn Read + Send>) {
        let app_handle = self.app_handle.clone();
        let inner = self.inner.clone();
        let event_name = format!("pty-output-{}", self.name);
        let pty_name = self.name.clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            // Rolling window of recent lines for QR detection.
            let mut recent_qr_lines: usize = 0;
            let mut partial_line = String::new();

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF – child has exited.
                        break;
                    }
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]).to_string();

                        // Emit output to frontend with named event.
                        let _ = app_handle.emit(&event_name, &chunk);

                        // ── Auto-accept known prompts ────────────
                        // Strip ANSI escape codes and check for prompts.
                        let clean = strip_ansi(&chunk);
                        if clean.contains("Esc to cancel") || clean.contains("Enter to confirm") {
                            // Small delay to let the TUI render fully.
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            let mut guard = inner.lock();
                            if let Some(ref mut pty_inner) = *guard {
                                let _ = pty_inner.writer.write_all(b"\r");
                                let _ = pty_inner.writer.flush();
                            }
                        }

                        // ── QR code detection ──────────────────────
                        partial_line.push_str(&chunk);

                        // Process complete lines.
                        while let Some(pos) = partial_line.find('\n') {
                            let line: String = partial_line.drain(..=pos).collect();
                            if line_looks_like_qr(&line) {
                                recent_qr_lines += 1;
                            } else {
                                recent_qr_lines = 0;
                            }
                            if recent_qr_lines >= QR_LINE_THRESHOLD {
                                let _ = app_handle.emit("qr-detected", ());
                                recent_qr_lines = 0;
                            }
                        }

                        // Avoid unbounded growth of the partial buffer.
                        if partial_line.len() > 8192 {
                            partial_line.clear();
                        }
                    }
                    Err(e) => {
                        eprintln!("[pty:{}] read error: {e}", pty_name);
                        break;
                    }
                }
            }

            // Mark process as gone so `is_alive` reflects reality quickly.
            let mut guard = inner.lock();
            if let Some(ref mut inner) = *guard {
                let _ = inner.child.try_wait();
            }
        });
    }
}

/// Detect the channel/ directory relative to the current executable.
///
/// In development the exe is deep inside src-tauri/target/…
/// Walk up until we find the project root (has channel/).
pub fn find_channel_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .and_then(|mut d| {
            for _ in 0..10 {
                if d.join("channel").exists() {
                    return Some(d.join("channel"));
                }
                if !d.pop() {
                    break;
                }
            }
            None
        })
}

// ── PtyPool: manages multiple named PTYs ──────────────────────────────

/// A pool of named PTY instances. Each PTY is identified by a string key
/// (e.g., "wechat", "claude").
pub struct PtyPool {
    ptys: HashMap<String, PtyManager>,
}

impl PtyPool {
    pub fn new() -> Self {
        Self {
            ptys: HashMap::new(),
        }
    }

    /// Add a new named PTY to the pool.
    pub fn add(&mut self, name: &str, pty: PtyManager) {
        self.ptys.insert(name.to_string(), pty);
    }

    /// Get a reference to a named PTY.
    pub fn get(&self, name: &str) -> Option<&PtyManager> {
        self.ptys.get(name)
    }

    /// Write data to a named PTY.
    pub fn write(&self, name: &str, data: &str) {
        if let Some(pty) = self.ptys.get(name) {
            pty.write(data);
        }
    }

    /// Resize a named PTY.
    pub fn resize(&self, name: &str, cols: u16, rows: u16) {
        if let Some(pty) = self.ptys.get(name) {
            pty.resize(cols, rows);
        }
    }

    /// Check if a named PTY's child process is alive.
    pub fn is_alive(&self, name: &str) -> bool {
        self.ptys.get(name).map_or(false, |pty| pty.is_alive())
    }

    /// Restart a named PTY.
    pub fn restart(&self, name: &str, cols: u16, rows: u16) -> bool {
        self.ptys
            .get(name)
            .map_or(false, |pty| pty.restart(cols, rows))
    }

    /// Kill all PTYs in the pool.
    pub fn kill_all(&self) {
        for pty in self.ptys.values() {
            pty.kill();
        }
    }
}

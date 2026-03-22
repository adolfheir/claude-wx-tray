import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ---- Type definitions ----

export type ConnectionStatus = "connected" | "connecting" | "disconnected";

export interface AppStatus {
  claude: ConnectionStatus;
  wechat: ConnectionStatus;
}

/** Valid PTY names matching the Rust backend pool keys. */
export type PtyName = "wechat" | "claude";

// ---- Command wrappers ----

export async function ptyInput(name: PtyName, data: string): Promise<void> {
  await invoke("pty_input", { name, data });
}

export async function ptyResize(
  name: PtyName,
  cols: number,
  rows: number
): Promise<void> {
  await invoke("pty_resize", { name, cols, rows });
}

export async function getStatus(): Promise<AppStatus> {
  return await invoke<AppStatus>("get_status");
}

export async function restartClaude(): Promise<void> {
  await invoke("restart_claude");
}

export async function restartWechat(): Promise<void> {
  await invoke("restart_wechat");
}

// ---- Event listeners ----

/**
 * Listen for PTY output from a named PTY instance.
 * Events are emitted as `pty-output-{name}` from the Rust backend.
 */
export function onPtyOutput(
  name: PtyName,
  callback: (data: string) => void
): Promise<UnlistenFn> {
  return listen<string>(`pty-output-${name}`, (event) => {
    callback(event.payload);
  });
}

export function onStatusChanged(
  callback: (status: AppStatus) => void
): Promise<UnlistenFn> {
  return listen<AppStatus>("status-changed", (event) => {
    callback(event.payload);
  });
}

export function onQrDetected(
  callback: (qrData: string) => void
): Promise<UnlistenFn> {
  return listen<string>("qr-detected", (event) => {
    callback(event.payload);
  });
}

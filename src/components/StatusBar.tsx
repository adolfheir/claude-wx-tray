"use client";

import { useEffect, useState } from "react";
import type { AppStatus, ConnectionStatus } from "../lib/tauri";

const STATUS_COLORS: Record<ConnectionStatus, string> = {
  connected: "#4caf50",
  connecting: "#ff9800",
  disconnected: "#f44336",
};

const STATUS_LABELS: Record<ConnectionStatus, string> = {
  connected: "已连接",
  connecting: "连接中",
  disconnected: "未连接",
};

export default function StatusBar() {
  const [status, setStatus] = useState<AppStatus>({
    claude: "disconnected",
    wechat: "disconnected",
  });
  const [tauriReady, setTauriReady] = useState(false);

  useEffect(() => {
    setTauriReady(true);
  }, []);

  useEffect(() => {
    if (!tauriReady) return;

    let unlisten: (() => void) | undefined;

    (async () => {
      try {
        const { getStatus, onStatusChanged } = await import("../lib/tauri");

        // Fetch initial status
        try {
          const initialStatus = await getStatus();
          setStatus(initialStatus);
        } catch {
          // Backend may not be ready yet
        }

        // Listen for status changes
        const unlistenFn = await onStatusChanged((newStatus) => {
          setStatus(newStatus);
        });
        unlisten = unlistenFn;
      } catch {
        // Tauri not available (e.g. during SSR or dev without Tauri)
      }
    })();

    return () => {
      unlisten?.();
    };
  }, [tauriReady]);

  const handleRestartClaude = async () => {
    try {
      const { restartClaude } = await import("../lib/tauri");
      await restartClaude();
    } catch {
      // ignore
    }
  };

  const handleRestartWechat = async () => {
    try {
      const { restartWechat } = await import("../lib/tauri");
      await restartWechat();
    } catch {
      // ignore
    }
  };

  return (
    <div style={styles.bar}>
      <div style={styles.section}>
        <span
          style={{
            ...styles.dot,
            backgroundColor: STATUS_COLORS[status.claude],
          }}
        />
        <span style={styles.label}>
          Claude Code: {STATUS_LABELS[status.claude]}
        </span>
        <button style={styles.button} onClick={handleRestartClaude}>
          重启
        </button>
      </div>

      <div style={styles.section}>
        <span
          style={{
            ...styles.dot,
            backgroundColor: STATUS_COLORS[status.wechat],
          }}
        />
        <span style={styles.label}>
          微信: {STATUS_LABELS[status.wechat]}
        </span>
        <button style={styles.button} onClick={handleRestartWechat}>
          重启
        </button>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  bar: {
    height: 40,
    minHeight: 40,
    display: "flex",
    alignItems: "center",
    justifyContent: "flex-start",
    gap: 24,
    padding: "0 16px",
    backgroundColor: "#252526",
    borderBottom: "1px solid #3c3c3c",
    userSelect: "none",
  },
  section: {
    display: "flex",
    alignItems: "center",
    gap: 8,
  },
  dot: {
    width: 8,
    height: 8,
    borderRadius: "50%",
    flexShrink: 0,
  },
  label: {
    fontSize: 13,
    color: "#cccccc",
    whiteSpace: "nowrap",
  },
  button: {
    fontSize: 12,
    padding: "2px 10px",
    border: "1px solid #555",
    borderRadius: 3,
    backgroundColor: "#333",
    color: "#ccc",
    cursor: "pointer",
    lineHeight: "20px",
  },
};

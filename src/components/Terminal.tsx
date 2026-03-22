"use client";

import { useEffect, useRef, useState, useCallback } from "react";
import type { PtyName } from "../lib/tauri";
import "@xterm/xterm/css/xterm.css";

interface TabConfig {
  name: PtyName;
  label: string;
}

const TABS: TabConfig[] = [
  { name: "claude", label: "Claude Code" },
  { name: "wechat", label: "微信登录" },
];

export default function TerminalComponent() {
  const [activeTab, setActiveTab] = useState<PtyName>("claude");
  const initialized = useRef(false);

  // Refs for each tab's terminal and container
  const wechatContainerRef = useRef<HTMLDivElement>(null);
  const claudeContainerRef = useRef<HTMLDivElement>(null);

  const wechatTermRef = useRef<import("@xterm/xterm").Terminal | null>(null);
  const claudeTermRef = useRef<import("@xterm/xterm").Terminal | null>(null);

  const wechatFitRef = useRef<import("@xterm/addon-fit").FitAddon | null>(null);
  const claudeFitRef = useRef<import("@xterm/addon-fit").FitAddon | null>(null);

  // Fit the active terminal when tab changes or window resizes
  const fitActive = useCallback(() => {
    requestAnimationFrame(() => {
      if (activeTab === "wechat" && wechatFitRef.current) {
        wechatFitRef.current.fit();
      } else if (activeTab === "claude" && claudeFitRef.current) {
        claudeFitRef.current.fit();
      }
    });
  }, [activeTab]);

  useEffect(() => {
    if (initialized.current) return;
    initialized.current = true;

    const unlisteners: (() => void)[] = [];

    (async () => {
      const { Terminal } = await import("@xterm/xterm");
      const { FitAddon } = await import("@xterm/addon-fit");

      const termTheme = {
        background: "#1e1e1e",
        foreground: "#d4d4d4",
        cursor: "#d4d4d4",
        selectionBackground: "#264f78",
        black: "#1e1e1e",
        red: "#f44747",
        green: "#6a9955",
        yellow: "#d7ba7d",
        blue: "#569cd6",
        magenta: "#c586c0",
        cyan: "#4ec9b0",
        white: "#d4d4d4",
        brightBlack: "#808080",
        brightRed: "#f44747",
        brightGreen: "#6a9955",
        brightYellow: "#d7ba7d",
        brightBlue: "#569cd6",
        brightMagenta: "#c586c0",
        brightCyan: "#4ec9b0",
        brightWhite: "#ffffff",
      };

      const termOptions = {
        cursorBlink: true,
        fontSize: 14,
        fontFamily: "'Menlo', 'Monaco', 'Courier New', monospace",
        theme: termTheme,
        allowProposedApi: true,
        scrollback: 10000,
      };

      // Helper to set up a single terminal tab
      const setupTerminal = async (
        name: PtyName,
        container: HTMLDivElement | null,
        termRef: React.MutableRefObject<import("@xterm/xterm").Terminal | null>,
        fitRef: React.MutableRefObject<import("@xterm/addon-fit").FitAddon | null>
      ) => {
        if (!container) return;

        const fitAddon = new FitAddon();
        fitRef.current = fitAddon;

        const term = new Terminal(termOptions);
        termRef.current = term;

        term.loadAddon(fitAddon);
        term.open(container);

        // Initial fit
        requestAnimationFrame(() => {
          fitAddon.fit();
        });

        try {
          const { ptyInput, ptyResize, onPtyOutput } = await import(
            "../lib/tauri"
          );

          // Listen for pty output from the named PTY
          const unlistenFn = await onPtyOutput(name, (data: string) => {
            term.write(data);
          });
          unlisteners.push(unlistenFn);

          // Send user input to the named PTY
          term.onData((data: string) => {
            ptyInput(name, data).catch(() => {
              // ignore errors if backend not ready
            });
          });

          // Notify backend on resize for this PTY
          term.onResize(({ cols, rows }) => {
            ptyResize(name, cols, rows).catch(() => {
              // ignore errors if backend not ready
            });
          });

          // Send initial size to backend
          ptyResize(name, term.cols, term.rows).catch(() => {});
        } catch {
          term.writeln("Waiting for Tauri backend...");
        }
      };

      // Set up both terminals
      await setupTerminal(
        "wechat",
        wechatContainerRef.current,
        wechatTermRef,
        wechatFitRef
      );
      await setupTerminal(
        "claude",
        claudeContainerRef.current,
        claudeTermRef,
        claudeFitRef
      );
    })();

    return () => {
      for (const fn of unlisteners) {
        fn();
      }
      if (wechatTermRef.current) {
        wechatTermRef.current.dispose();
        wechatTermRef.current = null;
      }
      if (claudeTermRef.current) {
        claudeTermRef.current.dispose();
        claudeTermRef.current = null;
      }
    };
  }, []);

  // Re-fit when active tab changes
  useEffect(() => {
    fitActive();
  }, [fitActive]);

  // Handle window resize
  useEffect(() => {
    const handler = () => fitActive();
    window.addEventListener("resize", handler);
    return () => window.removeEventListener("resize", handler);
  }, [fitActive]);

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      {/* Tab bar */}
      <div style={styles.tabBar}>
        {TABS.map((tab) => (
          <button
            key={tab.name}
            style={{
              ...styles.tab,
              ...(activeTab === tab.name ? styles.tabActive : {}),
            }}
            onClick={() => setActiveTab(tab.name)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Terminal containers — both always mounted, only active one visible */}
      <div style={{ flex: 1, position: "relative", overflow: "hidden" }}>
        <div
          ref={wechatContainerRef}
          style={{
            ...styles.terminalContainer,
            display: activeTab === "wechat" ? "block" : "none",
          }}
        />
        <div
          ref={claudeContainerRef}
          style={{
            ...styles.terminalContainer,
            display: activeTab === "claude" ? "block" : "none",
          }}
        />
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  tabBar: {
    display: "flex",
    backgroundColor: "#252526",
    borderBottom: "1px solid #3c3c3c",
    minHeight: 36,
    alignItems: "stretch",
    paddingLeft: 8,
    gap: 0,
    userSelect: "none",
  },
  tab: {
    padding: "6px 20px",
    fontSize: 13,
    color: "#969696",
    backgroundColor: "transparent",
    border: "none",
    borderBottomWidth: 2,
    borderBottomStyle: "solid",
    borderBottomColor: "transparent",
    cursor: "pointer",
    outline: "none",
    transition: "color 0.15s, border-color 0.15s",
    whiteSpace: "nowrap",
  },
  tabActive: {
    color: "#ffffff",
    borderBottomColor: "#569cd6",
    backgroundColor: "#1e1e1e",
  },
  terminalContainer: {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    width: "100%",
    height: "100%",
    overflow: "hidden",
    backgroundColor: "#1e1e1e",
  },
};

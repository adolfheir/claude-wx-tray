# ClaudeWXRelay

[中文](#中文) | [English](#english)

---

## 中文

一个 macOS 桌面应用，将 [claude-code-wechat-channel](https://github.com/Johnixr/claude-code-wechat-channel)（微信 ↔ Claude Code 桥接）包装为带 GUI 的桌面工具，提供进程保活和可视化管理。

### 功能

- **系统菜单栏常驻** — 状态栏图标显示连接状态（绿/黄/红）
- **内嵌终端** — xterm.js 完整终端，运行并展示 Claude Code 会话
- **Claude Code 进程保活** — 自动重连，指数退避重试，连续失败后系统通知
- **微信连接保活** — 自动重连，连续失败后系统通知
- **二维码自动弹窗** — 检测到微信登录二维码时自动打开窗口供扫码

### 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri v2 |
| 前端 | Next.js (静态导出) + xterm.js |
| 后端 | Rust (PTY 管理、进程保活、系统通知) |
| 平台 | macOS (先行)，后续计划支持 Windows |

### 架构

```
┌─ System Tray ────────────────────────┐
│  图标（绿/黄/红）+ 右键菜单           │
└──────────────────────────────────────┘
         │
┌─ Tauri 窗口 ─────────────────────────┐
│  ┌─ StatusBar ─────────────────────┐ │
│  │  [●] Claude: 已连接  [●] 微信   │ │
│  ├─ xterm.js Terminal ─────────────┤ │
│  │  PTY 输出 / Claude Code 交互    │ │
│  └─────────────────────────────────┘ │
└──────────────────────────────────────┘
         │ Tauri IPC
┌─ Rust 后端 ──────────────────────────┐
│  PTY 管理 · 进程保活 · 状态广播      │
│  系统通知 · 二维码检测               │
└──────────────────────────────────────┘
```

### 前置条件

- macOS
- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://rustup.rs/)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) 已安装并可用
- [claude-code-wechat-channel](https://github.com/Johnixr/claude-code-wechat-channel) 环境已配置

### 快速开始

```bash
# 克隆仓库
git clone https://github.com/adolfheir/ClaudeWXRelay.git
cd ClaudeWXRelay

# 安装前端依赖
npm install

# 安装 channel 依赖
cd channel && npm install && cd ..

# 开发模式运行
npm run tauri dev

# 构建生产版本
npm run tauri build
```

### 使用方式

1. 启动应用后，常驻系统菜单栏
2. 首次运行会显示微信登录二维码，扫码登录
3. 登录后窗口可关闭，应用在后台保持运行
4. 通过菜单栏图标颜色查看连接状态
5. 右键菜单可重启服务或退出应用

### 许可证

MIT

---

## English

A macOS desktop app that wraps [claude-code-wechat-channel](https://github.com/Johnixr/claude-code-wechat-channel) (WeChat ↔ Claude Code bridge) into a GUI tool with process keep-alive and visual management.

### Features

- **System Tray** — Status icon (green/yellow/red) with context menu
- **Embedded Terminal** — Full xterm.js terminal running Claude Code sessions
- **Claude Code Keep-Alive** — Auto-reconnect with exponential backoff, system notifications on failure
- **WeChat Keep-Alive** — Auto-reconnect with system notifications on failure
- **QR Code Auto-Popup** — Automatically opens window when WeChat login QR code is detected

### Tech Stack

| Layer | Technology |
|---|---|
| Desktop | Tauri v2 |
| Frontend | Next.js (static export) + xterm.js |
| Backend | Rust (PTY management, keep-alive, system notifications) |
| Platform | macOS (primary), Windows planned |

### Architecture

```
┌─ System Tray ────────────────────────┐
│  Icon (green/yellow/red) + menu      │
└──────────────────────────────────────┘
         │
┌─ Tauri Window ───────────────────────┐
│  ┌─ StatusBar ─────────────────────┐ │
│  │  [●] Claude: Connected  [●] WX  │ │
│  ├─ xterm.js Terminal ─────────────┤ │
│  │  PTY output / Claude Code       │ │
│  └─────────────────────────────────┘ │
└──────────────────────────────────────┘
         │ Tauri IPC
┌─ Rust Backend ───────────────────────┐
│  PTY · Keep-alive · State broadcast  │
│  Notifications · QR detection        │
└──────────────────────────────────────┘
```

### Prerequisites

- macOS
- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://rustup.rs/)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed and available
- [claude-code-wechat-channel](https://github.com/Johnixr/claude-code-wechat-channel) environment configured

### Quick Start

```bash
# Clone the repository
git clone https://github.com/adolfheir/ClaudeWXRelay.git
cd ClaudeWXRelay

# Install frontend dependencies
npm install

# Install channel dependencies
cd channel && npm install && cd ..

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

### Usage

1. After launch, the app stays in the system tray
2. On first run, a WeChat login QR code is displayed — scan to log in
3. After login, the window can be closed; the app keeps running in the background
4. Check connection status via the tray icon color
5. Right-click the tray icon to restart services or quit

### License

MIT

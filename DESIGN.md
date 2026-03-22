# ClaudeWXRelay 设计文档

## 概述

一个 macOS 桌面应用，将 [claude-code-wechat-channel](https://github.com/Johnixr/claude-code-wechat-channel)（微信 ↔ Claude Code 桥接）包装为带 GUI 的桌面工具，提供保活机制和可视化管理。

## 目标用户

面向技术用户的开源工具。用户已有 Claude Code 和 ClawBot 环境。

## 核心功能

1. **系统菜单栏常驻** — 状态栏图标显示连接状态（绿/黄/红）+ 右键菜单
2. **独立窗口** — 内嵌 xterm.js 完整终端，运行并展示 Claude Code 会话
3. **Claude Code 进程保活** — 静默自动重连，连续失败后系统通知
4. **微信连接保活** — 静默自动重连，连续失败后系统通知
5. **二维码自动弹窗** — 检测到微信二维码时自动打开窗口供扫码

## 非目标

- 不做聊天 UI 或消息格式化面板
- 不负责安装 Claude Code / ClawBot 依赖
- 不做大众用户引导
- 暂不支持 Windows（后续计划）

## 技术栈

- **桌面框架:** Tauri v2
- **前端:** Next.js（静态导出）+ xterm.js
- **后端:** Rust（PTY 管理、进程保活、系统通知）
- **平台:** macOS（先行），后续加 Windows

## 架构

```
┌─ macOS System Tray ──────────────────┐
│  图标（绿/黄/红）+ 右键菜单           │
│  - 打开窗口 / 重启 / 退出             │
└──────────────────────────────────────┘
         │
┌─ Tauri 窗口 ─────────────────────────┐
│  Next.js 前端                         │
│  ┌─ StatusBar ─────────────────────┐ │
│  │  [●] Claude: 已连接  [●] 微信    │ │
│  ├─ xterm.js ──────────────────────┤ │
│  │  PTY 输出渲染                    │ │
│  │  Claude Code 完整终端交互        │ │
│  └─────────────────────────────────┘ │
└──────────────────────────────────────┘
         │ Tauri IPC (events)
┌─ Rust 后端 ──────────────────────────┐
│  1. PTY 管理 - 启动/监听 Claude Code │
│  2. 进程保活 - 监控 + 自动重启       │
│  3. 微信连接保活 - 监控 ilink 轮询   │
│  4. 状态广播 - 通过 event 通知前端   │
│  5. 系统通知 - 失败时调用原生通知    │
│  6. 二维码检测 - 自动弹出窗口       │
└──────────────────────────────────────┘
```

## 项目结构

```
ClaudeWXRelay/
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── src/
│   │   ├── main.rs              # 入口，初始化 tray + 窗口
│   │   ├── tray.rs              # 系统菜单栏图标和菜单管理
│   │   ├── pty.rs               # PTY 创建和管理
│   │   ├── keepalive.rs         # 保活监控逻辑
│   │   ├── state.rs             # 全局状态管理
│   │   └── commands.rs          # Tauri IPC 命令
│   └── icons/                   # tray 图标（绿/黄/红）
├── src/                         # Next.js 前端
│   ├── app/
│   │   ├── layout.tsx
│   │   └── page.tsx             # 主页面
│   ├── components/
│   │   ├── Terminal.tsx          # xterm.js 封装
│   │   └── StatusBar.tsx        # 连接状态显示
│   └── lib/
│       └── tauri.ts             # Tauri IPC 封装
├── next.config.js               # 静态导出配置
├── package.json
└── README.md
```

## 状态管理

```rust
enum ConnectionStatus {
    Connected,      // 绿色图标
    Reconnecting,   // 黄色图标
    Disconnected,   // 红色图标
}

struct AppState {
    claude_status: ConnectionStatus,
    wechat_status: ConnectionStatus,
    pty_pair: Option<PtyPair>,
    retry_count: u32,
}
```

## Tauri IPC 接口

| 命令 | 方向 | 说明 |
|---|---|---|
| `pty-input` | 前端→Rust | 用户终端输入 |
| `pty-output` | Rust→前端 | 终端输出（event 推送） |
| `get-status` | 前端→Rust | 获取当前连接状态 |
| `status-changed` | Rust→前端 | 状态变更通知（event） |
| `restart-claude` | 前端→Rust | 手动重启 Claude Code |
| `restart-wechat` | 前端→Rust | 手动重连微信 |

## 保活策略

```
监控循环（每 5 秒检查一次）:
  ├─ 检查 Claude Code 进程是否存活
  │   └─ 死亡 → 状态设为 Reconnecting → 自动重启 PTY
  │         → 重试间隔: 3s, 6s, 12s, 30s, 60s（指数退避，上限 60s）
  │         → 连续失败 5 次 → 状态设为 Disconnected → 系统通知
  │
  └─ 检查微信连接（通过监控 PTY 输出中的错误关键词）
      └─ 检测到断开 → 同样的重试策略
```

## 启动流程

```
应用启动
  ├─ 初始化系统 tray（图标 + 菜单）
  ├─ 窗口默认隐藏
  ├─ 创建 PTY
  ├─ 检查微信凭证是否存在（~/.claude/channels/wechat/account.json）
  │   ├─ 存在 → 直接启动 Claude Code + 微信 channel
  │   └─ 不存在 → 先运行 setup.ts → 弹出窗口显示二维码
  ├─ 启动保活监控循环
  └─ 进入后台运行
```

## 微信登录流程

1. 应用启动 → 后台运行，窗口默认隐藏
2. Rust 监控 PTY 输出，检测到二维码特征（qrcode-terminal 的 ASCII 输出模式）
3. 自动弹出/聚焦 xterm 窗口，用户扫码
4. 扫码成功后，窗口可由用户自行关闭（回到后台运行）
5. 凭证缓存，下次启动无需重新扫码
6. 凭证过期时，再次触发二维码 → 自动弹窗

## 退出流程

```
用户点击"退出"
  ├─ 停止保活监控
  ├─ 向 PTY 发送终止信号，优雅关闭 Claude Code
  ├─ 等待最多 5 秒 → 超时强制 kill
  ├─ 关闭 PTY
  └─ 退出应用
```

## 窗口行为

- 关闭窗口 ≠ 退出应用，仅隐藏窗口，进程继续运行
- 点击 tray 图标或菜单"打开窗口" → 显示窗口
- 只有点"退出"才真正退出

## 系统菜单栏

**图标颜色:**
- 两个都连接 → 绿色
- 任一重连中 → 黄色
- 任一断开 → 红色

**右键菜单:**
```
Claude Code: 已连接
微信: 已连接
──────────────
打开窗口
重启 Claude Code
重启微信连接
──────────────
开机自启
退出
```

## 开机自启

通过 tray 菜单可勾选"开机自启"，macOS 使用 launchd 注册 Login Item。

## 决策日志

| # | 决策 | 备选方案 | 选择原因 |
|---|---|---|---|
| 1 | 保活覆盖 Claude Code + 微信连接 | 仅保活其中一个 | 完整无人值守运行 |
| 2 | 菜单栏图标 + 独立窗口 | 纯菜单栏 / 纯 Dock | 终端需要大显示区域，同时常驻后台 |
| 3 | 内嵌 xterm.js 终端 | 消息面板 / 系统终端 | 一体化体验，完整渲染 Claude Code 输出 |
| 4 | Tauri v2 | Electron / Swift | 性能与跨平台平衡 |
| 5 | Next.js 前端 | React / Vue / Svelte | 用户偏好 |
| 6 | Rust 直接管理 PTY | Node sidecar | 架构简单，保活可靠 |
| 7 | 静默重连 + 失败通知 | 手动重连 / 纯静默 | 无人值守但需知道故障 |
| 8 | 二维码自动弹窗 | 手动打开 / 单独弹窗 | 减少操作，流程自然 |
| 9 | 先 macOS 后 Windows | 同时开发 | 降低初期复杂度 |

## 假设

- 用户已有 Claude Code 和 ClawBot 环境
- 微信扫码登录在内嵌终端中完成
- 保活采用指数退避重试策略
- 使用 Tauri v2（对 system tray 支持更好）

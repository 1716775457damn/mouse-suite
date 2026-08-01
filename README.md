# Mouse Suite

Windows-first desktop automation suite that combines:

- **Recorder** – visual element library (name + preview) into SQLite
- **Clicker** – CSV / workflow auto-click with template matching
- **Flow editor** – visual drag-and-drop; pick recorded elements from gallery
- **操作文档** – clickscribe-style click → screenshot → annotate → export HTML/MD/JSON
- **Agent bridge** – file-based JSON command API for external agents

## Flow editor shortcuts

| Action | Shortcut |
|--------|----------|
| Multi-select | Ctrl+click / Ctrl+drag marquee |
| Select all | Ctrl+A |
| Copy / Paste | Ctrl+C / Ctrl+V |
| Undo / Redo | Ctrl+Z / Ctrl+Y |
| Auto layout | Ctrl+L |
| Delete | Delete |
| Pan canvas | Drag empty space |
| Start workflow (global) | Ctrl+Alt+F9 |
| Stop (global) | Ctrl+Alt+F10 |

## Flow Markdown + example

- Save/Open as **`.md`**: Mermaid diagram + `mouse-suite-flow` JSON block (Agent round-trip).
- Built-in sample **视觉成功点击×10**: Flow tab →「加载示例」, or `workflows/examples/vision-click-10.md`.
- Semantics: loop until **10 successful** vision matches+clicks; miss retries without consuming the count (`否` → back to 视觉条件).

## Vision & loops

- **Loop**: add `循环开始` / `循环结束` nodes; set times on the start node.
- **条件循环**: `条件循环` + `循环结束` — continue while template matches (`max_times` cap).
- **视觉条件**: dual ports (是/否) — match-only branch, no click. `否` 接回本节点 = 未达标重试不占次数。
- **Threshold**: clicker global slider, or per-click-node slider.
- **Pure vision**: only click when template match succeeds (global toggle + per-node override).
- **Retries**: per-node / global retry count + interval; fail policy `skip` or `abort`.
- **Match debug**: logs include best score; optional miss screenshots under `data/debug/` (or element folder `debug/`).
- **Screenshot on click node**: hides the app first, waits `hide_wait_ms` (config / recorder UI, 500–3000), then captures and binds the template.
- **Element picker**: click / if / while nodes can pick recorded element names from a dropdown.
- **Run highlight**: while a flow runs, the active node is highlighted on the canvas (stays on Flow tab).
- **Run HUD**: main window hides; a slim always-on-top bar at the top of the screen shows step `N/M`, label, progress, and Stop (Continue on pause/manual).
- **Property undo**: inspector edits participate in Ctrl+Z.

## 操作文档（文档页）

1. 打开 **文档** Tab → **开始录制**（或全局 **F8**；默认会最小化主窗口）
2. 在目标软件里左键点击（点击本程序窗口会被忽略）
3. **停止录制**（F8 / **Ctrl+Alt+F10**）后编辑标题/说明
4. **AI 写说明**（「AI 设置」对齐 clickscribe：CC Switch `:15721` Anthropic / 智谱 / 自定义；配置在 `data/ai_config.json`）
5. **生成流程图** → 自动切到流程图页（人工介入节点草稿）
6. 导出 **HTML** / **Markdown** / **JSON**；会话在 `data/scribe_sessions/`
7. 会话右键可 **复制 / 删除**

## 打包 / 安装包

推送 `v*` tag 后，GitHub Actions 会构建多端安装包：

| 平台 | 产物 |
|------|------|
| Windows | `*-setup.exe`（Inno Setup）+ portable zip |
| macOS | `*.dmg`（拖到 Applications）+ `.app.zip` |
| Linux | `*.deb` + tar.gz |

本地 Windows 便携包：

```powershell
.\scripts\package-release.ps1
```

本地 Windows 安装包（需 [Inno Setup 6](https://jrsoftware.org/isinfo.php)）：

```powershell
cargo build --release
.\scripts\package-windows-installer.ps1 -Version 0.2.3
```

## Requirements

- Rust 1.80+
- Windows 10/11 for full mouse automation features
- Linux / macOS builds are experimental (GUI available; click injection is stubbed)

## Build

```bash
cargo build --release
```

Binary:

- Windows: `target/release/mouse-suite.exe`
- Unix: `target/release/mouse-suite`

Release assets also live under `dist/`.

## Agent bridge

See [AGENT_BRIDGE.md](AGENT_BRIDGE.md).

Commands are exchanged via:

- `data/agent_command.json`
- `data/agent_response.json`

## License

MIT

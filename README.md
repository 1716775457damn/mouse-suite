# Mouse Suite

Windows-first desktop automation suite that combines:

- **Recorder** – capture UI element templates into SQLite
- **Clicker** – CSV / workflow auto-click with template matching
- **Flow editor** – visual drag-and-drop workflow graph
- **Agent bridge** – file-based JSON command API for external agents

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

## Agent bridge

See [AGENT_BRIDGE.md](AGENT_BRIDGE.md).

Commands are exchanged via:

- `data/agent_command.json`
- `data/agent_response.json`

## License

MIT

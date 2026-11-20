# Agent Usage Monitor (AUM)

A Windows desktop quota monitor built with Rust and Slint. The workspace contains a provider-neutral core, Codex app-server integration, an opt-in Claude Code status-line bridge, and the native Windows UI.

## Build and run

Install a stable Rust toolchain with the `x86_64-pc-windows-msvc` target and Windows SDK / Visual Studio C++ Build Tools. From `rust/`:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo run -p agent-usage-monitor-ui --bin agent-usage-monitor -- --demo
```

Use `--live` for live Codex data. Claude integration is opt-in with `--enable-claude-bridge`; see [`docs/claude-status-line.md`](docs/claude-status-line.md). Demo values are labeled as samples. Settings and alert state are stored locally under `%APPDATA%\Agent Usage Monitor`.

## Project layout

- `rust/agent-monitor-core`: provider models, Codex integration, alerts, and Claude bridge.
- `rust/agent-monitor-ui`: Slint UI, settings, localization, themes, and Windows tray integration.
- `docs/`: essential integration and architecture research notes.

The executable is `agent-usage-monitor.exe`. See `rust/agent-monitor-ui/README.md` for current features and limitations.

# Agent Usage Monitor (AUM)

**A lightweight, open-source Windows desktop monitor for AI coding-assistant usage, built with Rust and Slint.**

[![License: MIT](https://img.shields.io/badge/License-MIT-15243D?style=flat&labelColor=15243D)](LICENSE) ![Platform: Windows](https://img.shields.io/badge/Platform-Windows-15243D?style=flat&labelColor=15243D) ![Rust](https://img.shields.io/badge/Rust-Slint-15243D?style=flat&labelColor=15243D)

> **Development status:** Active development. A polished release, screenshots, and installation packages will be published when ready. Until then, follow the source-build instructions below; feature availability and integrations may change.

Agent Usage Monitor helps developers keep an eye on their AI coding-assistant usage without repeatedly switching away from their workflow. It combines a provider-neutral monitoring core with a small native Windows interface and local, privacy-conscious settings.

## What it does

- **Codex:** reads rate-limit information using the experimental `codex app-server` interface, with initial refresh and periodic updates in live mode.
- **Claude Code (opt-in):** receives only supported usage windows from Claude Code's documented status-line input through a local bridge. It never silently edits an existing status-line configuration.
- **At a glance:** remaining percentages and independent reset countdowns, with unavailable or stale data clearly distinguished from valid values.
- **Windows integration:** dynamic two-bar tray icon, hover preview and pinned flyout, theme-aware tray menu, Settings window, and Windows toast notifications under the AgentUsageMonitor name.
- **Personalization:** Graphite, Bluish Dark, and Light themes; Ukrainian, Polish, English, and Russian localization.
- **Local-first:** saves settings and alert state under the user's application data directory. It does not store authentication credentials or tokens.

### Current limitations

This is a **pre-release (0.1.0 release candidate), not yet published**. High-DPI layouts were checked at 100 % and with scale emulation only, and integration availability depends on upstream provider interfaces. Claude Code usage is session-driven and may be missing or stale. No promises of stable installation packages or released binaries are implied.

## License & reuse

This project is released under the **[MIT License](LICENSE)**.

You may **use, copy, modify, merge, publish, distribute, sublicense, and sell** copies of the software, including in commercial projects, subject to the license terms. **You must retain the original copyright notice and MIT permission notice** in copies or substantial portions of the software.

Copyright © 2026 **Valentyn Fetisov**. The software is provided **“AS IS”**, without warranty. MIT permits reuse; it does not require derivatives to be open-source. Third-party dependencies, trademarks, and provider assets may have separate conditions.

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

## Windows release candidate

`packaging/windows/build-installer.ps1` builds the x64 release binaries, a portable ZIP and, when [Inno Setup 6](https://jrsoftware.org/isinfo.php) is installed (`winget install --id JRSoftware.InnoSetup -e`), a per-user installer (`packaging/windows/AgentUsageMonitor.iss`). Artifacts are written to `rust/target/installer/`. The installer needs no administrator rights, adds a Start menu shortcut (desktop shortcut optional), keeps settings on upgrade and uninstall unless you choose otherwise, and includes optional Windows sign-in launch (off by default). The app registers only its own value under the current user's Run key; uninstall removes that value. The Claude Code bridge stays opt-in. Codex and Claude marks are embedded in the EXE only if a maintainer places them in the git-ignored `rust/agent-monitor-ui/assets/provider-icons/` folder before building; they are never committed, and a build containing them must not be distributed publicly without the brand owners' permission (see `docs/design/brand-assets.md`).

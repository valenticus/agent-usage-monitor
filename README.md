<div align="center">

<img src="assets/icon.png" width="112" alt="Agent Usage Monitor app icon">

# Agent Usage Monitor

**Codex and Claude Code usage limits, always in sight.**

A small Windows tray app that shows remaining quota, reset countdowns and low-limit alerts for your AI coding assistants.

[![Latest release](https://img.shields.io/badge/Release-v0.1.1-15243D?style=flat-square)](https://github.com/valenticus/agent-usage-monitor/releases/tag/v0.1.1)
[![License: MIT](https://img.shields.io/badge/License-MIT-15243D?style=flat-square)](LICENSE)
![Platform: Windows](https://img.shields.io/badge/Platform-Windows%2010%2B%20x64-15243D?style=flat-square)
![Rust](https://img.shields.io/badge/Rust-Slint-15243D?style=flat-square)
![Languages](https://img.shields.io/badge/UI-UA%20·%20EN%20·%20PL%20·%20RU-15243D?style=flat-square)

**[Download v0.1.1](https://github.com/valenticus/agent-usage-monitor/releases/tag/v0.1.1)** · [All releases](https://github.com/valenticus/agent-usage-monitor/releases) · [Report an issue](https://github.com/valenticus/agent-usage-monitor/issues)

<img src="assets/hero.png" alt="Agent Usage Monitor: Settings in Light and Bluish Dark themes with the tray window">

</div>

## Overview

Agent Usage Monitor (AUM) sits in the Windows notification area and keeps the 5-hour and weekly usage windows of **Codex** and **Claude Code** one glance away. It reads only what the tools expose locally, shows remaining percentages with reset countdowns, and warns you with Windows notifications before a limit runs out.

AUM is built with **Rust** and **Slint**. It is an independent community project.

## Features

- **Codex monitoring:** 5-hour and weekly quota through the local `codex app-server`, with manual refresh and automatic background refresh.
- **Claude Code monitoring:** session data through an optional local status-line bridge, plus an optional Claude CLI background refresh that you enable explicitly.
- **Clear overview:** remaining percentages, colour-coded progress bars, reset countdowns and honest "unavailable" states for each provider.
- **Tray window:** rest the pointer on the tray icon for a compact preview; double-click the icon or choose **Open** in the tray menu for the full window with refresh and settings. The tray menu also offers **Refresh**, **Settings** and **Exit**.
- **Windows notifications:** configurable warning and critical thresholds, exhausted-limit alerts, weekly-limit alerts and a test notification.
- **Themes:** Graphite, Bluish Dark and Light.
- **Languages:** Ukrainian, English, Polish and Russian.
- **Launch at sign-in:** optional start in the tray when you sign in to Windows.
- **Local-first:** settings and alert state stay in your user profile. AUM does not store provider credentials and does not change your agent settings.

## Screenshots

> Screenshots were captured with v0.1.0. v0.1.1 looks the same apart from more consistent provider icon sizing.

### Tray window

The compact preview appears on hover; the full window opens on double-click or from the tray menu.

<img src="assets/tray.png" alt="Compact hover preview and full tray window in dark and light themes">

<table>
<tr>
<td width="50%"><img src="assets/main-dark.png" alt="Full tray window, Bluish Dark theme"></td>
<td width="50%"><img src="assets/main-light.png" alt="Full tray window, Light theme"></td>
</tr>
<tr>
<td align="center">Bluish Dark</td>
<td align="center">Light</td>
</tr>
</table>

### Themes

<img src="assets/themes.png" alt="Settings in Bluish Dark, Graphite and Light themes">

### Settings

<table>
<tr>
<td width="50%"><img src="assets/settings-dark.png" alt="General settings, Bluish Dark theme"></td>
<td width="50%"><img src="assets/settings-light.png" alt="General settings, Light theme"></td>
</tr>
<tr>
<td align="center">General · Bluish Dark</td>
<td align="center">General · Light</td>
</tr>
<tr>
<td width="50%"><img src="assets/notifications.png" alt="Notification settings with a Windows test notification"></td>
<td width="50%"><img src="assets/about.png" alt="About page with connection status"></td>
</tr>
<tr>
<td align="center">Notifications</td>
<td align="center">About</td>
</tr>
</table>

### Languages

<img src="assets/languages.png" alt="Settings and tray window in Ukrainian, English, Polish and Russian">

## Download and install

Downloads are on **[GitHub Releases](https://github.com/valenticus/agent-usage-monitor/releases)**. Each release lists SHA-256 checksums for its files. Requirements: Windows 10 version 1809 or later, x64.

### Windows installer (recommended)

Download `AgentUsageMonitor-0.1.1-win-x64-setup.exe`, run the setup wizard, and launch **Agent Usage Monitor**. Installation is per-user; administrator access is not normally required. Installing over an earlier version upgrades it in place and keeps your settings.

The installer can create Start Menu and optional desktop shortcuts. AUM can optionally start when you sign in to Windows. Uninstalling the application can preserve your settings for a future reinstall.

### Portable ZIP

Download `AgentUsageMonitor-0.1.1-win-x64-portable.zip`, extract the archive, and run `agent-usage-monitor.exe`.

The archive contains the main executable, the optional `agent-usage-monitor-claude-bridge.exe`, LICENSE, and documentation. The bridge executable does not need to be launched manually just to open AUM.

**Portable does not mean self-contained settings:** both distributions use `%APPDATA%\Agent Usage Monitor` for per-user preferences and alert state. Windows notification behavior may differ without an installed Start Menu shortcut.

### Integrations

**Codex:** install and sign in to the Codex CLI. AUM reads rate-limit information through the local Codex app-server.

**Claude Code:** install and configure Claude Code separately. Session-driven usage information can be supplied through the optional status-line bridge. Optional Claude CLI background usage refresh is configurable in AUM. AUM never changes your Claude settings automatically.

For manual bridge setup, see [Claude status-line documentation](docs/claude-status-line.md). Feature availability depends on the provider's local interfaces and your configuration.

## Notes and limitations

- Usage information depends on what Codex and Claude Code expose locally. Providers may be unavailable or return incomplete or stale data.
- The Claude status-line bridge is optional; enabling Claude monitoring in the UI is not the same as installing or configuring the bridge.
- Notifications follow Windows notification settings and focus policies. The installed version integrates with Windows more completely than a portable launch.
- This is an independent community project, not an official product of OpenAI or Anthropic.
- Claude and Codex are trademarks of their respective owners. The provider marks shown in the app identify each provider only; they are not covered by the MIT license (see [brand asset notes](docs/design/brand-assets.md)).

## Build from source

Requirements: Windows, a stable Rust toolchain with `x86_64-pc-windows-msvc`, Visual Studio C++ Build Tools, and the Windows SDK.

```powershell
git clone https://github.com/valenticus/agent-usage-monitor.git
cd agent-usage-monitor/rust
rustup target add x86_64-pc-windows-msvc
cargo build --workspace --release
```

The application executable is `target\release\agent-usage-monitor.exe`. For development, run the demo mode with:

```powershell
cargo run -p agent-usage-monitor-ui --bin agent-usage-monitor -- --demo
```

## Project structure

- `rust/agent-monitor-core/` — provider models, Codex access, Claude integration, and alerts.
- `rust/agent-monitor-ui/` — Slint interface, settings, themes, localization, notifications, and Windows tray integration.
- `docs/` — provider integration notes, design specification, and project research.

## License

Agent Usage Monitor is licensed under the [MIT License](LICENSE). Copyright © 2026 Valentyn Fetisov. You may use, modify, distribute, and sell copies under the license terms; copies or substantial portions must retain the copyright and permission notices. The software is provided “AS IS,” without warranty. Third-party dependencies, names, and marks may have separate terms.

## Links

- [Download releases](https://github.com/valenticus/agent-usage-monitor/releases)
- [Report an issue](https://github.com/valenticus/agent-usage-monitor/issues)
- [Source repository](https://github.com/valenticus/agent-usage-monitor)
- [Claude Code status-line setup](docs/claude-status-line.md)
- [Provider icons](docs/provider-icons.md)
- [MIT License](LICENSE)

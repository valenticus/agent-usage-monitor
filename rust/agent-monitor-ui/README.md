# Agent Usage Monitor (AUM)

Run the standalone Slint preview from the `rust` directory:

```powershell
cargo run -p agent-usage-monitor-ui --bin agent-usage-monitor -- --demo
```

Demo percentages are visibly labeled as samples. Live mode is available with:

```powershell
cargo run -p agent-usage-monitor-ui --bin agent-usage-monitor -- --live
```

Live mode uses the Rust Core `codex app-server` provider, performs an initial request, then refreshes every five minutes. Claude remains unavailable unless the opt-in bridge listener is enabled:

```powershell
cargo run -p agent-usage-monitor-ui --bin agent-usage-monitor -- --live --enable-claude-bridge
```

The UI is provider-neutral at its adapter boundary, displays remaining percentages, preserves absent values as unavailable, and shows independent reset countdowns. Claude data comes only from an explicitly configured official status-line command, and becomes stale after 15 minutes. The current user's existing status-line command was left untouched; manual opt-in instructions are in [Claude status-line setup](../../docs/claude-status-line.md). Broader feasibility findings are in [Claude integration research](../../docs/CLAUDE_INTEGRATION_RESEARCH.md).

## Current milestone implementation

- Compact, provider-neutral Slint quota popup with independent provider rows, remaining percentages, reset countdowns, and a visible Demo sample label.
- Independent native-framed Settings window with General, Notifications, and About tabs; scrollable settings, theme/language selectors, provider controls, integration status, and project metadata.
- Typed settings schema and atomic local JSON writes under `%APPDATA%\Agent Usage Monitor\settings.json`; alert-engine state remains a separate Core store. First startup copies only validated `settings.json` and `alert-state.json` from `%APPDATA%\Agent Monitor\` when each target file is absent. The old directory and unrelated files are retained.
- Codex polling pauses when disabled and resumes immediately when enabled. Claude status-line messages are ignored while Claude is disabled; the bridge remains opt-in.
- Dark, light, and System app palette selection. System follows Windows `AppsUseLightTheme`, checked periodically.
- Ukrainian, English, and Polish popup, settings, status, and tray-menu strings; unsupported system locales fall back to English.
- Native Windows notification-area icon with left-click popup toggle and right-click Show quota, Settings, and Quit actions. Alert events are delivered through Windows tray balloon notifications.
- A generated neutral AUM EXE icon embedded as a Windows group icon resource. Provider marks remain neutral monograms until asset redistribution permission is established.
- A deterministic RGBA two-column tray icon renderer with provider-to-window mapping for all four enable combinations and 16/20/24/32 pixel sizes.

## Explicit limitations

- The tray icon currently uses the neutral AUM icon; live quota-driven icon refresh is not connected.
- Provider marks are not redistributed; neutral monograms are used pending official asset review. See the research document.
- Native Windows balloon presentation remains subject to Windows notification settings and focus policies.
- Scale-factor behavior has unit coverage; a full screenshot comparison at multiple physical DPI settings remains outstanding.

Settings are local preferences only; no authentication token or provider credential is stored. No Claude settings are modified by Agent Usage Monitor.

The default executable is `agent-usage-monitor.exe`. The bridge executable is `agent-usage-monitor-claude-bridge.exe`.

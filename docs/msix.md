# Microsoft Store (MSIX) build

AgentUsageMonitor ships in three forms from **one binary**: the Inno Setup installer, the portable ZIP and the Microsoft Store MSIX package. Store-specific behaviour is chosen at run time from the process's package identity (`packaged::state()`, via `GetCurrentPackageFullName`). Nothing is compiled differently, so the EXE builds keep their behaviour.

## Store identity

Confirmed in Partner Center → Product identity. Do not change these values.

| Field | Value |
| --- | --- |
| Reserved name / DisplayName | `AgentUsageMonitor` |
| Identity Name | `ValentynFetisov.AgentUsageMonitor` |
| Publisher | `CN=090E947F-DD2D-4592-BD86-2187AAA0F9BA` |
| PublisherDisplayName | `Valentyn Fetisov` |
| Package Family Name | `ValentynFetisov.AgentUsageMonitor_sfbqfhxtgjc58` |
| Store ID | `9N20DHG8SSS1` |
| Version | `x.y.z.0`, taken from `rust/agent-monitor-ui/Cargo.toml` (0.1.1 → `0.1.1.0`) |
| Architecture / minimum OS | x64, Windows 10 1809 (`10.0.17763.0`) |
| Capabilities | `runFullTrust` only |
| Framework dependency | `Microsoft.VCLibs.140.00.UWPDesktop` ≥ `14.0.33728.0` |

Windows derives the PFN suffix from the Publisher (Crockford base32 of the first 8 bytes of SHA-256 over the UTF-16LE Publisher string). `build-msix.ps1` derives it on every build and **fails** if it differs from the confirmed `ValentynFetisov.AgentUsageMonitor_sfbqfhxtgjc58`, so an accidental edit of the Identity Name or Publisher is caught locally and in CI.

## Package identity detection

`packaged::state()` has three outcomes:

| State | When | Behaviour |
| --- | --- | --- |
| `Packaged` | `GetCurrentPackageFullName` reports a package (`ERROR_INSUFFICIENT_BUFFER`) | Store behaviour (this document) |
| `Unpackaged` | Windows confirms no package (`APPMODEL_ERROR_NO_PACKAGE`) | EXE behaviour, unchanged |
| `Unknown` | any other result (not expected in practice) | fail-safe: no Store UI, **no** HKCU `Run` or AUMID registry writes, **no persistent storage** (see "Settings isolation") |

Only a confirmed unpackaged process may write the EXE builds' registry state or their settings folder.

## Differences in the packaged build

| Area | EXE (Inno, portable) | MSIX (Store) |
| --- | --- | --- |
| Data folder (settings, alert state, custom provider icons) | `%APPDATA%\Agent Usage Monitor` | the package's own `LocalState` (`ApplicationData.Current.LocalFolder`, `%LOCALAPPDATA%\Packages\ValentynFetisov.AgentUsageMonitor_sfbqfhxtgjc58\LocalState`); see "Settings isolation" |
| Sign-in launch | HKCU `Run` value `AgentUsageMonitor`, rewritten at each launch so it follows the current EXE path | `windows.startupTask` `AgentUsageMonitorStartup` (declared off), see "Startup task" |
| Notifications | own AUMID `Valenticus.AgentUsageMonitor`, registered under HKCU with an icon PNG | package AUMID `<PFN>!AgentUsageMonitor`; no registry writes, no explicit AUMID, default notifier; name and logo come from the manifest |
| Claude status-line listener | `--enable-claude-bridge` launch argument | Settings → General → Claude → **Status-line bridge** toggle (`claude_bridge_listener_enabled`, off by default, Store build only), applied at run time. Packaged apps cannot pass launch arguments on Windows 10 1809. |
| Bridge command for Claude Code | full path to `agent-usage-monitor-claude-bridge.exe` | App Execution Alias `agent-usage-monitor-claude-bridge.exe` (files in `WindowsApps` cannot be started by path from outside the package) |
| Legacy `Agent Monitor` folder migration | automatic, unchanged | never; explicit import from the EXE build instead |
| Updates | Inno upgrade over the same AppId | Store updates; applied when AUM is not running. The startup task state and `LocalState` survive updates. |

Unchanged in both builds: tray icon and menu, sleep/resume and Explorer-restart recovery, flyout Z-order, background running when windows are closed, single-instance mutex `Local\AgentUsageMonitor.Singleton`, Codex `app-server` and Claude CLI child processes (timeouts, cancellation, process-tree cleanup), and the pipe security (current user only, remote clients rejected, one instance).

### Settings isolation

The Store build never uses `%APPDATA%\Agent Usage Monitor`. Its data folder is the package's `LocalState`, resolved through `ApplicationData.Current.LocalFolder`. That folder belongs to the package, is not subject to AppData virtualization (AUM does not depend on virtualization at all), and Windows removes it on uninstall.

Guards:

- `settings::save` refuses any path inside `%APPDATA%\Agent Usage Monitor` unless the process is confirmed unpackaged.
- **No persistent storage** when `LocalState` cannot be resolved (error, empty or relative path, or a path inside the EXE folder) or the identity is `Unknown`. There is no fallback to `%TEMP%` or to the EXE folder. AUM then:
  - starts with the default settings in memory (nothing was readable, so nothing is reset or deleted);
  - refuses every save (`SettingsError::StorageUnavailable`), so no change is reported as saved; a toggle that cannot be saved does not change;
  - keeps alert state in memory only, uses only the embedded provider icons and hides the import;
  - shows a localized warning at the top of Settings → General ("settings storage is unavailable; changes are not saved").
  The Windows startup task still reflects its real state (it is owned by Windows, not by the settings file).
- At launch the Store build does not save settings (the startup task is only read), so no Store `settings.json` exists until the user changes a setting or imports.
- v0.1.1 ignores unknown settings fields, so `claude_bridge_listener_enabled` never breaks an EXE build that reads a file containing it.

### Import from the EXE build (optional, one time)

Store build only, General tab: **Import settings from the EXE version** (UA/EN/PL/RU). The row is shown only when, at launch, the EXE build's `settings.json` existed and was valid, and the Store build has no `settings.json` yet. It disappears as soon as the Store build has its own settings.

- **Explicit:** nothing is imported until the user clicks the button.
- **Read-only toward the source:** the EXE folder is only opened for reading (`%APPDATA%` reads fall through to the real files for packaged apps).
- **Never overwrites Store settings:** the import stops if `LocalState\settings.json` exists, and the final write uses a no-clobber rename.
- **Validated:** the file must be ≤ 64 KiB, parse as JSON, have schema version 1 and valid thresholds and duration. Missing, unreadable, truncated (partially written) or invalid files import nothing.
- **Imported:** theme, language, Codex/Claude provider switches, alerts on/off, warning and critical thresholds, exhausted-limit alerts, weekly limits, popup duration.
- **Never imported (stay off until enabled in the Store build):** launch at sign-in, the status-line bridge opt-in, Claude CLI background refresh. These are permissions granted to the other app.
- **Custom provider icons:** files from `provider-icons\` with one of the names AUM looks for, ≤ 2 MiB, regular files only, copied only when absent in the Store folder. They are used from the next start.
- **Not copied:** alert history (`alert-state.json`), the notification icon, CLI credentials, tokens and any other files.
- Settings are written last, so an interrupted import leaves no Store settings and can be started again.

### WinRT initialization

AUM calls WinRT only in the Store build (`ApplicationData`, `StartupTask`) and only on threads it creates for that purpose; the Slint UI thread's apartment is never initialized or changed by this code.

- `winrt::Apartment` is an RAII guard around `RoInitialize(RO_INIT_MULTITHREADED)`. `S_OK` and `S_FALSE` are owned and balanced by exactly one `RoUninitialize` on drop (on the same thread; the guard is not `Send`). `RPC_E_CHANGED_MODE` (the thread is already an STA owned by other code) is detected, never uninitialized, and rejected where blocking waits follow (`require_multithreaded`). Other HRESULTs are returned as errors.
- Package data folder: `ApplicationData.Current.LocalFolder.Path` is read once at startup on a short-lived `aum-winrt` thread inside its own MTA guard; failure means "no persistent storage" (see above).
- Startup task: each `query`/`set` on the controller's worker thread first takes the guard (declared first, so every WinRT object is released before `RoUninitialize`). An initialization failure is reported as a normal request error.
- Unit tests cover the HRESULT mapping, nested guards on a fresh thread, a foreign STA that is left alone, and error propagation from the worker. They do not prove packaged runtime behaviour.

### Startup task

- All `Windows.ApplicationModel.StartupTask` calls run on a short-lived worker thread (`autostart::StartupController`). The Slint UI thread only submits a request and later receives the result through `slint::invoke_from_event_loop`; it never waits on a WinRT operation.
- Microsoft documents a UI-thread requirement for `RequestEnableAsync` only for UWP apps (it shows a consent dialog there). For a packaged desktop app "no user-consent dialog is shown", and `StartupTask` is agile (`MarshalingType.Agile`, `ThreadingModel.Both`). Each call explicitly initializes the worker thread into the multithreaded apartment (see "WinRT initialization"), where blocking on the async result is allowed.
- One request at a time: while a request runs, further clicks are ignored and the toggle shows "Checking the Windows startup state…" (disabled). A failed or panicking request is reported and never leaves the controller stuck.
- At launch the task is only **read** (`Refresh`). It is read again whenever Settings opens, so changes made in Windows Settings or Task Manager appear. A click without a verified current state only re-reads the state.
- States: `Disabled` / `Enabled` → the AUM toggle decides; `DisabledByUser` → locked, "turn it on in Settings → Apps → Startup"; `DisabledByPolicy` / `EnabledByPolicy` → locked, "managed by your organization's policy". AUM never overrides a locked state. `RequestEnableAsync` returns the resulting state, which is shown as is.
- A user change is saved to the Store settings; launch-time reads stay in memory. Imported settings never enable startup.

### Claude status-line listener

One rule for all builds (`claude_bridge_wanted`): the listener runs only while the **Claude provider is on and the user opted in** (`--enable-claude-bridge` for the EXE builds, the Store toggle for MSIX).

- Turning Claude off or the opt-in off cancels the listener and drops its future immediately, closing the pipe handle, even in the middle of a message. Nothing more is accepted or parsed.
- Turning Claude back on restarts the listener only if the opt-in is still on. The opt-in is never set by anything except the user's toggle (or the launch argument); the import keeps it off.
- The Store toggle is always operable, so the opt-in can be revoked at any time, also while Claude is off.
- At most one listener exists at a time; a new pipe server is created only after the previous one was dropped.
- A listener error (for example the pipe name is taken by another AUM copy) is shown once as a Claude error. It is retried only after the wanted state goes off and on again; there is no busy restart loop.
- On exit the listener stops with the app and is never restarted.
- EXE builds: with `--enable-claude-bridge` the listener now also stops while the Claude provider is off. Before, it kept accepting and discarding messages.

### Child processes

Processes started by AUM (Codex `app-server`, Claude CLI) inherit the package context. Their writes under `%APPDATA%`/`%LOCALAPPDATA%` and HKCU are virtualized like AUM's. Their data in `~/.codex` and `~/.claude` (including credentials, which AUM never reads) is outside AppData and not virtualized, so sign-ins are shared with the user's normal CLI. CLI discovery reads PATH and real AppData locations, which works because reads fall through.

Process launch:

- `cmd.exe`, `powershell.exe` and `taskkill.exe` are started by absolute path from the system directory (`GetSystemDirectoryW`; the native `System32` in this 64-bit process). A bare name would be searched in the application directory first. `system_tool_path` returns an error when the directory cannot be read (call failure, non-absolute result, unstable length; long paths are handled by growing the buffer). It never falls back to a bare name: Codex then reports a spawn error for `.cmd`/`.ps1` shims, and process cleanup kills only the direct child instead of running `taskkill /T`.
- Claude CLI is resolved once from absolute `PATH` entries only (`resolve_cli_in`: `claude.exe`, then `claude.cmd`, per directory; `.`, empty and relative entries are skipped, so the current directory is never searched). The exact resolved path is used:
  - `claude.exe` (native installer) is started directly with `-p --no-session-persistence /usage`;
  - `claude.cmd` (npm shim) is started as `<System32>\cmd.exe /D /V:OFF /S /C ""<full path>" -p --no-session-persistence /usage"`, passed verbatim (`raw_arg`). The quoted path keeps `& | < > ^ ( )` literal; `/V:OFF` disables `!` expansion; paths containing `%`, `"` or control characters are refused, because cmd would expand or break them;
  - nothing found, another file type (`.ps1`, `.bat`), a relative path or no verified `cmd.exe`: the refresh fails and **no process is started**.
  - Timeout, cancellation and `taskkill /T` cleanup are unchanged.
- Codex: unchanged order: native `codex.exe`, the npm package's bundled native binary, then `codex.cmd` through `cmd.exe` or `codex.ps1` through Windows PowerShell.
- `cmd.exe` remains essential for npm `.cmd` shims (Windows can only run batch files through it); `taskkill /T` remains the process-tree cleanup on timeout and cancellation.

### Supported Windows editions

- **Windows 10 1809+ and Windows 11 (x64), normal mode:** supported. Requires the user's own Codex and/or Claude Code CLI for provider data.
- **Windows 10/11 in S mode:** AUM's data sources cannot work. S mode only runs Store apps, so the separately installed Codex and Claude CLIs (npm or native installers) cannot run. Microsoft also states that packaged apps on Windows 10 S "won't be able to start" `cmd.exe` or PowerShell. AUM itself should start and show the providers as unavailable (spawn errors are handled as "not available" or errors), but this is **not verified**.
- Microsoft's packaging guidance says "All apps submitted to the Microsoft Store must be compatible with Windows 10 S". No Partner Center setting to declare an app unsupported in S mode was found in the reviewed documentation. Before submission: test in S mode (or with the S mode code-integrity policy in audit mode) that AUM starts and degrades cleanly, disclose the CLI dependency at the start of the Store description (Store policy 10.2.4, "non-integrated software"), and use the certification notes for the dependency. **Store acceptance on this point is an open risk.**

## Coexistence with the EXE build

- Both builds use the mutex `Local\AgentUsageMonitor.Singleton` and the same pipe name, so at most one AUM runs per session. This is unchanged on purpose: two copies would fight over the pipe and show duplicate alerts.
- If both are installed and both launch at sign-in (Inno HKCU `Run` value and the Store startup task), whichever process creates the mutex first runs and the other exits silently. Which one wins is not predictable.
- Settings no longer interfere: each build has its own data folder.
- Recommendation for users: install only one build; when moving to the Store build, use the import, turn off "Launch at Windows sign-in" in the EXE build (or uninstall it).
- Not changed: making one build defer to the other (for example the Store build exiting when the EXE build's `Run` value exists) would change installed EXE behaviour and needs a product decision.
- Runtime coexistence was **not tested**; it needs a VM or second PC.

## Build

Requirements: the normal Rust build tools plus the Windows SDK (`makeappx.exe`, `makepri.exe`; 10.0.26100 is installed on the reference machine).

```powershell
# Full build: release binaries + MSIX into rust\target\msix-<version>\ (or -OutputDirectory)
pwsh -File packaging\msix\build-msix.ps1 -Jobs 2

# Reuse existing release binaries
pwsh -File packaging\msix\build-msix.ps1 -SkipBuild

# Static check without compiling Rust (placeholder EXEs, temporary output); also runs in CI
pwsh -File packaging\msix\build-msix.ps1 -ValidateManifestOnly
```

The script stages the two EXEs, `LICENSE`, the manifest (`packaging/msix/AppxManifest.xml` with `__VERSION__` replaced) and the visual assets resized from `assets/icon.png`. It then runs `makepri` and `makeappx pack /h SHA256`. It never runs the EXEs. Guards:

- It refuses a non-empty output folder.
- It fails if debug files, provider artwork or settings-like files are staged.
- It fails if an EXE imports `VCRUNTIME140`/`MSVCP140` while the manifest lacks the VCLibs framework dependency. The Rust MSVC target links the CRT dynamically, and `VCRUNTIME140.dll` is not part of Windows. The UCRT (`api-ms-win-crt-*`) is.
- It fails if the derived package family name differs from the Partner Center identity, or if the Identity Name, Publisher, `Properties/DisplayName` (`AgentUsageMonitor`), `PublisherDisplayName` (`Valentyn Fetisov`) or the tile `VisualElements` DisplayName differ (exact, case-sensitive). The Store ID `9N20DHG8SSS1` is not a manifest field.

The `.msix` is **unsigned**: Partner Center signs Store submissions. Do not submit a self-signed package.

CI (`.github/workflows/rust.yml`) runs `-ValidateManifestOnly`. That is a static check of the manifest, identity, assets and layout with zero-byte placeholder EXEs. It does not build, sign, install or run the package and is **not** evidence that the packaged app works.

## Testing limits and risks

Not tested anywhere yet (needs a clean Windows 10 1809+ and Windows 11 x64 VM or second PC; never on the working machine with the installed EXE build):

- installation, WACK, uninstall, Store update;
- `ApplicationData.LocalFolder` resolution on the MTA helper thread, settings persistence in `LocalState`, the "storage unavailable" warning path, and the import from a real EXE installation (including the `%APPDATA%` read fall-through on 1809 and 1903+);
- the StartupTask worker thread with explicit `RoInitialize` and real WinRT calls, all five states, the "Checking…" state, and changes made in Windows Settings / Task Manager while AUM runs;
- Claude CLI through a real npm `claude.cmd` and a native `claude.exe` inside the package context;
- tray, toasts with the package AUMID, sleep/resume in the package;
- Codex and Claude CLIs inside the package context (direct `claude.exe`, npm shims through `cmd.exe`, `taskkill` cleanup);
- the bridge alias from Git Bash, PowerShell and cmd; the Store toggle and Claude toggle on/off while Claude Code sends status lines;
- coexistence with an Inno installation (mutex race at sign-in);
- S mode behaviour.

Notes for the test machine:

- **Sideloading** a test build needs the package signed with a test certificate trusted on the test machine (never submit that one), and the `Microsoft.VCLibs.140.00.UWPDesktop` x64 framework installed first. The Store installs the framework automatically.
- The Windows App Certification Kit installs the package to test it, so it must also run on the test machine.
- Bridge alias: Claude Code runs the `statusLine` command through its shell (Git Bash on Windows). Verify that stdin/stdout pass through, and that turning the alias off in Settings → Apps → Advanced app settings → App execution aliases makes the bridge fail cleanly.

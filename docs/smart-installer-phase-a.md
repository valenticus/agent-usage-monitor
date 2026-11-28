# Smart Installer Phase A

The Windows installer remains Inno Setup. The existing AppId, per-user install identity, data-preserving upgrade behavior and default KEEP-on-uninstall behavior are unchanged.

## Claude bridge opt-in

- New EXE installations start with the bridge OFF. The persisted EXE preference is separate from the MSIX setting.
- The explicit Settings toggle changes only the EXE saved preference. It does not read credentials, modify provider configuration or change global Claude settings.
- Runtime behavior is `claude_enabled && (cli_flag || saved_opt_in)`. `--enable-claude-bridge` applies only to the current process, is never persisted automatically, and overrides saved OFF while that process runs. Settings shows the temporary override and disables the saved-choice control until AUM is started without the flag.
- MSIX continues to use its own `claude_bridge_listener_enabled` setting.

## v0.2 migration

Migration runs only for a verified Inno upgrade whose uninstall `DisplayVersion` is strictly older than `0.3.0`, or for an in-place Portable upgrade with an existing AUM executable and the exact legacy `--enable-claude-bridge` HKCU `Run` command. If the executable has a Windows version resource, it must identify a version older than `0.3.0`; older Portable binaries without a version resource rely on the exact legacy command as their explicit opt-in evidence.

Positive evidence is the selected `claudebridge` task, a Start Menu shortcut verified by exact executable, working directory, AppUserModelID and exact legacy arguments, or an exact quoted HKCU `Run` command and executable path. These sources are considered independently: an empty task selection does not block shortcut or Run evidence. Empty or negative evidence does not create a saved OFF value.

The per-user marker stores the exact installed executable path and `Pending` or `Done` state. `Pending` is written only with positive bridge evidence; it is not overwritten on later setup runs. With no positive evidence, setup writes `Done` only after successful installation, without enabling the bridge. A failed or canceled installation does not create terminal state. `Done` is terminal. A process may consume `Pending` only when its executable path matches exactly, it is confirmed unpackaged, and the adjacent Inno uninstaller exists. Portable copies and MSIX cannot consume the installer marker. Existing explicit EXE ON/OFF settings take precedence over migration evidence.

When positive Run evidence is captured, the Pending marker is persisted before the legacy argument is removed from the verified AUM Run entry. For an in-place Portable v0.2.0 upgrade, AUM first checks that the current executable exactly matches the quoted HKCU Run target and that the arguments are exactly `--enable-claude-bridge`; it also requires no saved EXE choice. AUM persists ON before stripping the legacy flag. If settings persistence fails, it leaves both the legacy flag and OFF-by-default setting untouched, and skips autostart reconciliation for that launch. A verified AUM Start Menu shortcut is refreshed only after installer evidence capture; pinned-taskbar shortcuts and unverified links are not edited. If installer marker persistence fails, the old Run argument is retained.

## Version and file replacement behavior

Installer version comparison accepts exactly three decimal components (`major.minor.patch`), matching the Cargo package version format used by this installer. Suffixes such as `-rc1`, missing components, non-numeric components and overflow are rejected safely; downgrade is blocked when the installed version is greater.

The bridge helper is listed before the main GUI executable. If it is locked, Inno uses its normal file error and retry flow; close AUM and any process using the helper, then retry setup. The per-user installer does not request replacement at reboot and does not promise atomic rollback. A failure during replacement can leave a partially updated installation; inspect the installer result and retry after releasing the file lock.

Installer setup waits for the AUM mutex during its wizard preflight. Uninstall checks run before Inno creates the uninstall progress form, so the wait does not update either form during that preflight; it waits without touching the Setup wizard object. Win32 mutex creation errors are captured immediately and included in the localized setup diagnostic.

## Validation boundary

Automated Rust tests and ISCC compilation do not exercise real registry state, shortcut property inspection, mutex contention, restart replacement or first-install/upgrade callbacks. Those require an isolated Windows runtime test. Do not treat compile-only checks as native installer QA.

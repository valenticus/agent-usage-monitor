# State: fix/tray-hover-background-preview

## Goal
Fix tray hover preview for an open but inactive main statistics window. Keep the existing single `MainWindow`, avoid activation on hover, preserve explicit double-click activation, and restore the background window's original position and Z-order.

## Git starting point
- Branch `fix/tray-hover-background-preview`, clean at `daab35501312c3bc49c9132c114b2964e9de6839` before this follow-up.
- PR #8 remains open and unmerged.

## Progress
- The user reproduced this regression on Windows: after background hover preview expiry, the full window returns at the preview location and covers the foreground app.
- Root cause confirmed in the restore path: `hide_now()` called `present(false, false)`, which resized/repositioned the still-TOPMOST HWND before `sync_native_style()` cleared TOPMOST with `SetWindowPos(HWND_NOTOPMOST, SWP_NOACTIVATE...)`. Microsoft documents `HWND_NOTOPMOST` as placing a window above all non-topmost windows; this preserves keyboard focus but covers a normal foreground application. Winit 0.30.13's `set_outer_position` and size calls use `SWP_NOZORDER | SWP_NOACTIVATE`, so they do not cause the raise.
- Background restoration now lowers the still-compact HWND first, behind the current normal foreground HWND (or behind all topmost windows / at the bottom when appropriate). The call uses `SetWindowPos` with `SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER`, checks that `WS_EX_TOPMOST` cleared, and safely falls back to `HWND_BOTTOM` if the foreground target became stale. Only after that does it switch to full content and apply the saved position and size. It reuses the existing `MainWindow`; no activation call runs on restoration.
- `restore_position()` now preserves the exact saved physical coordinates as long as the saved center remains on a connected monitor. If that monitor is gone, it falls back to the tray anchor. This avoids the previous unconditional work-area inset clamp.
- Added focused tests for Z-order target selection, full-window non-TOPMOST/no-activate decisions, and saved-position preservation/fallback.
- Validation passed on the final source: `cargo fmt --manifest-path rust/Cargo.toml --all --check`; `cargo test --locked --manifest-path rust/Cargo.toml --workspace -- --test-threads=2` (196 tests); strict workspace Clippy (`--all-targets -- -D warnings`); Windows MSVC workspace check; and `git diff --check`.
- Updated external QA launcher: `C:\Users\fetis\AppData\Local\AUM-QA\PR8-daab355\Start-AUM-PR8-QA.ps1`. It sets isolated `APPDATA`, `LOCALAPPDATA`, temp paths, and `CLAUDE_CONFIG_DIR`; launches `--qa-isolated --empty`; verifies the built QA executable SHA-256 and installed Store AUM v0.2.0 binary SHA-256; and does not start, modify, stop, uninstall, or replace the Store app.
- External debug build: `C:\Users\fetis\AppData\Local\AUM-QA\PR8-daab355\target\debug\agent-usage-monitor.exe` (SHA-256 `D21E636FAF76932688A5AEF234D70FA67C7D11B5D753C281B01F71061B15EFD1`).
- Store AUM v0.2.0 remains installed at `ValentynFetisov.AgentUsageMonitor_0.2.0.0_x64__sfbqfhxtgjc58`, status `Ok`; installed EXE SHA-256 remains `7E8228AB52D320AB35AF69EC9C41ECF6BF6CFF2DEE5A944803F5F70F0FBE6150`. It was not running during this follow-up. It was not started to avoid triggering its existing provider refresh configuration.
- The user-supplied runtime reproduction is confirmed by source/API behavior. Independent replay of the tray-hover sequence was not completed in this session; visual QA is still required and has not passed.
- Fix and state update are prepared for commit/push to the same PR #8. Check CI after push. Do not merge until manual Windows QA passes.

## Changed files
- `rust/agent-monitor-ui/src/flyout.rs`
- `docs/state/fix-tray-hover-background-preview.md`

## Next
- Commit and push these changes to `fix/tray-hover-background-preview` / PR #8.
- Check the new PR CI status.
- Provide the updated isolated QA launcher for manual scenarios A–H. Do not merge until those scenarios pass.

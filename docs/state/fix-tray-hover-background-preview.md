# State: fix/tray-hover-background-preview

## Goal
Keep the approved main statistics and compact preview visuals unchanged while giving them independent window lifecycles. Hover must never touch the main HWND; explicit Open activates the existing main window. Do not merge before manual Windows QA and visual approval.

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
- Fix commit `0898c62c305d969be6abe2ca59e66d36d2746191` was pushed to the same PR #8. GitHub Actions run `45616958537` passed the `windows` job in 3m10s (formatting, static MSIX validation, Clippy, tests, and Windows application build).
- The subsequent PR head `4a0dc6c1dac88586648e6cc7434f2d078af5602b` also passed `windows` in run `45617563369` (3m12s).
- PR #8 remains open and unmerged. Do not merge until manual Windows QA passes.

## Current two-window follow-up
- The previous same-HWND `FullPinned`/`CompactTemporary` restoration workaround is superseded. Tray hover is now owned by a second `MainWindow` component instance and a separate `Flyout` controller. The main controller and preview controller have fixed roles for their lifetimes; hover, pointer tracking, compact sizing, animation, TOPMOST and dismissal are routed only to the preview. Main Open reuses and activates the existing main instance.
- One process, one current `UiSnapshot`, shared settings/locale, and the existing provider refresh workers remain in use. Preview receives the same cached snapshot/config on provider updates; a hover refreshes only the preview from cached state and does not request provider polling. QA mode adds no live polling: use `--qa-isolated --empty`.
- Main is never compacted, moved, resized, hidden, raised, or assigned TOPMOST by a hover/dismiss event. The foreground check only reads the main HWND. Preview owns a separate HWND, is non-activating, and is TOPMOST only while visible. No Z-order capture/restore is used.
- The approved Slint source `rust/agent-monitor-ui/ui/main.slint` is unchanged. Its geometry baseline is width 440 logical px; full height is `40 + count*84 + max(count-1,0)*21 + 14` (zero providers uses `40 + 48 + 14`); compact height is `12 + count*72 + max(count-1,0)*17 + 12` (zero providers uses `12 + 48 + 12`). Thus at two providers: 440x243 full and 440x185 compact; one provider: 440x138 and 440x96; zero providers: 440x102 and 440x72. The `assets/main-dark.png`, `assets/main-light.png`, and `assets/tray.png` screenshots remain additional references. Theme, language, DPI and screenshot matching are for manual Windows visual QA; automatic screenshot comparison was not performed.
- Regression tests now cover fixed per-window presentation styles, preview-only hover ownership and main-active suppression, independent main/preview lifecycle decisions, visible-preview-only TOPMOST, and saved-position behavior. These are logic tests; they do not replace HWND-level/manual desktop verification.
- Validation on this source passed: `cargo fmt --manifest-path rust/Cargo.toml --all --check`; `cargo test --locked --manifest-path rust/Cargo.toml --workspace -- --test-threads=2` (196 passed); `cargo clippy --locked --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings`; `cargo check --locked --manifest-path rust/Cargo.toml --workspace --target x86_64-pc-windows-msvc`; and `git diff --check`.
- External debug build: `C:\Users\fetis\AppData\Local\AUM-QA\PR8-two-window\target\debug\agent-usage-monitor.exe`, SHA-256 `4B8AC11A4479504F820ECFB16BEB1E99D374FC72F7CF580FEE1F16832070B6B2`.
- New isolated launcher: `C:\Users\fetis\AppData\Local\AUM-QA\PR8-two-window\Start-AUM-PR8-QA.ps1`. It verifies the QA executable hash and exactly one healthy Store package at v0.2.0.0 with expected executable hash before launch; isolates APPDATA, LOCALAPPDATA, TEMP/TMP, and CLAUDE_CONFIG_DIR; passes only `--qa-isolated --empty`; and relies on the QA-only singleton mutex. Source guards disable notification identity registration, startup reconciliation, and live workers. Launcher syntax was parsed successfully. The QA app has not been launched during this follow-up; no runtime or visual QA is claimed.
- Installed Store package remains `ValentynFetisov.AgentUsageMonitor_0.2.0.0_x64__sfbqfhxtgjc58`, status `Ok`; its `agent-usage-monitor.exe` SHA-256 was rechecked as `7E8228AB52D320AB35AF69EC9C41ECF6BF6CFF2DEE5A944803F5F70F0FBE6150`, matching the earlier baseline. No Store app files/settings were modified and the Store app was not started or stopped.
- PR #8 remains open and unmerged. CI for the eventual pushed two-window commit must be checked after push. Manual runtime and visual approval remains required.

## Changed files
- `rust/agent-monitor-ui/src/flyout.rs`
- `rust/agent-monitor-ui/src/main.rs`
- `docs/state/fix-tray-hover-background-preview.md`

## Next
- Commit and push these changes to the existing PR #8, then inspect the resulting CI status.
- Run the external launcher from PowerShell and complete the manual visual/runtime checks: hover preview while main is closed; activate main by double-click; keep main behind foreground apps during repeated preview show/dismiss cycles; verify exact main HWND/bounds/visibility/Z-order do not change; test two side-by-side foreground apps and overlapping windows; verify no focus stealing, flicker, stuck TOPMOST, position loss, or duplicate windows; then test close/rehover, Settings, tray menu, refresh, and notifications.
- Compare full and compact visuals against the approved source and repository references at matched demo data/theme/language/DPI/display settings. Do not merge until manual visual and runtime QA passes.

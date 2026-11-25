# State: fix/tray-hover-background-preview

## Goal
Fix tray hover preview for an open but inactive main statistics window. Keep the existing single MainWindow, avoid activation on hover, and preserve explicit double-click activation.

## Git starting point
- Session-start fast-forwarded the clean `feat/v0.3.0-settings` branch to its upstream (already current).
- Fetched origin and fast-forwarded local `main` from `7a01a3f` to `3b08a76`; verified `main` equals `origin/main`.
- Created this branch from updated `main`; preserved the settings branch and the other detached worktree.

## Progress
- Confirmed root cause: `Flyout::hover()` unconditionally ignored `Mode::FullPinned`. That state tracks presentation, not whether the window is currently foreground.
- Added foreground-window detection by comparing the main window HWND with `GetForegroundWindow()` on Windows. Non-Windows uses winit focus; hidden windows are treated as inactive.
- Tray hover now keeps a foreground full window unchanged; for an inactive pinned window it switches the same window to the existing compact non-activating preview. It remembers the pinned position before the temporary switch. Double-click continues through the existing `open_full()` activation path.
- Added a state-decision regression test for hidden, compact, active-full and inactive-full states.
- Validation passed: `cargo fmt --all --check`; full `cargo test --locked --workspace -- --test-threads=2` (192 tests); targeted hover test after final code change; `cargo clippy --locked --workspace --all-targets -- -D warnings`; `cargo check --locked --workspace --target x86_64-pc-windows-msvc`; and `git diff --check`.
- Computer Use `@oai/sky` initialization failed because `@oai/cua/tinyskyAlt` is unavailable. No physical tray hover/double-click runtime QA was performed.
- Implementation commit: `f9ae5e3d2ea3d2b0932af00361ab969da55122b9`, pushed to `origin/fix/tray-hover-background-preview`.
- PR #8 opened against `main`: https://github.com/valenticus/agent-usage-monitor/pull/8. Not merged.
- GitHub Actions run `45605179725`: formatting and MSIX static validation passed; Clippy passed; workspace test step was in progress at last check, Windows build pending.

## Changed files
- `rust/agent-monitor-ui/src/flyout.rs`
- `docs/state/fix-tray-hover-background-preview.md`

## Next
- Check GitHub Actions run `45605179725` for its final result.
- Physical Windows tray hover/double-click QA remains recommended because it could not be performed here.

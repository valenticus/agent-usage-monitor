# State: fix/tray-hover-background-preview

## Goal
Fix tray hover preview for an open but inactive main statistics window. Keep the existing single MainWindow, avoid activation on hover, and preserve explicit double-click activation.

## Git starting point
- Branch `fix/tray-hover-background-preview` is based on `main` at `3b08a76`; other worktrees and the settings branch remain untouched.

## Progress
- Original root cause: `Flyout::hover()` unconditionally ignored `Mode::FullPinned`, and pointer scheduling only accepted `Mode::Hidden`.
- Pointer hover scheduling now accepts Hidden and background FullPinned states, leaves an existing hover deadline unchanged on repeated pointer events, cancels when the pointer leaves or the full window becomes foreground, then dispatches the existing hover handler after the delay.
- FullPinned foreground hover remains a no-op. Background FullPinned hover remembers the pinned position and changes the existing window to the compact preview with no activation.
- When that preview expires or is dismissed, the existing window returns to FullPinned in the background at its saved position. Geometry is applied before changing TOPMOST style so the full-size window is not briefly raised. TOPMOST and no-activate styles return to the normal full-window state without activating.
- Explicit Open/double-click selects FullPinned, clears pending restoration and calls the activation path. Explicit Close clears restoration and hides the window.
- Added regression tests for hover candidate scheduling, due-time dispatch, duplicate/repeated pointer events, fast pointer departure, active/inactive full state, hidden preview dismissal, background full restoration, explicit open and close transitions.
- Validation passed after final code edits: `cargo fmt --all --check`; `cargo test --locked --workspace -- --test-threads=2` (194 tests: Core 43, bridge unit 3, process integration 1, UI 147); strict workspace Clippy; Windows MSVC target check; `git diff --check`.
- Computer Use runtime is unavailable in this environment; no physical tray hover/double-click QA was performed. Installed AUM v0.2.0 was not modified.
- Follow-up implementation commit `3b4d050f7ec43e6cf1f76f2be785114f506a47c4` is pushed to PR #8. A comment requests fresh independent review: https://github.com/valenticus/agent-usage-monitor/pull/8#issuecomment-7161594356. PR remains open and unmerged.
- GitHub Actions for the follow-up head was in progress at last check.

## Changed files in this follow-up
- `rust/agent-monitor-ui/src/flyout.rs`
- `docs/state/fix-tray-hover-background-preview.md`

## Next
- Check the newest PR CI result and independent review feedback.
- Physical Windows tray hover/double-click QA remains recommended.

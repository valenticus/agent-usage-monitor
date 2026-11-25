# State: fix/tray-hover-background-preview

## Goal
Fix tray hover preview for an open but inactive main statistics window. Keep the existing single MainWindow, avoid activation on hover, and preserve explicit double-click activation.

## Git starting point
- Branch was clean and synchronized to origin before this follow-up.
- Current feature branch is `fix/tray-hover-background-preview`; base is `main` at `3b08a76`.

## Progress
- Original root cause: `Flyout::hover()` unconditionally ignored `Mode::FullPinned`, and pointer scheduling only accepted `Mode::Hidden`.
- Pointer hover scheduling now accepts Hidden and background FullPinned states, leaves an existing hover deadline unchanged on repeated pointer events, cancels when the pointer leaves or the full window becomes foreground, then dispatches the existing hover handler after the delay.
- FullPinned foreground hover remains a no-op. Background FullPinned hover remembers the pinned position and changes the existing window to the compact preview with no activation.
- When that preview expires or is dismissed, the existing window returns to FullPinned in the background at its saved position. Geometry is applied before changing TOPMOST style so the full-size window is not briefly raised. TOPMOST and no-activate styles return to the normal full-window state without activating.
- Explicit Open/double-click selects FullPinned, clears pending restoration and calls the activation path. Explicit Close clears restoration and hides the window.
- Added tests for pointer scheduling/dispatch, repeated events, fast pointer departure, active/inactive full state, hidden preview dismissal, background full restore, explicit open and close transitions.
- Validation passed after final code edits: `cargo fmt --all --check`; `cargo test --locked --workspace -- --test-threads=2` (194 tests: Core 43, bridge unit 3, process integration 1, UI 147); strict workspace Clippy; Windows MSVC target check; `git diff --check`.
- Computer Use runtime is unavailable in this environment; no physical tray hover/double-click QA was performed. Installed AUM v0.2.0 was not modified.
- Previous implementation commit: `f9ae5e3d2ea3d2b0932af00361ab969da55122b9`; PR #8 remains open and unmerged.

## Changed files in this follow-up
- `rust/agent-monitor-ui/src/flyout.rs`
- `docs/state/fix-tray-hover-background-preview.md`

## Next
- Commit and push this follow-up to PR #8, then request a fresh independent review.
- Check GitHub Actions for the new head.
- Physical Windows tray hover/double-click QA remains recommended.

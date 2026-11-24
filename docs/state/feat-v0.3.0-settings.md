# State: feat/v0.3.0-settings

## Goal
- Implement adaptive Settings window sizing with one content-derived height shared by all tabs.
- Keep the window within the active monitor work area, including taskbar space, while allowing internal scrolling on short displays.

## Git starting point
- Confirmed clean working tree on `feat/v0.3.0` at `39ef66e` and preserved that branch.
- Fetched `origin`; fast-forwarded local `main` to merge commit `7a01a3f2802ad2168f7b2c03c26cab0935034862`; verified equality with `origin/main`.
- Created `feat/v0.3.0-settings` from the synchronized `main`.
- Inspected and left the separate detached worktree untouched.
- `agent-init` and Bash are unavailable in this environment; created this branch state file manually. Project `AGENTS.md` was present and read.

## Progress
- Original adaptive sizing work remains in progress on this branch; no commit/push or external application changes.
- Claude review follow-up: replaced selected-tab measurement with three persistent Slint layouts, each exposing its independent preferred height. Rust reads Slint's max property without mutating the selected tab.
- N1: Flickable content height now follows the active page, while Window height continues to use the maximum across all pages. The common tab-selection function scrolls to the top. Real Slint 1.19 regression test scrolls About to the bottom in a constrained viewport and confirms About→General resets the offset and shrinks content extent without changing Window size.
- Removed automatic resizing from ordinary settings and language updates. Manual native resize remains supported and is not overridden by a toggle or status update. The auto size is set before the first show to avoid the previous post-show resize.
- The About update status has a fixed-height reserved region and a fixed-height action row. Dynamic provider text may grow the scrollable content; the open window height remains fixed.
- N2: monitor changes use the user's current logical size as the request and clamp only dimensions exceeding the new work area. Fitting size and position are preserved. Bounds adjustment is deferred while the left mouse button is held. A one-shot post-show check re-centers against the actual monitor and only clamps oversized dimensions, without a resize loop.
- Added Slint-backed preservation/clamp assertions plus simulated DPI tests at 100%, 125%, 150%, and 200%; work-area tests verify each axis is clamped independently.
- `cargo fmt --all --check` passed.
- `cargo test --locked --workspace -- --test-threads=2` passed: 43 core, 3 bridge unit, 1 bridge process integration, and 141 UI tests. Real Slint 1.19 N1/N2 regression cases passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` passed.
- `x86_64-pc-windows-msvc` workspace check and `git diff --check` passed.
- Changed files are limited to Settings layout/sizing, a Win32 input API feature for drag detection, the Slint testing-only dev dependency/lock entry, and layout docs/state.
- Physical multi-monitor/DPI movement is not available for manual testing here; geometry remains covered by simulated work-area tests.

## Constraints
- Do not commit or push, create a PR, install AUM, or modify release/Store artifacts.
- Preserve `feat/v0.3.0` and all other worktrees.

## Next
- Ready for final review/commit and PR when explicitly approved; no commit or push performed.

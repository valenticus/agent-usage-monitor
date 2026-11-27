# State: fix/v0.3.0-about-layout

_Handoff file for this branch, in the Agent Kit `handoff` skill format. Only this branch's agent edits it._

## Goal and constraints
- Fix the Settings → About update status layout shift for v0.3.0.
- Preserve the installed RC, provider behavior/settings, work-area sizing, small-display scrolling and DPI handling.
- Do not touch the Kilo-managed worktree or perform GUI/process manipulation.

## Done
<!-- each item: verified fact (with evidence) or assumption -->
- Inspected `main.slint` and settings sizing in `main.rs`: status visibility collapsed its row to 0 px when empty; non-empty statuses used 24–72 px based on wrapped text. About preferred height feeds shared settings sizing; its button position therefore moved with the status.
- Final compactness review: idle uses the existing neutral version chip in the product header; the status row collapses to 0 px, preserving the original 12 px distribution-label-to-button spacing.
- The single result text now appears immediately below the update button row. It uses Slint's natural wrapped preferred height with no cap; the row and its 8 px trailing gap grow within the Flickable, so complete localized messages remain readable without moving the update buttons.
- Settings sizing subtracts the dynamic status row and its 8 px trailing gap from the About page's window-sizing contribution. Thus status changes and reopening in a saved result state do not increase the common Settings window height; the expanded About content remains scrollable.
- Expanded Slint layout coverage for repeated Idle/Checking/Current/Available/Error transitions (including offline, HTTP, invalid metadata and timeout), all four locales, three themes, narrow content, stable button spacing and tab switching.

## Git state
<!-- HEAD, branch, `git status --short`, `git diff --stat` as captured -->
- Branch: `fix/v0.3.0-about-layout`, created from synchronized `main` (`8648085`).
- Base before compactness revision: `bf00245` (`Record About layout fix PR state`).
- PR: https://github.com/valenticus/agent-usage-monitor/pull/14 — open against `main`, not merged; this compactness revision is being prepared.

## Changed files
- `rust/agent-monitor-ui/ui/main.slint`
- `rust/agent-monitor-ui/src/main.rs`
- `rust/agent-monitor-ui/src/localization.rs`
- `docs/state/fix-v0.3.0-about-layout.md`

## Checks
<!-- commands run with results; not-run or stale checks named as such -->
- `cargo fmt --all --check` — passed.
- `cargo test --locked --workspace` — passed (45 Core, 3 bridge, 1 process integration, 153 UI; 202 tests total; doc tests passed).
- `cargo test --locked --release --workspace` — passed (45 Core, 3 bridge, 1 process integration, 153 UI; 202 tests total; doc tests passed). Release test build reports three existing dead-code warnings in the QA preview code.
- Compactness focus tests — passed: status transition/layout matrix; localized failures at 420×360 logical px; constrained-scroll About behavior.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed.
- `cargo check --locked --workspace --target x86_64-pc-windows-msvc` — passed.
- `cargo check --locked --workspace --release --target x86_64-pc-windows-msvc` — passed.
- `git diff --check` — passed.
- Manual GUI verification was not performed; the installed RC and GUI were left untouched. Headless Slint tests are not visual confirmation.

## Decisions (and why)
- Idle reuses the existing installed-version chip. Result text goes below the update buttons and takes its natural wrapped height, because a fixed slot clips long translations while moving the row below the buttons preserves their position.
- The current idle spacing before the update actions remains 12 logical px; result copy gets an 8 logical px trailing gap. Status growth is excluded from the window-sizing basis and handled by the existing Flickable.

## Blockers
- None known.

## Next actions
1. Await review of PR #14; do not merge automatically.

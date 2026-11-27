# State: fix/v0.3.0-about-layout

_Handoff file for this branch, in the Agent Kit `handoff` skill format. Only this branch's agent edits it._

## Goal and constraints
- Fix the Settings → About update status layout shift for v0.3.0.
- Preserve the installed RC, provider behavior/settings, work-area sizing, small-display scrolling and DPI handling.
- Do not touch the Kilo-managed worktree or perform GUI/process manipulation.

## Done
<!-- each item: verified fact (with evidence) or assumption -->
- Confirmed the user's manual GUI finding with the Slint testing backend: at 720 logical px, About grows from 738 px idle to 764 px with a normal one-line status, while common sizing stayed at 738 px. Flickable range was 26 px and content-y moved 0 to -26 px; the copyright was below the viewport. General (641 px) and Notifications (606–623 px) had zero range.
- Final compactness review: idle uses the existing neutral version chip in the product header; the status row collapses to 0 px, preserving the original 12 px distribution-label-to-button spacing.
- The single result text now appears immediately below the update button row. It uses Slint's natural wrapped preferred height with no cap; the row and its 8 px trailing gap grow within the Flickable, so complete localized messages remain readable without moving the update buttons.
- The About status expansion is now offset by reclaiming up to 26 px from four existing gaps after the update actions: 16/16/16/10 px contract to 8/8/8/8 px while a status is visible. Idle spacing remains unchanged, no status placeholder is reserved, and the 12 px gap before update controls remains.
- Normal status layouts retain the 720×738 logical test-backend size, page preferred height 738 px, zero scroll extent, and content-y 0.0. The update button remains at y=465 px, status y=505 px, and footer bottom y=714 px; normal status text is 18 px high. Long test copy remains naturally wrapped at 52 px and scrolls by its actual 34 px excess.
- Settings keeps one common height across tabs. Manual window dimensions are retained in memory on close and reused on reopen when they fit the current work area; existing clamping handles smaller work areas.
- Expanded Slint layout coverage for repeated Idle/Checking/Current/Available/Error transitions (including offline, HTTP, invalid metadata and timeout), all four locales, three themes, narrow content, zero scroll range for fitting states, footer recovery for long/constrained content, stable button/footer geometry, and tab switching.
- Isolated GUI QA hardening: `--qa-isolated --empty` uses the distinct QA mutex, skips autostart reconciliation and starts no live provider worker. QA mode now blocks automatic/manual update checks and external About links. The explicit `idle` preview value is accepted.
- Manual QA commands will give each status a fresh temporary `%APPDATA%`, keeping the installed AUM's settings out of the preview process and directing any setting writes to the temporary QA folder.

## Git state
<!-- HEAD, branch, `git status --short`, `git diff --stat` as captured -->
- Branch: `fix/v0.3.0-about-layout`, created from synchronized `main` (`8648085`).
- Base before compactness revision: `bf00245` (`Record About layout fix PR state`).
- Compactness revision commit: `15d3d49` (`Keep About update status compact and readable`), pushed to PR #14.
- Current PR base: `fe7541d` (`Harden isolated About QA preview`), clean and synced before this correction.
- PR: https://github.com/valenticus/agent-usage-monitor/pull/14 — open against `main`, not merged. This layout correction is in progress and not yet pushed.
- No installed AUM instance or GUI executable has been launched or manipulated.

## Changed files
- `rust/agent-monitor-ui/ui/main.slint`
- `rust/agent-monitor-ui/src/main.rs`
- `rust/agent-monitor-ui/src/localization.rs`
- `docs/settings-window-layout.md`
- `docs/state/fix-v0.3.0-about-layout.md`

## Checks
<!-- commands run with results; not-run or stale checks named as such -->
- `cargo fmt --all --check` — passed.
- `cargo test --locked --workspace` — passed on the current correction (45 Core, 3 bridge, 1 bridge process integration, 155 UI; 204 tests total; doc tests passed).
- `cargo test --locked --package agent-usage-monitor-ui --bin agent-usage-monitor isolated_qa_blocks_update_network_actions -- --nocapture` — passed.
- `cargo test --locked --release --workspace` — passed on the current correction (45 Core, 3 bridge, 1 bridge process integration, 155 UI; 204 tests total; doc tests passed). Release test build reports one existing dead-code warning for the debug-only `QA_ABOUT_PREVIEW` static.
- `about_distribution_and_status_layout_is_shared_and_stable` — passed after correction; tests status transitions across 3 distributions × 4 locales × 3 themes, identical dimensions on all tabs, zero scroll for normal states, unchanged update button/footer positions, and real long-text scrolling.
- `localized_update_status_wraps_fully_on_narrow_about_content` — passed at 420×360 logical px; localized status text remains fully wrapped, the button remains stationary, and scrolling to the bottom reveals the copyright footer.
- `settings_reopen_preserves_a_user_sized_window_when_available` — passed for initial sizing and retained user sizing.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed on the current correction.
- `cargo check --locked --workspace --target x86_64-pc-windows-msvc` — passed; the final subsequent edit was confined to the test module.
- `cargo check --locked --workspace --release --target x86_64-pc-windows-msvc` — passed; the final subsequent edit was confined to the test module.
- `cargo fmt --all --check` and `git diff --check` — passed.
- Windows CI and an isolated executable for this correction — pending push/CI.
- Manual GUI verification was not performed; the installed RC and GUI were left untouched. Headless Slint tests are not visual confirmation.
- After push, wait for Windows CI on the final head, then build from that exact passing head without launching it.

## Decisions (and why)
- Idle reuses the existing installed-version chip. Result text goes below the update buttons and takes its natural wrapped height, because a fixed slot clips long translations while moving the row below the buttons preserves their position.
- The idle layout has no status row; update copy remains below the actions. Ordinary single-line status growth uses 26 px recovered from existing post-action whitespace while preserving at least 8 px in each adjusted gap. Exceptional text beyond that measured allowance uses the existing Flickable.
- The isolated manual-QA binary must be a debug build because preview arguments are debug-only. Its build target and each preview's `%APPDATA%` will be separate temporary paths; the executable will remain unlaunched.

## Blockers
- None known.

## Next actions
1. Complete all requested validations.
2. Commit and push this correction to existing PR #14, then wait for CI on the exact new head.
3. Build from the exact passing PR head and provide the executable path, hash, and safe preview commands without launching it.

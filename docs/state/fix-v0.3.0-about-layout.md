# State: fix/v0.3.0-about-layout

_Handoff file for this branch, in the Agent Kit `handoff` skill format. Only this branch's agent edits it._

## Goal and constraints
- Fix the Settings → About update status layout shift for v0.3.0.
- Preserve the installed RC, provider behavior/settings, work-area sizing, small-display scrolling and DPI handling.
- Do not touch the Kilo-managed worktree or perform GUI/process manipulation.

## Done
<!-- each item: verified fact (with evidence) or assumption -->
- Latest native report (user-provided, before this correction): scale 1.0; native client, Slint logical window, and Flickable viewport are all 738 px; About content/preferred height is 748 px; scroll extent 10 px; button y 465→455 during scroll. General is 677 px and Notifications 606 px. This rules out outer-frame/DPI conversion as the cause.
- The initial common-height measurement runs while the Winit window is hidden. QA-03R shows the earlier first-show correction did not alter the observed 738 px client. Code inspection confirms `show_settings_centered` does schedule `show_settings_measured`, so bypassing that wrapper is not the cause. Which first-show guard, screen lookup, work-area clamp, or native resize outcome explains the result remains unconfirmed pending expanded diagnostics.
- Changed first-show Windows behavior to remeasure and apply the common tab height once after native layout settles (150 ms), only if no retained size exists and the user has not resized. The existing work-area and DPI geometry clamp still governs the applied size. Retained/manual sizing is preserved.
- Added `native_fit_resolves_748_required_height_from_738_client`, covering the reported 10 px initial extent and the expected 720×748 fit in a sufficiently large work area. Expanded debug-only `--qa-geometry` output to show first-show guard inputs, screen lookup failure, requested/clamped dimensions, whether `set_size` ran, immediate result, before/after fit, and later settled client geometry.
- Native post-fix GUI measurements have not been collected. The code-path correction and headless late-reflow regression are verified; do not describe this as native visual PASS or assert post-fix native scroll extent without manual measurement.
- Final geometry investigation on exact prior PR head `0a4b5f4a54551c4a858aff09019e6c0cb4758da2`: headless Slint measurements show a 720×738 logical window/client, 738 px Flickable viewport and About preferred height, zero scroll range, and `content-y` 0 before/after attempted bottom scrolling. This does not reproduce the user's native Windows overflow, so the native root cause remains unestablished and no layout/padding adjustment is justified.
- Added debug-only `--qa-geometry` diagnostics for isolated previews. While Settings is visible, it samples native Win32 outer/client rectangles, scale, Slint logical window dimensions, Flickable viewport/content/extent/content-y, active/About preferred heights, and button/footer positions every 500 ms. The trace can correlate measurements with a manual scroll attempt.
- Strengthened the geometry matrix to set the same Settings sizing property used by production and assert the Flickable viewport matches the Slint logical client height before checking zero range and scroll behavior.
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
- Current PR head at start of geometry investigation: `0a4b5f4a54551c4a858aff09019e6c0cb4758da2` (`Fit About statuses without scroll`), clean and synced before this correction.
- PR: https://github.com/valenticus/agent-usage-monitor/pull/14 — open against `main`, not merged. Geometry correction commit `f531b50b3fc989a2626763978e59fc2217677665` is pushed; exact-head Windows CI passed (run `46069491077`).
- Isolated MSVC debug build from that exact source commit, not launched: `%TEMP%\AUM-PR14-QA-f531b50-build\x86_64-pc-windows-msvc\debug\agent-usage-monitor.exe`, SHA256 `CC94281B0AC06B8373E412633D7677DC491EA30D1B76DEE8DBE37B9A5829A221`.
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
- Focused geometry matrix — passed after adding the sizing-property/client-viewport consistency assertion; headless range remains exactly zero for normal states, while long content retains measured scrolling.
- Windows-native geometry after the correction remains unmeasured. The new diagnostics distinguish a skipped fit, work-area clamp, missing screen handle, and failed/late native resize; no GUI executable was launched.
- `cargo fmt --all --check` — passed.
- `cargo test --locked --workspace` — passed: 45 Core + 3 bridge + 1 bridge-process + 155 UI tests; doc tests passed.
- `cargo test --locked --release --workspace` — passed on retry with the same counts. The first full run transiently failed `cancellation_kills_child_and_returns_cancelled_error` because its mock child did not start; the focused rerun and complete retry passed. The release test build emits the existing debug-preview static dead-code warning.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed.
- `cargo check --locked --workspace --target x86_64-pc-windows-msvc` and release variant — passed.
- `git diff --check` — passed.
- PR Windows CI on `c046723daf8253cbde4ac5c71a354500796e940e` — passed (run `46064879606`: formatting, static MSIX validation, Clippy, Windows tests, application build, artifact upload).
- Isolated Windows MSVC debug executable from that PR head — built offline, not launched: `%TEMP%\AUM-PR14-QA-c046723-build\x86_64-pc-windows-msvc\debug\agent-usage-monitor.exe`, SHA256 `CC9E43F67C9BC3ABD25355095AF6098821591556C8D1C8102DF393875967EE47`.
- PR #14 remains open and unmerged. Native GUI root cause and visual verification are pending manual measurements from the affected Windows display.
- Manual GUI verification was not performed; the installed RC and GUI were left untouched. Headless Slint tests are not visual confirmation.
- Push the diagnostic/regression correction, wait for CI on its exact head, build a fresh isolated MSVC debug executable without launching it, then have user provide the new `QA_GEOMETRY` output. Confirm first-show eligibility, screen lookup, work-area clamp, `set_size` request/result, settled viewport/content/extent, and scroll behavior before claiming native layout fixed. Keep PR #14 open.

## Decisions (and why)
- Idle reuses the existing installed-version chip. Result text goes below the update buttons and takes its natural wrapped height, because a fixed slot clips long translations while moving the row below the buttons preserves their position.
- The idle layout has no status row; update copy remains below the actions. Ordinary single-line status growth uses 26 px recovered from existing post-action whitespace while preserving at least 8 px in each adjusted gap. Exceptional text beyond that measured allowance uses the existing Flickable.
- The isolated manual-QA binary must be a debug build because preview arguments are debug-only. Its build target and each preview's `%APPDATA%` will be separate temporary paths; the executable will remain unlaunched.

## Blockers
- None known.

## Next actions
1. Collect `--qa-geometry` output from a manual isolated preview while Settings is visible and after attempting to scroll About.
2. Compare HWND outer/client physical sizes, scale-converted Slint logical size, Flickable viewport/content/range/content-y, preferred heights, and footer position.
3. Establish the native Windows-specific root cause from those measurements before proposing any layout correction; keep PR #14 open until then.

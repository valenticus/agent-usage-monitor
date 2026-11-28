# State: fix/v0.3.0-about-layout

_Handoff file for this branch, in the Agent Kit `handoff` skill format. Only this branch's agent edits it._

## Goal and constraints
- Fix the Settings → About update status layout shift for v0.3.0, and the native Windows Settings fit (About needed 748 px, the window stayed at 738 px, leaving 10 px of scroll).
- Preserve the installed RC, provider behavior/settings, work-area sizing, small-display scrolling and DPI handling.
- Do not touch the Kilo-managed worktree or perform GUI/process manipulation. Do not merge PR #14 without explicit approval.

## Done
<!-- each item: verified fact (with evidence) or assumption -->
- About layout:
  - Idle reuses the product header's version chip, and the status row collapses to 0 px.
  - The result text sits below the update buttons at its natural wrapped height.
  - Up to 26 px is reclaimed from four post-action gaps (16/16/16/10 → 8/8/8/8 px) while a status is visible, so normal one-line statuses fit with zero scroll and the update button stays put.
  - Longer text uses the Flickable.
  - All tabs share one window size.
- Native-fit root cause #1 (verified against the QA trace at `0d164b1` and the Slint 1.19 / winit 0.30.13 sources):
  - On Windows, Winit's `set_size` is asynchronous. `i-slint-backend-winit-1.19.0` `resize_window` gets `None` from `request_inner_size`, and winit's `WindowFlags::set_size` uses `SetWindowPos(SWP_ASYNCWINDOWPOS)`. `Window::size()` therefore stays stale until `WindowEvent::Resized`.
  - The post-show guard compared the current size (720x738, after the app's own first-fit resize landed) with a snapshot taken right after `set_size` (stale 720x480). This misclassified the app's own resize as manual: `fit_allowed=false`.
  - The fallback branch then requested the current size, so `required=748` produced `requested=738`.
- Native-fit fix (`27ca78d`):
  - `recalculate_settings_layout` returns the clamped size it requested.
  - The post-show fit is allowed only while the current size is app-owned, meaning the first fit's request or the pre-fit size (resize not landed yet).
- Root cause #2 (independent review of `27ca78d`):
  - Close always stored the current size as "retained", so automatic or clamped sizes stuck for the rest of the session. Reopen skipped refitting after language/content growth or a monitor change.
  - The post-show fit chose the monitor under the cursor.
- Fix (`2295c5a`):
  - `SettingsSizeOwnership` is fed by a `SetWindowSubclass` tracker on the Settings HWND. Only a `WM_ENTERSIZEMOVE`…`WM_EXITSIZEMOVE` loop that contains `WM_SIZING` (border drag or keyboard sizing) records a user size. It is stored as the final client size, converted with `GetDpiForWindow`.
  - App/Winit `SetWindowPos`, DPI changes, caption moves, maximize and snap are never retained. Close records nothing.
  - Automatic sizes are remeasured on every reopen.
  - `settings_fit_monitor(visible)`: a hidden window opens on the cursor's monitor. The visible window (post-show fit, monitor refresh) uses its own monitor.
  - The tracker never calls Slint and never resizes the window.
- Maximize: previously the maximized client size could be retained. Now it is never recorded, and an earlier user-selected size is kept. Winit clears `MAXIMIZED` when a different size is requested. Native QA-06 confirmed that maximize → close → reopen works.
- Intentional behavior: Aero Snap and Win+Arrow placements are not user-selected sizes, so after reopen Settings fits automatically again.
- Isolated QA hardening (debug-only): `--qa-isolated --empty` uses a separate mutex, skips autostart and live workers, and blocks update checks and external links. `--qa-geometry` traces native/Slint geometry, the fit decisions (`auto_size`, `pre_fit`, `app_requested`, `monitor`) and `QA_GEOMETRY_SIZE_LOOP_END`.
- Native QA (user-verified on Windows, single display):
  - QA-05 FULL PASS: About `scroll_extent=0`, no unintended scrolling, identical window size on all tabs, manual resize persists after reopen, post-show fit works.
  - QA-06 FULL PASS: automatic reopen, manual resize persistence, About zero overflow, stable dimensions across tabs, maximize and reopen.
- Independent Codex review of `2295c5a` (read-only): PASS WITH CONDITIONS. The only finding is the theoretical `try_lock` contention described under Blockers/risks. It confirmed the subclass lifecycle, `WM_SIZING` semantics, monitor selection and the DPI/work-area clamp.

## Git state
<!-- HEAD, branch, `git status --short`, `git diff --stat` as captured -->
- Branch `fix/v0.3.0-about-layout`, created from `main` `8648085`. PR: https://github.com/valenticus/agent-usage-monitor/pull/14 — open, not merged.
- HEAD before this documentation commit: `2295c5ace2e58a6ff7d0483e35c7dba1d50cd663` (final implementation, `Retain only user-selected Settings sizes`). Parent `27ca78d` (`Honor asynchronous native Settings resize`). Clean and synced with origin.
- The implementation was developed in the isolated worktree `C:\project\agent-usage-monitor-native-fit` (branch `fix/v0.3.0-about-native-fit`) and fast-forwarded here. That branch's handoff file is removed from the PR in this commit; its findings are kept here and in `docs/settings-window-layout.md`.
- QA build of `2295c5a` (not launched by the agent): `%TEMP%\AUM-PR14-QA-2295c5a\agent-usage-monitor.exe`, SHA256 `520468D87953BA7751E8CCF10F3F0445BBCC7FCB579A107E8281A714248421B3`.

## Changed files
- `rust/agent-monitor-ui/ui/main.slint`
- `rust/agent-monitor-ui/src/main.rs`
- `rust/agent-monitor-ui/src/localization.rs`
- `docs/settings-window-layout.md`
- `docs/state/fix-v0.3.0-about-layout.md`

## Checks
<!-- commands run with results; not-run or stale checks named as such -->
- On `2295c5a`, all passed:
  - `cargo fmt --all --check`
  - `cargo test --locked --workspace`: 45 Core, 3 bridge, 1 bridge-process, 165 UI
  - `cargo test --locked --release --workspace`: same counts, plus the existing `QA_ABOUT_PREVIEW` dead-code warning
  - `cargo clippy --locked --workspace --all-targets -- -D warnings`
  - `cargo check --locked --workspace --target x86_64-pc-windows-msvc`, debug and release
  - `git diff --check`
- PR Windows CI on `2295c5a`: passed (run `46124398244`). Earlier heads also passed: `27ca78d` run `46077869883`, `0d164b1` run `46073318821`.
- Regression tests:
  - `native_first_show_fit_is_allowed_after_asynchronous_app_resize`
  - `native_fit_resolves_748_required_height_from_738_client`
  - `native_second_fit_requests_remeasured_height_within_work_area`
  - `auto_sized_settings_are_measured_again_after_close_and_reopen`
  - `language_or_content_growth_between_opens_refits_an_automatic_size`
  - `manual_resize_persists_across_close_and_reopen`
  - `only_a_native_user_size_loop_marks_the_size_as_user_selected`
  - `post_show_fit_uses_the_monitor_containing_settings`
  - `moving_settings_between_monitors_keeps_work_area_and_dpi_bounds`
  - plus the About layout matrix (3 distributions × 4 locales × 3 themes)
- Known flaky tests that predate this PR: `codex::tests::cancellation_kills_child_and_returns_cancelled_error` (40 s PowerShell mock start budget from `235ca51` on main) and `claude_statusline::tests::generated_command_runs_in_every_available_shell`. Both spawn external processes and occasionally fail under parallel load. Each passed in isolation and on full reruns. This is a CI risk, not a product risk.

## Decisions (and why)
- Detect manual sizing from the native `WM_SIZING` loop, not from size comparisons, because Winit resizes asynchronously and DPI/maximize/snap change the size without sizing intent.
- Record the user size at `WM_EXITSIZEMOVE`, not on close, so a maximized or later app-clamped size never overwrites the user's choice.
- Choose the monitor by visibility, so open, post-show fit and monitor refresh follow one rule.
- No About padding change for 748 vs 738: native text metrics differ from headless, and the post-show remeasure absorbs the difference.
- The manual-QA binary is a debug build, because the preview arguments are debug-only.

## Blockers
- None for merge readiness. Merge requires explicit user approval.
- Known limitations and risks:
  - Multi-monitor and mixed-DPI behavior is not verified natively (single display available). It is covered only by headless synthetic geometry tests.
  - Theoretical `try_lock` contention: the tracker uses `SETTINGS_LAYOUT.try_lock()` and drops a size-loop event if the lock is held. In production every lock scope is a single statement on the UI thread with no Win32 call or message pumping, so this cannot happen today. A future change that holds the lock across a Win32 call could lose a manual size, though it would not deadlock.
  - Headless tests do not dispatch real Win32 messages through the subclass. That path is covered only by native QA-06.

## Next actions
1. User decides on merging PR #14 (CI green, QA-05/QA-06 PASS). Do not merge without explicit approval.
2. Optional follow-ups: native multi-monitor QA when a second display is available; lock-free tracker state if extra robustness is wanted.

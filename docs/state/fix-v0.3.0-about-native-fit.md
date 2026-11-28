# State: fix/v0.3.0-about-native-fit

_Handoff file for this branch, in the Agent Kit `handoff` skill format. Only this branch's agent edits it._

## Goal and constraints
- Follow-up to the independent review of PR #14 (reviewed HEAD `27ca78d`, verdict PASS WITH CONDITIONS).
- Task 1: retain only genuine user-selected Settings sizes; automatic sizes must be measured again on reopen.
- Task 2: the 150 ms post-show fit must use the monitor containing Settings, not the cursor's monitor.
- Task 3: regression coverage. Investigate maximize behaviour without widening scope.
- Preserve the native-fit fix verified by QA-05, the DPI/work-area clamp, and small-screen scrolling. Do not touch the installed RC, merge PR #14 or force-push.
- Worktree `C:\project\agent-usage-monitor-native-fit`, branch `fix/v0.3.0-about-native-fit`, based on PR #14 head `27ca78d`.

## Done
<!-- each item: verified fact (with evidence) or assumption -->
- Root cause, Task 1 (verified in code): at `27ca78d`, `on_close_requested` called `remember_settings_window_size`, which stored the current size on every close. That handler is the only hide path for Settings. Any size, automatic or clamped, therefore became `retained_size`, `settings_open_size` reused it, and the post-show fit was skipped on every later open.
- Root cause, Task 2 (verified in code): the post-show fit called `recalculate_settings_layout(.., true, false)`, and `use_common_size=true` also selected `use_cursor=true` in `settings_screen_signature`.
- Fix, Task 1: added `SettingsSizeOwnership`, kept in `SETTINGS_LAYOUT`. The Settings HWND gets a `SetWindowSubclass` tracker, the same pattern as the flyout caption guard. A size counts as user-selected only when a `WM_ENTERSIZEMOVE`…`WM_EXITSIZEMOVE` loop contains `WM_SIZING`. The final client size is stored, converted to logical units with `GetDpiForWindow`.
  - Never counted as user-selected: app/Winit `SetWindowPos` resizes, DPI changes, caption moves, maximize, snap. The decision does not depend on an asynchronous size snapshot.
  - Close no longer records anything, and `remember_settings_window_size` was removed.
  - The tracker only stores numbers (with `try_lock`) and removes itself on `WM_NCDESTROY`. It never calls into Slint or resizes the window, so it cannot start a resize loop.
- Fix, Task 2: `settings_fit_monitor(visible)` picks the cursor's monitor while Settings is hidden (open) and the window's monitor while it is visible (post-show fit, monitor refresh). The QA trace prints `monitor=`.
- Post-show fit gate unchanged: it is still based on app-owned sizes, so the 748/738 native fix is preserved. It now runs on every automatic-size open, not only the first.
- Maximize investigation (code only, not native): the old behaviour recorded the maximized client size as retained (confirmed defect class). Now no maximized size is recorded. Winit 0.30.13 `request_inner_size` clears `MAXIMIZED` when the size differs (source read). Whether a window closed while maximized reopens restored has not been verified natively.
- Non-Windows dev builds no longer retain any size, because the tracker is Windows-only. Windows is the product target.

## Git state
<!-- HEAD, branch, `git status --short`, `git diff --stat` as captured -->
- Base `27ca78d` (PR #14 head). Changes: `rust/agent-monitor-ui/src/main.rs`, `docs/settings-window-layout.md`, this state file. The commit SHA is reported in chat; this file cannot contain its own hash. Not pushed.

## Changed files
- `rust/agent-monitor-ui/src/main.rs`
- `docs/settings-window-layout.md`
- `docs/state/fix-v0.3.0-about-native-fit.md`

## Checks
<!-- commands run with results; not-run or stale checks named as such -->
- `cargo fmt --all --check` and `git diff --check`: passed.
- `cargo test --locked --workspace`: passed (45 Core, 3 bridge, 1 bridge-process, 165 UI).
- `cargo test --locked --release --workspace`: passed, same counts. Emits the existing `QA_ABOUT_PREVIEW` dead-code warning.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo check --locked --workspace --target x86_64-pc-windows-msvc`, debug and release: passed.
- New tests:
  - `auto_sized_settings_are_measured_again_after_close_and_reopen`
  - `language_or_content_growth_between_opens_refits_an_automatic_size`
  - `manual_resize_persists_across_close_and_reopen`
  - `only_a_native_user_size_loop_marks_the_size_as_user_selected`
  - `post_show_fit_uses_the_monitor_containing_settings`
  - `moving_settings_between_monitors_keeps_work_area_and_dpi_bounds`
- Retained tests: `native_first_show_fit_is_allowed_after_asynchronous_app_resize`, `native_fit_resolves_748_required_height_from_738_client`, `native_second_fit_requests_remeasured_height_within_work_area`, `native_first_show_remeasure_respects_retained_and_user_resized_sizes`.
- Native GUI: NOT verified. Headless tests cannot drive the Win32 size loop.

## Decisions (and why)
- Detect manual sizing from `WM_SIZING`, not from size comparisons, because Winit resizes asynchronously and DPI/maximize/snap change the size without user sizing intent.
- Record the user size at `WM_EXITSIZEMOVE`, not on close, so maximize or a later app clamp never overwrites the user's choice.
- Choose the monitor by visibility, so all callers (open, post-show, refresh) follow one rule.

## Blockers
- Native GUI verification by the user.

## Next actions
1. User runs the isolated QA build: auto-size → close → reopen (expect `auto_size=true fit_allowed=true`); border resize → close → reopen (expect `QA_GEOMETRY_SIZE_LOOP_END ... user_size_recorded=Some(..)`, and the size kept); move between monitors; maximize → close → reopen; small display still scrolls.
2. On PASS and approval: push and fast-forward PR #14. Wait for CI. Do not merge.

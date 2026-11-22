# State: fix/tray-resume-zorder

## Goal and constraints
- Fix two tray issues with equal priority: (1) the tray icon after sleep/resume and Explorer restarts; (2) the hover popup appearing behind other windows.
- Worktree `C:\project\agent-usage-monitor-tray`, branch from `main` at `daba890`. Not committed, pushed or merged (needs explicit approval).
- Do not touch the installed AUM v0.1.1, `feat/store-msix`, releases, WinGet or the Store.

## Root causes (confirmed in code)
- The grey striped icon after sleep is AUM's own **Stale** rendering. Data older than 900 s is drawn in `[159,151,132]` with every third row skipped. Codex refreshed only on the 5-minute interval, and nothing reacted to resume. The Stale visual is kept by design.
- The tray window was message-only (`HWND_MESSAGE`), so it could not receive `TaskbarCreated` or `WM_POWERBROADCAST`. Neither was handled anyway, so after an Explorer restart the icon was lost until AUM restarted, while `NIM_MODIFY` failed and was logged every second.
- The compact hover preview was shown without activation and its Z-order was never set. It kept its old position, below any app activated since the last show.

## Done (uncommitted)
- `tray.rs`:
  - hidden top-level tray window (`WS_POPUP`, `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, never shown);
  - `TaskbarCreated` registered; `register_tray_icon` (NIM_ADD → NIM_MODIFY fallback → NIM_SETVERSION 4, same hwnd+uID so no duplicates);
  - `NIM_MODIFY` failure falls back to one re-registration;
  - `OnceLog` logs each outage once;
  - `WM_POWERBROADCAST` resume → `TrayEvent::Resumed` and icon invalidation;
  - a start-up `NIM_ADD` failure is no longer fatal (retried on `TaskbarCreated` and on the next icon update);
  - HICON ownership unchanged: the new icon is destroyed on failure, the previous one after success, the dynamic one at exit; the static icon is never destroyed.
- `flyout.rs`: `wants_topmost` rule plus a single `sync_topmost`, called after every show/hide/presentation change, in `closed_externally`, and in `tick` as a safety net. TOPMOST is applied only to the visible compact preview, via `SetWindowPos(HWND_TOPMOST/HWND_NOTOPMOST, SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE|SWP_NOOWNERZORDER)`. TOPMOST is cleared before the full flyout is activated. Failures are logged once.
- `main.rs`: `TrayEvent::Resumed` restarts one 10 s single-shot timer (`RESUME_REFRESH_DELAY`) → `request_refresh()`. This coalesces repeated resume notifications, and the bounded worker queues coalesce with pending manual refreshes. The flyout is not shown.
- Tests:
  - `tray::tests::only_resume_power_notifications_trigger_recovery`;
  - `tray::tests::registration_failures_are_logged_once_per_outage`;
  - `flyout::tests::only_the_visible_compact_preview_is_topmost`.

## Checks (on AC power)
- `cargo fmt --all --check` — passed.
- `cargo check --locked --workspace --all-targets -j 2` — passed, no warnings.
- `cargo clippy --locked --workspace --all-targets -j 2 -- -D warnings` — passed.
- `cargo test --locked -p agent-usage-monitor-ui -j 2` — 72/72 passed, including the 3 new tests.
- `cargo test --workspace`:
  - Core 31/31 and bridge unit 3/3 passed;
  - `claude_bridge_process` failed because the running installed AUM (PID 23520, `--enable-claude-bridge`) owns the bridge pipe. This is a known environmental failure; the Core crate is unchanged.

## Runtime validation (2026-11-22, debug build, installed AUM closed via tray, isolated APPDATA)
- Backups: `%TEMP%\aum-tray-test\backup\` (registry exports, `snapshot.json`, `baseline-after-close.json` with SHA-256 of the user's settings files).
- Hover preview **passed**:
  - probe: while shown `visible=True`, `WS_EX_TOPMOST=True`, foreground unchanged (VS Code); after hiding `visible=False`, `WS_EX_TOPMOST=False`, foreground unchanged;
  - Z-order probe: preview #4 above ChatGPT #7 (ChatGPT has no TOPMOST flag);
  - the user confirmed by hand that the preview works correctly over ChatGPT.
- The first launcher did not capture logs (PowerShell event actions do not run while waiting); fixed by redirecting stderr/stdout straight to files.
- Short sleep/resume (~2 min, test PID 27228) **PASS**:
  - 11:55:38 resume → icon invalidated and re-accepted;
  - 11:55:48 (+10 s) `RESUME:` → refresh queued for Codex and Claude;
  - Codex `id=1` completed 11:55:51, Claude CLI completed 11:55:55;
  - 4× `TRAY_ICON: Windows accepted`, no tray registration errors.
  - Observations, not regressions: the 5-minute interval fired its missed tick right after resume (tokio's clock appears to count sleep on Windows). That background Codex refresh failed, and the existing log does not show why. The resume refresh 10 s later succeeded, so Codex was refreshed twice.
- Long sleep/resume (20–30 min, Stale state): **NOT TESTED** (postponed by the user).
- Full mode and 14 hover cycles (recorder `states-fullmode-cycles.log`, 12:03–12:05):
  - full mode **PASS**: height 243, `topmost=False`, foreground AUM; × closes with `topmost=False`; compact→full drops TOPMOST before activation;
  - TOPMOST never stuck across 14 cycles **PASS**. Two transient lags of about 120 ms were corrected by the `tick` safety net; `is_visible()` likely lags just after show/hide;
  - focus **FAIL**: after the double-click, every compact show activated the preview (`foreground=AUM-flyout`).
  - Root cause (winit 0.30.13 `window_state.rs:327–336`): `with_active(false)` applies only to the first show (the hidden `warm_up`); every later `set_visible(true)` uses `ShowWindow(SW_SHOW)`, which activates whenever Windows lets the process take the foreground (after the user clicked the tray icon).
  - Fix: `NativeStyle`/`native_style` plus a single `sync_native_style`. `WS_EX_NOACTIVATE` is applied for the compact preview before any show, and cleared before the full flyout is shown or activated; TOPMOST rules are unchanged. New tests: `compact_preview_never_activates_and_full_flyout_always_can`, `mode_transitions_set_the_expected_native_style`. fmt/check/Clippy pass, UI 74/74.
- Re-test 1 (`states-noactivate.log`): focus held, but `noactivate` was False while shown.
  - winit 0.30.13 rewrites `GWL_EXSTYLE` right after `ShowWindow` (`window_state.rs:399–407`), and the cached comparison never restored it.
  - Second fix: decide from the live style (`read_no_activate`) using a pure `no_activate_action`, with at most 4 writes per presentation; the budget is reset in `present`.
  - New tests: `no_activate_is_decided_from_the_live_style`, `winit_style_rewrites_are_repaired_within_the_write_budget`, `a_style_that_keeps_being_reset_stops_after_the_budget`.
  - fmt/check/Clippy pass, UI **77/77**.
- **Final runtime check PASS** (`states-final.log`, test PID 29408):
  - 6 compact shows over ChatGPT: `noactivate=True` and `topmost=True` while shown (one 123 ms TOPMOST lag fixed by `tick`), `topmost=False` after hiding, ChatGPT kept the focus;
  - double-click → full: `topmost=False`, `noactivate=False`, foreground AUM; × closes;
  - hover **after** the double-click (twice): `noactivate=True`, `topmost=True`, foreground stayed `explorer` (not AUM), so the earlier FAIL is resolved;
  - 0 errors in the app log.
- Explorer restart recovery **PASS** (user restarted "Windows Explorer" in Task Manager; shell PID 7488 → 26160 at 12:00:57):
  - 12:01:00 `TRAY_ICON: taskbar recreated; restoring…`, followed in the same second by `Windows accepted`;
  - no new errors; test AUM PID 27228 kept running;
  - the user confirmed one coloured AUM icon and a working hover preview afterwards.

## Codex review follow-up (PASS WITH CONDITIONS) — 2026-11-22
**Duplicate Codex refresh after resume (fixed, unit-tested):**
- Cause, confirmed in code and in the first sleep log:
  - the worker's timer wait does not elapse during sleep, while tokio's `Instant` counts it;
  - so the overdue 5-minute tick became ready at the same moment the delayed resume request woke the worker (11:55:48), and `select!` ran both.
  - Either order duplicates the call: tick then request, or request then the still-overdue tick.
- Fix (`main.rs`):
  - `ManualRefreshRequest` carries `origin: RefreshOrigin { Manual, Resume }`;
  - a Resume request is skipped when Codex refreshed successfully within `RESUME_COALESCE_SECONDS` (30 s) (pure `skip_redundant_resume_refresh`). A failed overdue tick, as on 2026-11-22, therefore does not suppress the retry;
  - every request-driven refresh calls `interval.reset()`, so an overdue tick cannot repeat it immediately;
  - the 10 s `RESUME_REFRESH_DELAY` is kept; manual refreshes are never skipped.
- Periodic behaviour: unchanged between requests. After a manual or resume refresh, the next periodic tick is 5 min later instead of on the old schedule.
**Duplicate Claude CLI refresh after resume (second P2 from the Codex review; fixed, unit-tested):**
- Cause (`claude_refresher::run_with_intervals`): when the periodic timer refreshed first (for example on wake-up) and the delayed resume request arrived later, the CLI ran twice. In the opposite order there was no duplicate: every refresh restarts the wait, so an overdue timer is dropped. When both are ready together, the queued request rides along with the same run.
- Fix:
  - the scheduler records `last_success_at` (wall clock; failures leave it unchanged);
  - a Resume request uses the same `skip_redundant_resume_refresh` rule as Codex: it is skipped after a successful CLI refresh within 30 s, and the scheduler then waits out the rest of the periodic interval;
  - manual requests are never skipped, and a failed refresh never suppresses the retry;
  - the Claude bridge IPC is unchanged.
- Tests:
  - `resume_request_after_a_successful_timer_refresh_is_skipped` (timer first; a manual request still runs);
  - `resume_request_after_a_failed_timer_refresh_retries`;
  - `timer_due_after_a_resume_refresh_does_not_run_it_again` (resume first, with an overdue timer).

**`claude_bridge_process` isolation (implemented; later run locally next to a live AUM and in CI, both pass):**
- Cause: the server and the bridge client both used the hard-coded production pipe, so the test reached a running AUM's listener and injected a synthetic Claude snapshot.
- Core changes (`claude_pipe.rs`):
  - `run_claude_status_pipe_named(name, …)` (production still uses `run_claude_status_pipe` → `CLAUDE_PIPE_NAME`, and the AUM listener never reads the environment);
  - the bridge client resolves its pipe with `resolve_client_pipe_name`: no override → production; `AUM_CLAUDE_PIPE_NAME` is accepted only as `\\.\pipe\AgentMonitor.ClaudeStatusLine.test-` plus 1–64 `[A-Za-z0-9-]`; any other value is rejected (InvalidInput → bridge "delivery failed") instead of falling back.
- Test (`tests/claude_bridge_process.rs`): a unique per-run test pipe for both the server and the bridge child (via the env override); it asserts the name differs from production and is accepted.

**Checks (AC power, `-j 2`):**
- fmt — passed; check `--workspace --all-targets` — passed, no warnings; Clippy `-D warnings` — passed;
- core `--lib` **33/33** (new: `bridge_uses_the_production_pipe_only_without_an_override`, `invalid_overrides_are_rejected_instead_of_falling_back`);
- bridge unit 3/3;
- UI **78/78** (new: `resume_refresh_is_skipped_only_after_a_recent_successful_refresh`).
- `claude_bridge_process` with the isolated pipe: **1/1 pass** locally (while the installed AUM listened on the production pipe) and in CI run 45019228305.
- After the Claude fix: `cargo test --locked --workspace` — core 33/33, bridge unit 3/3, `claude_bridge_process` 1/1, UI **81/81**; fmt and Clippy pass. Run on battery (89 %), incremental build.

## PR
- https://github.com/valenticus/agent-usage-monitor/pull/1 (open; no merge without approval).

## Not tested / known limitations
- Long sleep (20–30 min, Stale state) and repeated sleep cycles: NOT TESTED.
- The duplicate-refresh fixes (Codex and Claude): unit-tested only; no runtime sleep test with the new build.
- DPI 125–200 %, multiple monitors, Windows 10: NOT TESTED. Release build: not runtime-tested (debug only).
- A transient TOPMOST lag of about 120 ms right after a show or hide is corrected by the 50 ms `tick` safety net.
- The reason for the Codex failure right after resume is not logged (pre-existing).

## Next actions
1. Independent review of PR #1, then merge only with explicit approval.
2. After merge, port to `feat/store-msix` (shared code; no MSIX-specific changes needed).

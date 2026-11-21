# State: feat/ui-redesign-themes

## Goal and constraints
- Finish the AUM flyout and Settings redesign, add four persistent themes, provider icon integration, and Windows CI artifact delivery.
- Preserve prior Claude status-line delivery, opt-in CLI refresh, Codex retrieval, and tray startup fixes.
- Never run authenticated Claude requests, access credentials, modify global settings, reset the worktree, or push to main.

## Done
- Resumed implementation on the existing `feat/ui-redesign-themes` branch from `ef5b3a55190221247bc3ebe15d688a5b989ce069`; no branches were deleted, reset, or switched.
- Redesigned the compact flyout with localized heading/actions, separate provider cards, aligned 5-hour and weekly rows, remaining-percentage bars, reset/status presentation, and refresh action wired to Codex refresh.
- Updated Settings navigation/content layout and localization for Ukrainian, Polish, and English; retained provider and notification controls and added the Claude background-refresh explanation/toggle.
- Added persistent System, Graphite, Navy, and Light theme modes. Legacy `dark` config deserializes as Navy. System follows Windows dark/light preference changes.
- Added Windows system accent lookup with fallback accent.
- Added optional local SVG loading for authentic provider marks from `%APPDATA%\\Agent Usage Monitor\\provider-icons\\{codex,claude}.svg`; no provider logos are bundled because redistribution permission was not confirmed. Documented the limitation in `docs/provider-icons.md`.
- Kept the previous bridge delivery fixes, optional Claude CLI refresh, and tray fallback/error diagnostics intact.
- Updated Windows GitHub Actions validation and artifact workflow for feature branches and PRs to main. It uploads the runnable exe plus LICENSE and README; no release publishing.

## Checks
- `cargo check -p agent-usage-monitor-ui -j 2` — passed.
- `cargo fmt --all --check` — passed after running `cargo fmt --all`.
- `cargo test -p agent-usage-monitor-ui -j 2` — passed: 41 tests.
- `cargo clippy --workspace -- -D warnings` — passed.
- `cargo build -p agent-usage-monitor-ui -j 2` — passed, 31.99 s incremental Windows build.
- `git diff --check` — passed.
- Build warning: Windows SDK `rc.exe` is absent locally; build.rs reports the icon resource was not embedded. Runtime fallback remains present.
- No authenticated Claude `/usage` command was run. No Cargo build/test processes remain active.

## Git state
- Branch: `feat/ui-redesign-themes`
- HEAD before handoff commit: `34e7beeb90a5a06efba0313442a5fa13ab08911f`
- Implementation is on `feat/ui-redesign-themes`, with CI fix commit `34e7beeb90a5a06efba0313442a5fa13ab08911f` pushed. `docs/state/main.md` remains untracked and untouched.

## Decisions and limitations
- Nook's asset notice identifies Lucide/Simple Icons and Nook-owned assets but does not grant rights to redistribute OpenAI or Anthropic trademarks. AUM supports user-provided local artwork and does not substitute fabricated marks.
- Pixel-level review at 100%, 125%, and 150% scaling was not reliable in the available desktop capture; those scales still need manual visual QA.
- GitHub Actions run `44809779950` passed formatting, workspace Clippy, workspace tests, Windows build, and artifact upload. Earlier runs `44807116604` and `44808553804` failed only during artifact staging; setting `working-directory: rust` fixed the path. Windows artifact ID `14666196352` is available until 2026-12-04.

## Next actions
1. Perform manual visual review at 100%, 125%, and 150% Windows scaling.
2. Integrate official provider artwork only after redistribution permission is confirmed.

## Current iteration — flyout v4, 2026-11-21
- Read `CODEX-PROMPT-AUM.md`, `docs/design/AUM-design-spec-FINAL-v4.md`, and viewed all nine files in `docs/design/references/`.
- Reworked only flyout composition/theme wiring and the theme/language selector values needed to select the three specified themes: Graphite default, Bluish Dark, Light. Language default is Ukrainian; Russian flyout strings were added alongside EN/PL.
- Aligned the flyout to 380 px with 16 px window radius, 12 px margins, 12 px card radius, 8 px card gap, 56/118/48/76 quota columns and 10 px column gaps. Added quota headings and 32 px header actions.
- Quota bars and percentages now follow the configured alert thresholds (default 25/10): remaining > warning is green, remaining <= warning and > critical is amber, and remaining <= critical is red. Per-window reset values remain sourced from existing data.
- `cargo fmt --all` completed. `cargo build -p agent-usage-monitor-ui -j 2` passed in 54.07 s. `git diff --check` passed. No tests or Clippy were run.
- Launched `rust/target/debug/agent-usage-monitor.exe --demo`; process 30088 was running at last check. Screenshot review could not be captured: both Windows UI automation clients reported native pipe unavailable (`The system cannot find the file specified`).
- No user configuration was modified intentionally; no commit or push was made. Waiting for visual review/approval before the next design stage.

## Current iteration — visual QA corrections, 2026-11-21
- Reopened and viewed all nine images in `docs/design/references/`; compared the Flyout, General Graphite, and General Light references with the current Slint implementation and v4 measurements.
- Updated only `rust/agent-monitor-ui/ui/main.slint` in this correction: added theme-specific neutral custom toggles (including notification toggles), replaced the blue/standard sidebar selection with one neutral active item and hover state, restored AUM sidebar mark/title/subtitle/version, enlarged Settings to reference proportions, grouped theme/language controls into a bordered card, and adjusted General provider cards and spacing. Flyout remains 380 px and its header actions now use custom theme-colored surfaces. Quota data logic was not changed.
- Demo mode was confirmed as an explicit launch flag (`--demo`) on PID 30088 from the prior QA step. The normal launch path sets `use_live` unless `--demo` or `--empty` is selected (`rust/agent-monitor-ui/src/main.rs:159-170`). Launched the updated build once with `--empty` (PID 13496) so it displays unavailable values without fabricating quota data or starting provider refresh.
- `cargo fmt --all --check` passed. `cargo build -p agent-usage-monitor-ui -j 2` passed after stopping the prior owned `--empty` instance that had locked the executable. `git diff --check` passed. No tests or Clippy were run per request.
- Windows screenshot capture remains unavailable because the computer-use native pipe returned “The system cannot find the file specified”; the updated runtime appearance and theme dropdown states are therefore not visually confirmed. Windows SDK `rc.exe` is still absent, so the executable icon resource was not embedded.
- Official OpenAI and Anthropic brand/trademark pages were reviewed. OpenAI says use is subject to its brand guide and provides a contact for permission requests; Anthropic lists trademark guidelines. No explicit redistribution permission for bundling the requested contrasting Codex/Claude logo variants in this MIT app was confirmed, so no provider logo assets were copied or added.
- No quota/provider integration or settings files were changed. No commit or push was made. Waiting for visual review and approval.

## Current iteration — Claude takeover, Phase 1 visual rebuild, 2026-11-21
- Took over UI implementation from Codex. Read the v4 spec and CODEX-PROMPT-AUM.md and viewed all nine references. The Settings references are 2x renders of a 720x608 client area.
- Rewrote `rust/agent-monitor-ui/ui/main.slint` around a `Theme` token global (Graphite, Bluish Dark, Light) and reusable components: ToggleSwitch (neutral, theme-specific), Card, SettingsRow, NavItem, Dropdown (custom popup), StatusChip, PlanChip, HeaderButton, SettingsButton, ProviderCard, QuotaRow, ThresholdBar, AumMark, and own line icons drawn with Path. No std widgets are used.
- Flyout: 380 wide, 15/600 header title, 32x32 icon buttons (refresh, settings, close), column headings, 116-high cards, 56/1fr/48/76 grid. Status shows a colored dot with muted relative text ("щойно" / "N хв тому") from the real last-success time. Reset times are compact ("4 год 19 хв", "2д 3г"). An unknown reset shows "—".
- Settings: 720x608, 200 px sidebar with AUM mark, three nav items, version at the bottom. General, Notifications (master switch, threshold bar and legend, warning/critical dropdowns, "Показати тест", exhausted/weekly switches, duration) and About (hero, feature list, connections with live status chips, GitHub/Issues buttons, legal note from the spec) follow the references.
- Rust: localization strings for the new rows; adapter status levels; `settings_status_level`; title bar colored through DWM (new windows-sys feature `Win32_Graphics_Dwm`, retried until the native handle exists); rendered AUM window icon; flyout close callback; test notification and issues link wired. Provider icons can now have per-theme variants (`<provider>-on-dark.svg` / `-on-light.svg`).
- Provider logos: the official OpenAI and Anthropic pages require prior permission. Placeholders were kept and documented in `docs/design/brand-assets.md`.
- Debug-only review flags: `--preview-flyout`, `--preview-settings=<0|1|2>`, `--preview-theme=<graphite|bluish|light>`. The theme override stays in memory and is never saved.
- Verified by screenshots at 100% scaling: the flyout in all three themes, and Settings General, Notifications and About in Graphite and Light. Not yet captured: Bluish Dark Settings, open dropdowns, hover states, 125/150/200% scaling.
- Checks: `cargo build -p agent-usage-monitor-ui -j 2` passes with no Rust warnings. Adapter test expectations were updated for the new strings. Tests and Clippy were not run, as requested. No commit or push.
- Not in scope: Windows autostart and refresh-interval rows. Project AGENTS.md forbids autostart without approved scope, so these need a decision.

### Next actions
1. User visual review and approval of Phase 1.
2. Decide on autostart and refresh-interval rows (blocked by AGENTS.md scope).
3. Tray menu order/theming and flyout TEMPORARY/PINNED behavior (later phase).

## Current iteration — final polish, 2026-11-21
- Flyout: the native window is clipped to a 16 px rounded region (`SetWindowRgn`, scaled by DPI, rebuilt when the size or scale changes). DWM's own corner rounding is turned off for this window. A zoomed screenshot shows no square corner artifacts. The width stays 380.
- Percentages are now 16 px, weight 600, in a fixed 48 px right-aligned column. The header's sun icon was replaced with an eight-tooth gear line icon; it still only opens Settings.
- Settings preferred height is now 680, so About fits at 100% scaling with room at the bottom. On open, the window is clamped to the monitor work area minus a 16 px margin, and the content scrolls when it is shortened.
- The provider status detail in General now wraps and the card grows to fit. Checked with long "setup required" text in UK and RU.
- Provider placeholders are now neutral "Cx"/"Cl" monogram tiles. The per-asset limitations are documented in `docs/design/brand-assets.md`. Per-theme loading of local official files is unchanged.
- Added a debug-only `--preview-language=<uk|en|pl|ru>` flag (in memory only, never saved).
- Checked with screenshots: Graphite (flyout, General, About, empty-data General), Light (flyout, About), Bluish Dark (flyout, Notifications), RU General, PL empty flyout.
- Checks: fmt and build only. No tests, no Clippy, no commit, no push.

## Current iteration — two-mode tray flyout, 2026-11-21
- New `rust/agent-monitor-ui/src/flyout.rs`: UI-thread state machine with `Hidden`, `CompactTemporary` and `FullPinned`. One `MainWindow` instance serves both presentations through the Slint `compact` property, so there are no duplicate windows and no second polling path.
- Hide timing uses deadlines checked in the existing 50 ms timer: 2 s after the cursor leaves both icon and preview, and a 5 s hold after a context-menu refresh completes (only real interaction with the preview cancels the hold). The refresh hold has a 60 s safety timeout. Animations carry a generation token, so a stale fade cannot hide a re-shown or pinned window.
- Tray (`tray.rs`): NOTIFYICON_VERSION_4. A hover is `NIN_POPUPOPEN`, or `WM_MOUSEMOVE` followed by a 400 ms rest. A single click does nothing. A double click opens the full pinned flyout. The menu is `WM_CONTEXTMENU` with exactly Відкрити / Оновити / Налаштування / Вихід, localized in UA/PL/EN/RU, and its language updates live. The shell tooltip is suppressed, since the preview replaces it.
- Focus: Slint `unstable-winit-030` attributes hook gives the frameless flyout `with_active(false)`, so winit shows it with SW_SHOWNOACTIVATE. Full flyout: draggable header (`drag_window`), always on top, remembers its position (validated against the current monitors), never hides on focus loss. Native 16 px rounded region in both presentations.
- Animations: native layered-window fade plus a 6 px slide, 180 ms show and 150 ms hide. Skipped when Windows "Animation effects" is off. The window is pre-created off-screen and transparent so the first fade cannot flash.
- `main()` now uses `slint::run_event_loop_until_quit()`. The previous `window.run()` showed the flyout at startup. The worker reports completion of requested refreshes (`refresh-finished`). Without a live worker, the menu refresh holds for 5 s directly.
- Sizes: full 440x322, compact 440x220 (two providers). Percentages are 14 px semibold in a fixed 48 px column, and the reset column is 84 px.
- Badges: one rule for both providers (`short_plan`): shown only for a real `plan_type`, normalized ("plus" becomes "Plus"), and hidden for empty or unknown values. Only Codex reports a tier today; Claude's status line and `/usage` parsers set `plan_type: None`, so the Claude badge stays hidden.
- Logos: still unresolved. Both owners require permission. The cards show the provider name only, with no invented mark. Sources and the permission path are in `docs/design/brand-assets.md`.
- Verified with real synthetic input on this machine (the AUM icon is in the Windows 11 overflow panel): hover shows the compact preview unfocused; leaving hides it after about 2.3 s; a single click produces no event; a double click and menu Open give the full flyout, still pinned after 3 s away; hover while pinned is ignored; × hides it; menu Refresh holds 4 s and hides by 6.2 s; menu Exit quits. Screenshots checked: Graphite, Light and Bluish Dark, both presentations.
- Checks: fmt and build only. No tests, no Clippy, no commit.

## Current iteration — provider integration and tray audit, 2026-11-21
- Kept all pre-existing worktree changes. Audited Claude StatusLine → normalized snapshot → `ClaudeUsageSnapshot` merge → provider/UI adapter → Slint, plus `/usage` parsing and provider state. Official Claude Code docs define both reset fields as epoch seconds, but expose no exact subscription-tier field. The currently saved AUM setting has opt-in CLI refresh enabled.
- Ran the configured `claude -p --no-session-persistence /usage` source once with output filtered to field-presence diagnostics; both 5-hour and weekly reset text and `Europe/Warsaw` timezone were present. On the live AUM run, `CLAUDE_RESET` reported `source_text=true parsed=true` for both windows. Found and fixed reset cache loss: a newer reset-less StatusLine window now updates its percentage but retains the last known reset timestamp. Expired timestamps display unavailable (`—`) rather than a negative countdown. Added UI and Core regression coverage.
- Claude exact plan remains unavailable: documented StatusLine metadata has usage/reset fields and subscriber eligibility only, not Pro/Max/Ultra account identity. Both provider badges use the existing shared missing-value rule; no tier is inferred and no manual selector was added.
- Connected the existing tray renderer to per-provider state. The left Codex and right Claude bars independently use the minimum available 5-hour/weekly remaining percentage and existing 25/10 thresholds; unknown/disabled providers are neutral independently, and stale data uses a muted segmented bar. A keyed one-second local update sends a transparent 16×16 BGRA icon through `CreateIconIndirect` and `Shell_NotifyIconW(NIM_MODIFY)`; successful Windows shell updates emitted only `TRAY_ICON` status, without quota values.
- Four controlled cases (96/49, 20/80, unknown/65, both unknown) passed at 16×16. Generated 16/20/24/32 native-size and 4× nearest-neighbor previews for dark and light backgrounds under `%TEMP%\AUM-icon-review\`; visual inspection showed distinguishable heights and independent neutral states. Actual notification-area appearance could not be captured through the available desktop window inventory.
- Extracted provider SVG selection into a directory loader and tested temporary generic, non-provider assets for dark/light variants, single-file fallback, and missing files. Existing startup cache intentionally loads both themes once; no official marks are bundled. The expected per-user provider-icons directory is currently absent. Official [OpenAI brand guidelines](https://openai.com/brand/) and [Anthropic trademark guidelines](https://www.anthropic.com/legal/trademark-guidelines) require permission/approval; exact redistribute permission remains unconfirmed. User-supplied paths remain documented in `docs/provider-icons.md`.
- Source audit found the flyout has Slint `no-frame: true`; native helper only changes taskbar extended styles, not caption styles. No overlay or flyout geometry changes were made. CUA could not expose the hidden tray flyout for screenshot/reproduction, so live artifact and interaction checks remain unverified in this iteration. Prior manual tray-interaction results above remain the only direct behavior evidence.
- `cargo fmt --all --check`, targeted reset/merge/tray/SVG tests, and `cargo build -p agent-usage-monitor-ui -j 2` passed. Windows SDK `rc.exe` remains unavailable, so the EXE icon resource warning persists. No full workspace tests, Clippy, commit, or push.
- Final LIVE launch remains in the notification area (PID 14504) with `--enable-claude-bridge`; its temporary diagnostics report both `/usage` reset fields parsed and successful Windows icon updates. No UI screenshot was available.

## Current iteration — cardless Variant A flyout, 2026-11-21
- Reference: `docs/design/references/Головне вікно без карток · варіанти А і Б@2x.png`, top row (Variant A) only.
- `ui/main.slint`: `ProviderCard` → `ProviderSection` (no background/border/radius); one shared flyout surface; one 1 px `Theme.flyout-divider` line between Codex and Claude (only when both are enabled); shared column grid (label | bar | 48 px percent 14 px semibold | reset) with headings right-aligned to the percent/reset columns. Full 440×297, compact 440×204 (both wider than tall). Settings untouched.
- `src/flyout.rs`: fixed the white native caption artifact. Cause: winit keeps `WS_CAPTION|WS_SYSMENU` on undecorated windows and forwards `WM_NCACTIVATE` to `DefWindowProc`, so Windows painted the tool-window caption on activation/deactivation and its × on click/drag. Fix: `install_caption_guard` strips the caption style bits and subclasses the flyout (`WM_NCACTIVATE` with `lParam=-1`, `WM_NCPAINT` swallowed). Verified by synthetic click, drag (window moved), active and deactivated states: no artifact.
- Six real-data screenshots (Graphite/Bluish Dark/Light × compact/full) were taken; settings.json was backed up and restored.
- Not fixed (out of scope, provider polling): worker threads call `slint::Weak::upgrade()`, which returns `None` off the UI thread, so the flyout only receives new data from the 60 s countdown timer (tray icon updates every 1 s). Visible as "Завантаження/Недоступно" for up to 60 s after start and Claude reset "—" until the next tick after the CLI result.
- Tray icon (real, 16 px, dark taskbar): two green bars with proportional heights (Codex 93% ≈ 11 px, Claude 36% ≈ 4 px). Deviation from IMG_9330: no grey track behind the fill (unfilled part is transparent). Reported, not changed.
- Checks: `cargo fmt --all --check`, `cargo build -p agent-usage-monitor-ui -j 2`. No tests, no Clippy, no commit. Single instance relaunched with `--enable-claude-bridge`.

## Current iteration — consolidated stabilization, 2026-11-21
- Fixed live worker-to-UI delivery: Codex, Claude StatusLine, `/usage`, and pipe-error snapshots now enter Slint through `Weak::upgrade_in_event_loop`. The UI callback reads the authoritative shared snapshot at execution time, so queued older snapshots cannot overwrite newer cached state. Hover/Open/Refresh apply the latest cache before presenting the flyout. Added separate safe timing diagnostics for provider response and UI propagation; LIVE observed Codex response at 2,749 ms and snapshot-to-event-loop at 0 ms (an earlier run measured 2,473 ms / 2 ms).
- LIVE `/usage` exposed a new hour-rounded reset string (`5pm`); the parser previously accepted only minute-precision times. Added hour-only parsing and regression coverage. Latest live diagnostics report `source_text=true parsed=true` for both five-hour and weekly reset values. Existing timestamp merge still preserves known resets across reset-less newer updates and rejects stale events. The upstream StatusLine schema intentionally excludes session/account identity, so AUM cannot reliably detect an account switch without unsupported/private metadata; cross-account invalidation remains UNVERIFIED.
- Applied the approved tier fallback: the adapter now always supplies empty plan labels for both providers while preserving Core metadata/parser behavior. Targeted test confirms both compact/full data bindings receive no badge.
- Tray renderer now paints a full-height neutral gray track behind each bar; unknown values use a neutral full rail and valid fills rise from the bottom. Focused renderer tests pass for all four cases, 16/20/24/32 sizes and light/dark review backgrounds. Preview files are under `%TEMP%\AUM-icon-review\` (including `case-a-16-dark.bmp`). Windows Shell accepted the live 16×16 BGRA replacement, but the tray itself was not visually inspected this round.
- Added an explicit documented `HWND_NOTOPMOST` transition for the full pinned flyout after show. Build succeeds; CUA could not complete the live interaction because its input channel reported detected user input. Actual z-order, drag persistence, white-caption regression, themes, context menu, notification behavior, and tray interactions remain UNVERIFIED this round.
- The native context menu is still a classic `HMENU`. Microsoft documents menu creation and background configuration but no simple per-menu dark-theme switch; custom owner-drawn/themed menu work was not introduced because it would add keyboard/accessibility/focus complexity. Theme-aware menu remains PARTIAL. (Microsoft Learn: [MENUINFO](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-menuinfo), [Win32 theme guidance](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/apply-windows-themes).)
- Native notifications still use `Shell_NotifyIconW(NIF_INFO)`. Localization tables now visibly contain test copy for UA/PL/EN/RU, and both test and real alerts ultimately call the same native notifier, but actual toast/Notification Center identity and icon were not verified or fixed. The supported branded-toast path requires app identity/shortcut setup beyond the current tray-balloon implementation; no shortcut/installer behavior was added. `rc.exe`, `llvm-rc.exe`, and `windres.exe` are absent, so the static EXE icon resource is still not embedded; the build emits a warning rather than silently claiming success. Generic app identity/icon and blue system information glyph remain BLOCKED.
- Provider marks are still BLOCKED: no official OpenAI/Anthropic assets are bundled, the documented user-supplied SVG directory is absent, and no recognizable provider marks were visually confirmed.
- Focused checks: Core `claude_usage::tests::` 6/6; UI `tray::tests::` 4/4, `adapter::tests::` 14/14, `provider_icons::tests::` 2/2; `cargo fmt --all --check`, `cargo build -p agent-usage-monitor-ui -j 2`, and `git diff --check` passed. No full workspace tests, Clippy, commit, or push. The LIVE instance is PID 32084 with `--live --enable-claude-bridge`; logs are in `%TEMP%\aum-stabilization-live-20261121-final.err.log` and `.log`.
- No live screenshot was captured. Stop here for user visual review of the actual running flyout and tray icon before treating visual/interaction requirements as complete.

## Current iteration — Claude manual refresh follow-up, 2026-11-21
- Root cause: the Refresh dispatcher sent only to Codex's worker channel. Claude's opt-in CLI scheduler ran independently on its 5-minute cadence, with a 10-minute cooldown after failures, and had no manual-request input. The saved settings were checked by reading only booleans: Codex enabled, Claude enabled, CLI refresh opt-in enabled, UI language Ukrainian.
- Added a bounded manual refresh channel for each provider. One user request fans out to both enabled providers; Claude is skipped with an explicit safe reason when disabled or when its CLI opt-in is off. Full channels coalesce repeat clicks, and the single Claude scheduler serializes requests so clicks cannot start concurrent CLI processes. Manual requests wake the scheduler during its normal success interval or failure cooldown. Background intervals remain unchanged.
- Claude refresh now emits safe lifecycle diagnostics for requested/queued/coalesced/skipped, started, CLI completion or timeout, parsing duration, completed/failed, cache update, and UI update. It logs durations and event IDs only; authenticated stdout/stderr and quota values are not logged. CLI execution and parsing are timed separately; cache and event-loop propagation are measured separately. Failures publish an error state while retaining last-good quota and reset values, so refresh attempt time does not replace last-success freshness.
- A CLI snapshot is timestamped at request start instead of process completion. That prevents a slow CLI result from appearing newer than a StatusLine event observed while the process was running. The existing independent-window merge still retains reset fields omitted by partial updates.
- Focused tests: Claude refresher `7/7` (including manual refresh bypassing failure cooldown, success/failure, timeout, and opt-in gating); bounded dispatch coalescing `1/1`; failed Claude refresh cache/freshness `1/1`; Claude reset core `6/6`. `cargo fmt --all --check`, `cargo build -p agent-usage-monitor-ui -j 2`, and `git diff --check` passed.
- LIVE startup (PID 25356) confirmed a fresh Codex response (2,365 ms) and Claude CLI success (9,710 ms), parse 0 ms, cache update 0 ms, UI propagation 0 ms; both reset fields parsed. The LIVE process remains running with the repository build and configured `--live --enable-claude-bridge`.
- Actual repeated manual button clicks could not be completed: the CUA inventory showed live user interaction (Start search overlay/input) and did not expose an AUM window. Therefore the dispatcher fix is covered by focused tests, but real manual clicks and their event IDs remain UNVERIFIED. Do not mark the issue visually accepted until the user inspects/tests the running flyout.

## Current iteration — corner rendering and final live handoff, 2026-11-21
- Flyout corner root cause was explicit `DWMWCP_DONOTROUND` plus a binary `CreateRoundRectRgn`/`SetWindowRgn` clip. Removed the GDI region and now request documented `DWMWCP_ROUND`, retaining Slint's transparent 16 px surface radius as fallback. The UI crate builds; actual edge quality is UNVERIFIED because no screenshot could be captured this pass.
- Rechecked modern notifications: Microsoft recommends Windows App SDK `AppNotificationManager` for new unpackaged Win32 apps; the repo currently calls legacy `Shell_NotifyIconW(NIF_INFO)`. Rust's existing `windows` crate includes the older WinRT `Windows.UI.Notifications.ToastNotificationManager` bindings, but not Windows App SDK App Notifications. The latter requires a Windows App SDK integration/bootstrap and per-user app identity/shortcut setup, and is not implemented here. Current notification branding remains BLOCKED; no false visual PASS is claimed.
- Started latest repository build in LIVE mode with `--live --enable-claude-bridge`, PID 32036. Safe diagnostics: Codex response 2,275 ms; Claude CLI 8,459 ms; parse 0 ms; cache update 0 ms; UI event-loop propagation 0 ms; both `/usage` reset fields parsed. These are startup/background observations, not evidence of manual button clicks.
- User interaction was visible in the Windows UI automation surface, so no repeated manual Refresh clicks, notification Test click, or visual screenshot was performed. Those checks remain UNVERIFIED. Current executable: `rust/target/debug/agent-usage-monitor.exe`; logs: `%TEMP%/aum-stabilization-final-20261121.err.log` and `%TEMP%/aum-stabilization-final-20261121.log`.
- Focused tests from the immediately preceding refresh pass remain green: Claude refresher 7/7, dispatch coalescing 1/1, adapter failure/freshness 1/1, Core reset parsing/merge 6/6. After the corner update, `cargo fmt --all --check`, `cargo build -p agent-usage-monitor-ui -j 2`, and `git diff --check` passed. No full workspace tests, Clippy, commit, or push.
- Stop point: leave PID 32036 running for the user's visual review and acceptance; do not call tray, flyout, or notification items visually verified without that review.

## Current iteration — About polish, provider logos, notification branding, 2026-11-21
- About (`ui/main.slint` only): GitHub and Report Issue now sit in one row as two equal-width 32 px buttons, 12 px apart, spanning the content column. The trademark disclaimer is centered below them (max 420 px, 12 px muted, centered lines). The copyright line is anchored to the bottom of the content area with a 1 px divider; the About page fills the viewport via `min-height`, a stretch spacer keeps at least 20 px, and the Flickable scrolls when the window is short. The sidebar version label is hidden on About only, so only the chip next to the product name shows the version. Checked by LIVE screenshots: Graphite, Bluish Dark, Light (UA) and PL/EN/RU (Graphite).
- Static AUM icon: new dependency-free `src/aum_icon.rs` renders the spec v4 application mark (tile + two bars; 16 px is bars only). It is shared by `build.rs` and the runtime (`theme::aum_icon_image`). `build.rs` now writes the Win32 `.res` file itself (RT_ICON frames 16/20/24/32/40/48/64/256 + RT_GROUP_ICON 1) and passes it to the MSVC linker, which converts it with its bundled `cvtres`. `rc.exe` is no longer needed and the build warning is gone. Verified: Explorer's associated icon for the EXE is the AUM tile; the tray fallback `LoadIconW(hinst, 1)` now resolves the embedded resource. The old blue two-column build icon is gone.
- Notifications: new `src/notifications.rs`. On start, AUM sets the process AUMID `Valenticus.AgentUsageMonitor`, writes `%APPDATA%\Agent Usage Monitor\notification-icon.png` (the static mark, 256 px), and registers `HKCU\Software\Classes\AppUserModelId\Valenticus.AgentUsageMonitor` (`DisplayName=AgentUsageMonitor`, `IconUri`). This is the per-user registration that Windows App SDK / Community Toolkit use for unpackaged apps. There is no shortcut, installer step or autostart. Remove with `reg delete HKCU\Software\Classes\AppUserModelId\Valenticus.AgentUsageMonitor /f`. `TrayNotifier::notify` (used by real alerts and Settings → Test) now shows a WinRT `ToastNotificationManager` toast (`windows` 0.62.2, already in the lock as a transitive dependency): ToastGeneric title/body, AUM `appLogoOverride`, Tag (FNV hash of the title) + Group `aum` to replace repeats, short/long duration. If the toast fails, the error is logged (`NOTIFY: toast failed`) and the legacy balloon is used. Windows App SDK `AppNotificationManager` was not used: it needs the Windows App Runtime plus a bootstrapper and has no Rust projection in the workspace.
- Test titles no longer repeat the app name, since the header now shows it: «Тестове сповіщення» / «Powiadomienie testowe» / «Test notification» / «Тестовое уведомление».
- LIVE verification: clicked the real Settings → Notifications → «Показати тест» button in UA/PL/EN/RU. The log shows `NOTIFY: toast delivered` for each. Notification Center shows the group "AgentUsageMonitor" with the small AUM icon, the large AUM logo in the toast, and the localized title/body. There is no EXE name and no blue info glyph. A real quota alert ("Claude · 5 год · попередження / Залишилося 18%") appeared through the same pipeline and identity. Pop-up banners did not appear during the test; Windows Do Not Disturb appeared active, which is respected behavior, so banner appearance remains UNVERIFIED. The Test handler does not touch the alert engine. `alert-state.json` changed once during the UA run because of the real alert engine (the Claude warning above), not because of Test.
- Provider logos: still BLOCKED. The loader matches the docs (`%APPDATA%\Agent Usage Monitor\provider-icons\<provider>-on-{dark,light}.svg`, fallback `<provider>.svg`), and the folder does not exist. The Anthropic press kit is directly downloadable, but its trademark guidelines require prior approval. OpenAI's logo ZIP requires agreeing to its Marks usage terms. I did not accept terms on the user's behalf or install any mark. Exact files and placement are in `docs/design/brand-assets.md`.
- Regression: LIVE pinned flyout after a header click shows no white caption, rounded corners, unchanged Variant A layout and real data. The pinned flyout is intentionally non-topmost (earlier `HWND_NOTOPMOST` change), so a foreground app such as VLC can cover it until it is activated. That is existing behavior and was not changed.
- Checks: `cargo fmt --all --check`, `cargo build -p agent-usage-monitor-ui -j 2` (no warnings) and `git diff --check` passed. Focused tests `notifications localization provider_icons adapter` passed: 23/23. No full workspace tests, no Clippy, no commit, no push. Final single LIVE instance: PID 3472 `--live --enable-claude-bridge`; logs at `%TEMP%\aum-branding-live-20261121.err.log`.

## Current iteration — local provider icons (PNG), 2026-11-21
- The user placed five official files in `%APPDATA%\Agent Usage Monitor\provider-icons\`. All decode as 32-bit ARGB PNGs:
  - `Claude Spark - Clay.png` 937², almost no margin;
  - `ClaudeIcon-Rounded.png` and `ClaudeIcon-Square.png` 1280² (app tiles, not used);
  - `OAI_OpenAI-Blossom_Black.png` and `_White.png` 716² with a transparent 180 px margin, so the content is 50 % of the edge.

  The originals' SHA-256 hashes are unchanged after this work.
- Generated locally, not in Git: `claude-on-dark.png` (#FFFFFF) and `claude-on-light.png` (#111111). Both use the exact Spark alpha mask, with only RGB replaced. A sampled alpha comparison against the source found 0 differences.
- `src/provider_icons.rs` was rewritten:
  - PNG and SVG load with a deterministic per-theme candidate order (see `docs/provider-icons.md`); `ClaudeIcon-*` is never a candidate.
  - Each file is decoded once per run. PNG margins are trimmed into a centred square master of at most 256 px. Area-average downscaling uses premultiplied alpha, at exactly size × scale device px, cached per (provider, theme, px).
  - Missing, corrupted, oversized and blank files fall through to the next candidate.
- `adapter.rs` and `main.rs` request 20 px marks for the flyout and 16 px for the Settings card at the window scale factor. In `main.slint`, `ProviderMark` gained a `size` property, set to 20 px in `ProviderSection`. Variant A geometry, percentages, bars and countdowns are unchanged. `Cargo.toml` adds `png` as a dev-dependency for test fixtures.
- LIVE verification (`--live --enable-claude-bridge`, real data):
  - Graphite, Bluish Dark and Light, compact and full: white Blossom and white Spark on the dark themes, black on Light, the same 20×20 visible size, centred against the provider name, no tile, clipping or stretching.
  - An 8× zoom at 100 % shows clean antialiasing.
  - Switching the theme in Settings → General in one process (Graphite → Bluish Dark → Graphite → Light → Graphite) switched the flyout icons immediately.
  - 125/150/200 % scaling is covered by the exact-size test, but not visually checked on screen.
- Checks:
  - `cargo fmt --all --check` and `git diff --check` passed.
  - `cargo build -p agent-usage-monitor-ui -j 2` passed with no warnings.
  - Focused tests `provider_icons adapter`: 22/22. They cover PNG trimming and exact scaling, SVG compatibility, theme selection with original names, theme PNG priority, missing/corrupted/oversized/blank files, cache, and alpha-correct resampling.
  - No full test suite or Clippy was run, and nothing was committed or pushed. `settings.json` is back to Graphite. Final LIVE instance: PID 22424 (`%TEMP%\aum-icons-live-20261121.err.log`).

## Current iteration — 14 px provider icons, flyout border and corners, 2026-11-21
- Icons: the flyout requested 20 px marks (`adapter.rs`) inside a 20 px `ProviderMark` (`main.slint`, `ProviderSection`). Both are now 14 logical px. The cache, DPI-exact resampling, PNG/SVG support and live theme switching are unchanged. The icon is still vertically centred in the name row with 8 px spacing. Settings card icons remain 16 px.
- Border and corner root cause, confirmed by 8× corner crops of the live window before the change (combination E):
  - (A) The Slint `Window` background was `transparent`. The software renderer presents through softbuffer/GDI without per-pixel alpha, so transparent pixels became dark.
  - (B) Slint drew a 16 px radius inside DWM's fixed `DWMWCP_ROUND` clip of about 8 px, which left dark wedges between the arcs.
  - (C) The Slint root `Rectangle` had a 1 px `Theme.border` stroke in addition to DWM's own window border, which made a double outline.
- Fix (documented APIs only):
  - `main.slint`: the `MainWindow` background is the opaque `Theme.window-bg`, and the root `Rectangle` is full-bleed with no radius and no border.
  - `flyout.rs` (`apply_native_corners`): keeps `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND` and adds `DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE`.
  - No GDI region, no overlay, and DWM composition is not disabled. The DWM shadow is kept.
- Limitation: the corner radius is now Windows 11's system radius (about 8 px), not the earlier 16 px. A custom 16 px antialiased radius would need per-pixel window alpha (a GPU renderer with a transparent swap chain, or `UpdateLayeredWindow`), which the current software renderer does not provide. On Windows 10, where there is no DWM rounding, corners are square.
- LIVE verification (`--live --enable-claude-bridge`, real data):
  - Graphite, Bluish Dark and Light, compact and full: clean antialiased corners, no dark wedges, no outline, no white caption.
  - Codex and Claude icons are both 14 px and both visible.
  - On the real tray icon: double-click opened the pinned flyout, which stayed open after 3 s; header drag moved it by (−250, −200); after focus loss it stayed visible with clean corners and no caption; × closed it.
  - Unverified: hover preview from synthetic cursor moves (not triggered), a second double-click reopen (delayed and ambiguous), and position memory after reopen (it reopened at the default tray position). Testing stopped because the user was interacting with AUM at the same time (`settings.json` was switched to Light at 15:11:26 by the user, and that choice was left unchanged).
- Checks: `cargo fmt --all --check`, `cargo build -p agent-usage-monitor-ui -j 2` and `git diff --check` passed. Focused tests `provider_icons adapter` passed 22/22. No full workspace tests, no Clippy, no commit, no push. LIVE instance: PID 25060 (`%TEMP%\aum-corners-live-20261121.err.log`). No provider icon files are in Git.

## Current iteration — UI/UX polish, custom tray menu, 0.1.0 release candidate, 2026-11-21
- Safety: branch `feat/ui-redesign-themes` (as expected); all earlier uncommitted work preserved; versions 0.1.0 in both crates. No commit, push, release, install or uninstall.
- Flyout (`ui/main.slint`):
  - The title "Ліміти агентів" and the "ЗАЛИШОК/СКИДАННЯ" headings are no longer drawn (UA/PL/EN/RU); the title is kept as the window's accessible name.
  - Full flyout: 28 px Refresh/Settings/Close at the top right, glyphs ending on the 16 px content edge; the drag region is the rest of that row. Compact flyout starts with Codex and has no buttons.
  - One shared `FlyoutGrid` for both modes, 16/16 side padding: label 56 | 10 | bar | 12 | percent 40 | 16 | reset 78. Status and countdowns are right-aligned on the content edge; the divider sits on the grid. Provider name 14/600, labels 12, percent 13/500, reset/status 12.
  - Sizes: full 440×243, compact 440×185.
- Bar alignment (measured on screen): Slint centres the cap height of the percentage on the row centre. The bar is centred on the row on a whole pixel, so bar and percentage share midpoint 98 in a row spanning 84..112. A font-metric offset was tried, made the bar 1 px too low, and was removed. Lower-case labels and countdowns sit about 1–1.5 px lower visually and cannot share a pixel-exact axis with cap-height digits.
- Light theme tokens: text primary #171717, secondary #424242, status/reset #525252, disabled #737373 (new `text-disabled`), positive #167C52, dividers #DADADA. Warning and critical colours are unchanged.
- Typography:
  - New exported `Typography` global used by all windows; default "Segoe UI Variable Text".
  - Verified by pixel comparison that it resolves: it differs from the fallback, while "Segoe UI" renders identically to an unknown family, since Segoe UI is the system fallback.
  - The Slint 1.19 software renderer uses fontique + swash with variable-axis coordinates (real 400/500/600), but no hinting, grayscale AA and quarter-pixel glyph positions. Small text is therefore softer than native ClearType. This is a renderer limitation and was not changed.
  - Navigation 13 px, active 500 (was 600). Page heading 22/600; section headings 12/600, 0.3 px tracking, secondary colour; body 13, descriptions 12.
- Icons:
  - General is now a two-track sliders icon; the old three-track glyph merged into a "crossed Z" at 16 px.
  - All line icons use one 1.5 px stroke. New `open` and `power` icons for the menu.
  - Provider marks, the AUM app icon and the tray indicator are unchanged.
- Settings:
  - Switches use the restrained AUM accent when on, with a white knob; the knob also moves and grows.
  - The Claude source note now sits inside the Claude card with a 14 px info icon on the 16 px content edge.
  - Default size is 720×640 (was 680 tall); About spacing was tightened without removing content or shrinking fonts.
  - `place_settings` opens at the default size, clamped to the work area including the title bar, and runs again 150 ms after show with the final scale factor. `min-height` is now 420.
- About verification, default size, no scrolling, copyright at the bottom:
  - 100 %: Graphite, Bluish Dark and Light (UA), and PL/EN/RU in Graphite.
  - `SLINT_SCALE_FACTOR` emulation: 1.25 and 1.5 fit. At 2.0 on 1080p only 516 logical px exist, so the window is clamped and the page scrolls (allowed).
  - Real Windows 125/150/200 % scaling was not switched: UNVERIFIED.
- Custom tray menu (new `TrayMenu` Slint window, `src/tray_menu.rs`; replaces the white native `HMENU`, `show_tray_menu` removed):
  - Items: Open, Refresh limits, Settings, divider, Exit; localized UA/PL/EN/RU.
  - Window: 232×145, theme tokens (#202020/#333333, #192331/#29384A, #FAFAFA/#ECECEC), opaque, DWM `DWMWCP_ROUND`, DWM border off, tool window, caption guard.
  - Placement: above and right of the anchor, flipped and clamped into the monitor work area. Unit-tested, including a top taskbar and negative multi-monitor coordinates.
  - Keyboard: Up/Down/Home/End/Enter/Space/Escape. Closes on `focus-lost(window-activation)`.
  - Behaviour: one reused instance; a temporary preview is dismissed when it opens; hover is ignored while it is open.
  - `TrayEvent::ContextMenu{x,y}` carries the v4 anchor. The winit hook now keeps only the flyout inactive. One shared dispatcher runs tray events and menu commands; the menu closes first.
- Live tray interaction (debug build, real input on the real tray icon):
  - Hover: the compact preview opened in about 0.5 s and hid about 2.2 s after leaving.
  - Clicks: a single click did not open the flyout. A double click opened the pinned flyout, which stayed after 3 s and after focus loss.
  - Drag moved it by (−200, −150). × closed it; it reopened at the dragged position. One window, one process.
  - Menu: right-click opened it. Down/Up/Escape and outside activation worked. Enter on Settings opened Settings; Open opened the pinned flyout; Refresh logged `REFRESH: requested id=1` for both providers.
  - Settings → Notifications → Test delivered a toast ("Тестове сповіщення" in the Notification Center). No white caption or corner artifacts.
- Position policy: the pinned position is remembered in memory for the session (validated against monitors) and resets to the tray anchor after restart. Recommendation: keep it. Not changed.
- Release readiness fixes:
  - `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`: the release build opened a console window.
  - `CREATE_NO_WINDOW` for the Codex app-server launch and taskkill in `agent-monitor-core/src/codex.rs`, so the GUI build does not flash consoles. The Claude refresher already had it.
  - Clippy `-D warnings` had been failing CI. Fixed mechanically without behaviour change: `?` in the Claude hour parser, two type aliases in `tray.rs`, `&info` for `CreateDIBSection`, and removed `drop()` on non-guard values.
- Release check (x86_64-pc-windows-msvc release):
  - Binary: PE32+ x64, GUI subsystem, embedded static AUM icon.
  - Run: started LIVE with real Codex/Claude data; child cmd/claude/codex/conhost had no visible windows. The compact preview, the double-click pinned flyout and menu Exit worked; the process ended and the tray icon was removed.
  - Windows placed the new release EXE's tray icon in the overflow area. A synthetic click sequence dragged it into the visible tray during testing, changing the user's tray layout; this is reported to the user.
- Packaging:
  - `packaging/windows/AgentUsageMonitor.iss`: Inno Setup 6, per-user `{autopf}` = `%LOCALAPPDATA%\Programs`, no admin.
  - Shortcuts: Start menu shortcut with AUMID `Valenticus.AgentUsageMonitor`; optional desktop shortcut.
  - Claude bridge: opt-in unchecked task adds `--enable-claude-bridge`; the bridge EXE is installed.
  - Install behaviour: Restart Manager plus `AppMutex`; UA/EN/PL/RU wizard.
  - Uninstall: removes the HKCU AUMID key and the generated notification PNG. Settings are deleted only after an explicit Yes; `provider-icons` is never touched.
  - No autostart, updates or telemetry.
  - `packaging/windows/build-installer.ps1`: builds release, stages, guards against provider artwork, writes a portable ZIP and runs ISCC if present.
  - Installer `.exe`: BLOCKED, Inno Setup is not installed (`winget install --id JRSoftware.InnoSetup -e`). The portable ZIP was built: `rust/target/installer/AgentUsageMonitor-0.1.0-win-x64-portable.zip`.
- README: updated stale feature and limitation lines; added a release-candidate section.
- Checks: `cargo fmt --all --check`, debug build, `cargo clippy --workspace -- -D warnings` and the release build passed. `cargo test --workspace`: core 31, bridge 3, bridge process integration 1, UI 59, all passed.

### Follow-up — row centrelines, grey switches, warning yellow, nav icons (2026-11-21)
- Flyout rows:
  - Root cause: shapes (icon, dot, bar) were centred on the row box. Slint centres text on its ascent + descent line box, so mixed-case text sits about 1.5 px lower and digit-only text about 0.5 px lower. Measured spread per row was 1.5 px.
  - Fix: each row is now one `HorizontalLayout`. Header: mark, 8, name, [badge], stretch, dot, 6, status. Quota row: label, 10, bar, 12, percent, 16, countdown. All children are vertically centred.
  - The shared axis is the optical centre of mixed-case text, `((ascent − descent)/2 − (capH + xH)/4)/upm = 233.5/2048 em` below the box centre. `AxisSlot` moves shapes onto it, and digits by `(capH − xH)/4`.
  - Offsets are rounded to whole px: quota 1 px; header 1 px (mean of the 14/12 px fonts); digits 1 px.
  - Result by ink measurement in all themes and both modes: header elements within ±0.5 px of one axis (spread 1.0); quota rows spread 0.5 px. This is the 100 % pixel quantum: glyph bounds are whole pixels, and crisp shapes sit on whole pixels.
  - Overlays: `axis-*.png`. A 125 % emulation capture was taken (`SLINT_SCALE_FACTOR`).
- Switches:
  - Regression came from the previous iteration, which set `toggle-on-track: accent`.
  - Now neutral tokens: `toggle-on/off-track(-hover)`, `toggle-off-border`, `toggle-knob` #FFFFFF.
  - Measured: Graphite and Bluish ON #737373 / OFF #474747; Light ON #5C5C5C / OFF #D4D4D4 with a #ABABAB outline. White knob in both states.
  - OFF was captured by real clicks on "Тижневі ліміти" in all themes; `settings.json` was restored byte-for-byte.
- Warning colour:
  - `Theme.warning-yellow` #FACC15 is used for bars, legend dots and chips; small warning text on Light uses #A16207. Legend dots now use the segment (bar) colour.
  - Tray: `TRAY_WARNING` = [250, 204, 21]; `TRAY_NORMAL` and `TRAY_CRITICAL` are unchanged and boundaries are unchanged (test). The renderer fixture (20 % at 16 px) shows the yellow bar.
  - Real warning state in the live tray: UNVERIFIED, because current quotas are not in the warning range and no fake data was injected.
- Legend: order is now critical, warning, normal (matches the scale), laid out space-between under the scale.
  - Text is built by `threshold_legend()` from the configured thresholds.
  - Wording follows the domain rule: normal is strictly above warning ("Норма понад / Norma powyżej / Normal above / Норма выше"); EN critical reads "Critical up to".
  - Checked in UA/PL/EN/RU.
- Navigation icons: new `NavIcons` on a 16×16 grid: sliders-horizontal, bell, info. They are own drawings, so no third-party asset or licence is involved.
  - Root cause of blur: the 24-grid paths drawn at 16 px put strokes on fractional pixels (for example y = 5.33). Every straight stroke now runs through a pixel centre (x.5), with a 1.5 px stroke.
  - Same geometry, stroke and colour in active and inactive states.
  - Verified at 1:1 and 6× in three themes and three selected states.
- Checks: fmt, debug build, `cargo clippy --workspace -D warnings`, and focused tests (legend, tray incl. new yellow/boundary test, menu, localization, adapter, provider icons: 36/36) passed. `git diff --check` passed.

### Follow-up — threshold legend layout (2026-11-21)
- Root cause: the legend was one `space-between` row of non-wrapping labels with 18 px spacing and a fixed 62 px block. A non-wrapping text's minimum width is its full width, so when the three labels plus spacing exceeded the 423 px scale width, the row ran past the right edge. UA was about 429 px (almost touching), and RU was wider (crossing the edge).
- Fix in `ui/main.slint` (`ThresholdBar`, new `LegendItem`):
  - The entries are sized by their measured text, spread over exactly the scale width (`space-between`, 12 px minimum gap), so the first starts at the scale's left edge and the last ends at its right edge, 16 px inside the card.
  - Labels may wrap between words. Only the middle (warning) entry shrinks (`horizontal-stretch: 1`, outer entries 0). The dot stays on the first text line, and the block height follows the legend.
  - `threshold_legend()` puts U+2060 WORD JOINER around the range dash, so "10–25%" never breaks.
  - Font size, Settings size, scale direction, the #FACC15 yellow, thresholds and wording are unchanged.
- Verified (screenshots `shots9`): UA, PL and EN fit on one line; RU wraps only "Предупреждение / 10–25%". No clipping, overlap or overflow in Graphite, Bluish Dark or Light, nor with 1.25 scale emulation.
- Checks: fmt, build, `clippy --workspace -D warnings`, focused tests (`threshold_legend` incl. the no-break assertion, tray, localization: 15/15), `git diff --check`.

### v0.1.0 release preparation — embedded provider marks (2026-11-21)
- Audit: the only provider-icon path was `provider_icons::local()`, used by `adapter::apply_config` (flyout, 14 px) and `update_settings_window` (Settings cards, 16 px). It read `%APPDATA%\Agent Usage Monitor\provider-icons` only.
- Bundling:
  - `build.rs` embeds the known candidate PNGs from the git-ignored `rust/agent-monitor-ui/assets/provider-icons/` folder (override: `AUM_PROVIDER_ICONS_DIR`) via a generated `OUT_DIR/bundled_provider_icons.rs` (`include_bytes!`). A missing folder only produces a build warning.
  - Runtime: per slot, embedded files win; the user folder is only a fallback for slots without an embedded file. A startup log line `PROVIDER_ICONS: <slot> source=Bundled|LocalFolder` records the source.
  - Embedded PNGs are decoded with the `png` crate (now a regular dependency), passed through Slint's own premultiply/unpremultiply (`from_rgba8` → `to_rgba8_premultiplied` → `from_rgba8_premultiplied` → `to_rgba8`), then the same trim and area-resampling pipeline.
  - Test `bundled_assets_match_file_rendering_and_need_no_local_folder`: rendered icons are bit-identical to the file path at 14/16/18/20/21/28/32 px, and all four slots load with an empty local folder.
- Assets: copies of the four approved files (OpenAI Blossom white/black; Claude Spark white/#111111 variants). Hashes are identical to the `%APPDATA%` originals, which were left untouched. `.gitignore` ignores the folder except its `README.md`; `git status --ignored` confirms the PNGs are ignored.
- Packaging:
  - `build-installer.ps1` now checks that all four slots have a file to embed and fails otherwise, unless `-AllowMissingProviderIcons` is passed. The staging guard still forbids loose logo files, since the marks live only inside the EXE.
  - `.iss`, README, `docs/provider-icons.md` and `docs/design/brand-assets.md` were updated.
- Release check:
  - Release EXE contains each asset byte-for-byte.
  - LIVE start logs `source=Bundled` for all four slots; the flyout shows both marks.
  - Portable ZIP contents: two EXEs, `LICENSE`, `README.md`.
- Checks: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` (core 31, bridge 3, integration 1, UI 64) and `git diff --check` passed. Release build passed. The installer was not built: Inno Setup 6 is missing.
- Release blockers:
  1. Brand permission for public distribution of a build with embedded marks. OpenAI marks terms apply. Anthropic requires prior approval and forbids colour changes; the white/#111111 Spark variants are recolours.
  2. Inno Setup 6 is not installed, so there is no installer `.exe` yet.
  3. Real Windows 125/150/200 % scaling and a live warning-state tray icon are still unverified.

### Next actions
1. User visual approval of the flyout, Settings, About and the custom tray menu.
2. Install Inno Setup 6, run `packaging/windows/build-installer.ps1 -SkipBuild`, and inspect the installer before any install test.
3. Optional: check real Windows 125/150/200 % scaling; decide whether CI should also build the release ZIP.

## Current iteration — Windows sign-in launch and v0.1.0 audit, 2026-11-21
- Added `launch_at_startup` to persisted settings with a serde default of false. Added a localized General settings toggle (UK/EN/PL/RU) in the existing neutral card/toggle style.
- New `src/autostart.rs` owns one HKCU `Software\Microsoft\Windows\CurrentVersion\Run` value named `AgentUsageMonitor`. Enable writes the quoted `current_exe()` path; disable deletes only that value. Startup reconciles according to the saved preference, repairing a moved portable/installed executable path or removing a stale app entry. It preserves the opt-in `--enable-claude-bridge` argument when the current launch has it. Settings UI reads back the actual registered path; if it cannot verify the registry, it displays a localized error state and disables the toggle. Registry/write failures are logged and do not report a successful change. Existing `SingleInstance` mutex prevents a second app process.
- Inno Setup uninstall removes only the AUM Run value. The settings JSON remains subject to the existing explicit uninstall choice. README/notification module notes were updated. No Windows setting was toggled during testing, no installer was run, and no files were deleted.
- Tests added for off-by-default/backward-compatible settings, Unicode/spaced executable quoting, exact current-path matching, stale paths, and unsupported extra args. Verification: `cargo fmt --all --check --manifest-path rust/Cargo.toml`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` (101 passed: Core 31, bridge 3, bridge process 1, UI 66), Windows UI build, and `git diff --check` passed.
- A fresh optimized MSVC release build also passed in an isolated target directory `%TEMP%/aum-v010-release-audit-20261121`; it did not overwrite the existing release EXE or ZIP. The UI toggle determines the next state from a fresh registry read, so an externally removed or stale entry does not invert the user's visible state incorrectly.
- Packaging audit: existing portable ZIP contains the GUI EXE, Claude bridge EXE, LICENSE and README; its GUI EXE SHA-256 matches the existing release EXE. Both were timestamped 2026-11-21 16:14 UTC, earlier than this autostart code, so neither artifact includes the new feature and they must be rebuilt before release. Inno Setup `ISCC.exe` is absent; installer readiness is blocked. No package script was run because it deletes/replaces its stage directory and portable ZIP.
- Reproducibility/licensing: all four provider PNG slots exist locally but are git-ignored; clean checkout cannot recreate a logo-bearing package without separately supplied assets. `build-installer.ps1` fails closed when those slots are absent (or can explicitly build unbranded with `-AllowMissingProviderIcons`). OpenAI/Anthropic redistribution permission remains unresolved; the local Spark logo variants are recoloured contrary to the recorded Anthropic guideline. Do not publish these logo-bearing artifacts until permission/compliant assets are resolved. Public release is BLOCKED on this point.
- Windows toast audit: the installer Start Menu shortcut sets the AUMID, which is required for classic desktop toast delivery; the portable ZIP has no shortcut, so its `ToastNotificationManager` delivery is not supported/reliable and may fall back to a legacy balloon. Microsoft recommends Windows App SDK `AppNotificationManager` for new unpackaged desktop apps. Uninstall removes AUMID registration and its generated PNG; this also means a separately retained portable copy must re-register on next launch. These notification/portable identity behaviors were not changed in this task.
- All `--preview-*` window/menu paths are compiled only under `debug_assertions`. `build.rs` builds the AUM icon `.res` directly and lets the MSVC linker use `cvtres`, so `rc.exe` is not required. Provider protocols in `codex.rs` and `claude_usage.rs` remain on supported local sources; focused/full tests passed.
- Visual review of the new General row was not performed; Claude Code should check spacing/scrolling at the supported window sizes and all three themes. No live autostart toggle, sign-in, moved-portable-path, installer uninstall, or toast test was performed. The prior running AUM process from this repo was stopped only after its executable path was verified so Cargo could link the updated UI binary; no AUM instance was relaunched.
- No commit, push, tag, publication, installation, or deletion was performed. Existing dirty files and local provider assets were preserved.

### Follow-up — visual review of the startup row (Claude Code, 2026-11-21)
- Reviewed Settings → General "Launch at Windows sign-in" in Graphite, Bluish Dark and Light, UA/PL/EN/RU, OFF/ON/unverified states, at 100 % and with 1.25 scale emulation.
- Correct as built: own card 8 px below Appearance (same rhythm as the stacked provider cards), standard `SettingsRow` (13 px title, 12 px description, about 54 px row), switch vertically centred on the 16 px content edge. No clipping in any language. The General page fits the default 720×640 window without scrolling.
- Confirmed defects, fixed in `ui/main.slint` only:
  1. The unverified/error description looked like a normal muted description. `SettingsRow` gained `error`; the startup row sets `error: !startup-state-known`, so the text uses the semantic warning text colour (#FACC15 dark, #A16207 Light).
  2. Disabled switches at opacity 0.4 were nearly invisible on Light cards. This affects all disabled switches and is now 0.55.
- Autostart backend and registry logic are unchanged. `HKCU\...\Run\AgentUsageMonitor` was absent before and after all captures (`launch_at_startup` unset, so startup reconciliation was a no-op).
- Added a debug-only `--preview-startup=on|error` flag (`PREVIEW_STARTUP`, `cfg(debug_assertions)`). It overrides only the UI values in `update_settings_window` and does no registry access or change.
- Showcase: no approved showcase asset set exists in the repository (only `docs/design/references`). Screenshots affected by the new row are every Settings → General capture. Updated set: scratchpad `showcase-autostart-20261121/`; earlier sets were not overwritten.
- Checks: `cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`, UI tests 66/66 and `git diff --check` passed. No AUM instance was left running, matching the state at start.

## Current iteration — v0.1.0 local installer build, 2026-11-21
- Built the current x64 MSVC release from the branch's latest source (including the reviewed General startup row) into a fresh `%TEMP%` Cargo target, then compiled with the per-user `C:\Users\fetis\AppData\Local\Programs\Inno Setup 6\ISCC.exe` (Inno Setup 6.7.3). The installer compiled successfully; the application was not installed or run.
- Updated `packaging/windows/build-installer.ps1` with `-OutputDirectory` and `-TargetDirectory`. It now refuses to overwrite any existing stage, ZIP, or setup output and no longer removes prior staging/artifact files. Its ISCC discovery includes `%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe`; the actual build passed the exact ISCC path explicitly. All four required provider icon slots were present; `-AllowMissingProviderIcons` was not used.
- New output directory: `%TEMP%\AUM-v0.1.0-local-installer-20261121`. Installer: `AgentUsageMonitor-0.1.0-win-x64-setup.exe`, 7,750,471 bytes, SHA-256 `F37C641FB41E4037970B66EFA18AF4C634566F0E62379F2944CA68E8D706B31A`. Portable ZIP in the same directory: 7,903,784 bytes, SHA-256 `449439F47A22BD2404C85D103B493C10E6AAF95ECE8DD77D5691B765B2A58D0E`.
- Fresh release app EXE: 17,684,480 bytes, SHA-256 `2835CE4237D0C59D388A510F80BBF980BC77E481DB29892336B08CB5848314A3`; staged EXE hash matches. All four local provider PNG files were found byte-for-byte embedded in the fresh EXE. The ZIP contains only the GUI EXE, Claude bridge EXE, LICENSE, and README (no provider PNG files).
- Validation: Setup PE metadata reports ProductName `AgentUsageMonitor`, ProductVersion `0.1.0`, FileVersion `0.1.0.0`, CompanyName `Valentyn Fetisov`; application and setup icons both extract at 32×32. Inno verified the setup executable and compiled the script's metadata, Start Menu/optional desktop shortcuts with AUMID, per-user privileges, uninstall entry, AppUserModelId cleanup, notification PNG cleanup, and HKCU `Run\AgentUsageMonitor` `uninsdeletevalue` cleanup. These were source/compiler checks only; no install or uninstall was performed.
- The old repository release EXE and portable ZIP hashes remain unchanged; the old `rust/target/installer/stage` timestamp is unchanged. Read-only check confirmed `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\AgentUsageMonitor` remains absent. No Windows startup value was modified.
- Remaining public-distribution blockers: provider-logo redistribution permission is unresolved (and the local Claude Spark variants are recoloured); portable classic toasts still lack a registered Start Menu shortcut. This package is for local testing only. Installer path and hashes are reported in the task response.
- No commit, push, tag, publication, installation, uninstallation, or deletion. Existing dirty work and local assets remain preserved.

## Current iteration — topmost and uninstall fixes, 2026-11-21
- Root cause: `MainWindow` was declared `always-on-top: true` in Slint. Calling `SetWindowPos(HWND_NOTOPMOST)` after show was not durable because the toolkit's window level remained topmost and could reapply it. Settings' declarative window was also topmost.
- Fix: removed persistent always-on-top from the main flyout and Settings. Temporary previews retain the existing nonactivating Winit presentation. Explicit pinned Open and Settings actions use `SetForegroundWindow` (no topmost band), so they foreground normally and remain in normal Z-order. The custom tray menu retains its own topmost behavior.
- Inno uninstall confirmation now shows the requested exact UA/EN/PL/RU prompt, standard Yes/No buttons, at `usPostUninstall`. Yes deletes only `settings.json` and `alert-state.json`, then nonrecursively removes the AUM directory only if empty. No and silent uninstall preserve data. The provider-icons directory and all user files are preserved by design. Existing AppUserModelID cleanup and only the AUM HKCU Run value cleanup remain.
- Mechanical Clippy-only fixes: moved Claude parser helpers before tests; adjusted two test initializers and a redundant arithmetic expression without behavior changes.
- Validation: fmt check passed; `cargo clippy --workspace --all-targets -- -D warnings` passed; `cargo test --workspace` passed (101 tests); `git diff --check` passed. Inno Setup 6.7.3 successfully compiled the installer.
- Fresh isolated artifacts in `%TEMP%\AUM-v0.1.0-topmost-uninstall-20261121-190121`: setup SHA-256 `537F9924C3E6C03D88E8D980A68B72BA37DCD17F81982944F56251BA9D78FB80`; portable ZIP SHA-256 `2B988883BC3BD1E512ADE8DD301CA9D32E0A8EAB54DE08FEA01994A7DDB47D48`. ZIP contains only the two application EXEs, LICENSE and README.
- No install/uninstall, commit, push, tag, or publication was performed. Existing dirty work was present before this task and was preserved. Manual runtime Z-order and actual uninstaller button behavior remain unverified because installation/uninstallation was explicitly prohibited.

## Current iteration — reproducible provider SVG bundle, 2026-11-21
- Located the exact original vector sources in the official OpenAI logo package and Anthropic press kit. Local successful builds had bundled four derived PNGs; the source assets are two original OpenAI Blossom SVG variants and the original Anthropic `Claude Spark - Clay.svg`.
- Added the original SVG bytes under `rust/agent-monitor-ui/assets/providers/`: `OAI_OpenAI-Blossom_White.svg`, `OAI_OpenAI-Blossom_Black.svg`, and `Claude Spark - Clay.svg`. The build embeds them from the tracked tree by default. Optional `AUM_PROVIDER_ICONS_DIR` build-time files and `%APPDATA%\Agent Usage Monitor\provider-icons\` runtime overrides remain supported; runtime user icons now override embedded defaults, and missing slots fall back to embedded files.
- Narrowed `.gitignore` to whitelist only these approved provider SVGs while retaining the git-ignored local override folder. Updated packaging preflight to require the three tracked source files. Updated provider icon docs and third-party brand notice: the marks remain owner trademarks and are excluded from AUM's MIT license.
- Validation: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (101 tests), `git diff --check`; all passed. Added test that confirms exact SVG bytes render as file-loaded SVGs and fill all four slots without a local folder; override precedence test passes.
- Clean isolated Windows release build and Inno Setup 6.7.3 compile succeeded with `AUM_PROVIDER_ICONS_DIR` unset, using only tracked SVG defaults. Output folder `%TEMP%\AUM-v0.1.0-tracked-provider-svgs-20261121-193504`: installer SHA-256 `5651704F3620B0DBEFE6DA5864D5EFE46B2B3D89E8BAAD1525F599981CC7D374`; portable ZIP SHA-256 `D7F2EDBD1B35B54D4767D8303C4726EAD57081CCD672FA38ADAB24052410B5F9`.
- No install/uninstall, release publication, or tag was performed. Earlier local PNG assets remain untouched and ignored.

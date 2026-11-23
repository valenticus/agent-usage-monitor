# State: feat/store-msix

## Goal and constraints
- Prepare a Microsoft Store (MSIX) distribution of AgentUsageMonitor 0.1.1 next to the existing EXE builds, from the same binary.
- Worktree `C:\project\agent-usage-monitor-msix`, branch `feat/store-msix` from `main` at `daba890`. Pushed; HEAD `565dac4` = `origin/feat/store-msix`. **PR #2** (https://github.com/valenticus/agent-usage-monitor/pull/2) open, CI `windows` green, **not merged**.
- Do not touch the published v0.1.1 release, the Inno installer, WinGet PR #486060, the user's installed AUM or its settings. Do not submit to the Store.
- Build and test only on mains power. Never install the MSIX on the user's working machine, never run a generated package or AUM binary, and never start a second AUM instance there.

## Git state (2026-11-22, after integration)
- Codex review #3: PASS WITH CONDITIONS (no source-level integration blockers).
- Feature commit `10ff200` (`feat(msix): add Microsoft Store (MSIX) build from the same binary`), parent `daba890`, 23 files, author Valentyn (configured identity), no AI trailers.
- `git fetch origin`: `origin/main` still `e4dbc26` (no new commits beyond the reviewed main).
- Merge commit `0c31e22` = `10ff200` + `main` `e4dbc26` (normal merge, `--no-ff`). **No conflicts**; `main.rs`, `claude_refresher.rs` and `lib.rs` auto-merged. `main` itself unchanged.
- Brought in from `main`: tray sleep/resume recovery, compact flyout Z-order, duplicate Claude refresh prevention, isolated bridge integration test (`4c77301`).
- Pipe isolation gate (before the full suite): `tests/claude_bridge_process.rs` uses a unique `…ClaudeStatusLine.test-<pid>-<nanos>` pipe via `run_claude_status_pipe_named` and passes it to the bridge child through `AUM_CLAUDE_PIPE_NAME`; it asserts the name differs from the production pipe, and the bridge rejects (does not fall back from) any non-test override. Production pipe use remains only in app runtime code. No test writes the registry, calls `reconcile_autostart`/`register_identity`, or saves into the real settings folder. The former hazard (production-pipe test on this branch) is resolved by the merge.
- Not pushed. Backups of the pre-commit work remain in the scratchpad (`…\c7a95641-…\scratchpad\msix-backup`, `…\8da2abdf-…\scratchpad\backup-before-codex-fixes`, `…\backup-before-review2-fixes`).

## Codex review (FAIL, 4 high findings) — resolved in the worktree
- **H1 settings isolation:** the Store build's data folder is now the package's `LocalState` (`ApplicationData.Current.LocalFolder`), never `%APPDATA%\Agent Usage Monitor`. `settings::save` refuses the EXE folder unless the process is confirmed unpackaged. Unresolvable folder or unknown identity: no persistent storage (see review #2 below; the earlier `%TEMP%` fallback was removed). The legacy `Agent Monitor` migration runs only for confirmed unpackaged processes. The Store build no longer saves settings at launch.
- **H1 import:** explicit, optional, one-time **Import settings from the EXE version** row (Store build, General tab, UA/EN/PL/RU). Read-only toward the source, validated, no-clobber, settings written last. Imports preferences only; launch at sign-in, the bridge opt-in and Claude CLI background refresh stay off. Copies custom provider icons with known names (≤ 2 MiB, used from the next start). No alert history, credentials or other files.
- **H2 StartupTask threading:** `autostart::StartupController` runs every StartupTask call on a worker thread and reports through a listener → `slint::invoke_from_event_loop`. The UI thread never waits. One request at a time (extra clicks ignored, toggle shows "Checking…"). Panics and errors are reported and never leave it busy. Launch and opening Settings only read the task. A click without a verified state only re-reads. All five states handled; locked states are never overridden. HKCU `Run` path for EXE builds unchanged.
- **H3 bridge lifecycle:** one rule `claude_bridge_wanted` = Claude provider on **and** opt-in (argument or Store toggle). `run_bridge_lifecycle` starts/stops the listener exactly on that rule, drops the listener future at once (pipe handle closed), never runs two listeners, reports a failure once and retries only after an off→on change, and stops for good on exit. The claude receiver is always drained and drops unwanted messages. The Store toggle is always operable (revocable while Claude is off). Behaviour change for EXE builds: with `--enable-claude-bridge`, the listener now also stops while Claude is off.
- **H4 processes:** `cmd.exe`, `powershell.exe`, `taskkill.exe` now start from the system directory by absolute path (`process_tools::system_tool_path`). A native `claude.exe` on `PATH` is started directly; npm shims keep `cmd.exe /D /S /C`. Codex resolution order unchanged. S mode cannot run the third-party CLIs at all; documented as an open Store risk.
- **PFN:** corrected everywhere to the confirmed `ValentynFetisov.AgentUsageMonitor_sfbqfhxtgjc58`. `build-msix.ps1` now fails if the derived PFN differs.
- **Package detection:** `packaged::state()` = `Packaged` / `Unpackaged` / `Unknown`. `Unknown` gets no Store UI, no HKCU `Run` or AUMID writes, and no persistent storage (review #2: the former `%TEMP%` folder was removed).
- **Coexistence:** mutex unchanged; documented (whichever starts first wins; recommend one build; settings no longer interfere).

## Codex review #2 (FAIL: 1 high, 3 medium, 1 low) — resolved in the worktree
- **High, WinRT init:** new `winrt.rs` RAII guard `Apartment` (`RoInitialize(RO_INIT_MULTITHREADED)`, balanced `RoUninitialize` only when owned; `RPC_E_CHANGED_MODE` detected, never uninitialized, rejected before blocking waits). `ApplicationData.LocalFolder` is read on a dedicated `aum-winrt` MTA thread; each StartupTask `query`/`set` takes the guard first on the controller worker. Feature `Win32_System_WinRT` (+ `Win32_System_Com` for the STA test) of the existing `windows` crate; no new crates.
- **Medium, TEMP fallback removed:** `settings_directory()` is `Option`. No LocalState (or unknown identity) ⇒ no persistent storage: defaults in memory, `save` returns `StorageUnavailable`, alert state in memory, embedded icons only, import hidden, localized warning row in Settings → General (UA/EN/PL/RU, `storage_unavailable`).
- **Medium, Claude shim:** `process_tools::resolve_cli_in` (absolute PATH entries only, `.exe` then `.cmd`). `claude.exe` direct; `claude.cmd` via `<System32>\cmd.exe /D /V:OFF /S /C ""<full path>" args"` (`raw_arg`); `%`, `"`, control chars refused; nothing resolved ⇒ error, no process.
- **Medium, system utilities:** `system_tool_path` returns `Result`; buffer grows for long paths; non-absolute/unstable results rejected; no bare-name fallback. Callers: Codex `.cmd`/`.ps1` ⇒ spawn error; cleanup ⇒ direct child kill only.
- **Low, identity:** `build-msix.ps1` also checks Identity Name, Publisher, DisplayName, PublisherDisplayName and VisualElements DisplayName (exact). Verified failing on a scratch copy with a changed PublisherDisplayName. CI runs it (`-ValidateManifestOnly`).

## Decisions (and why)
- Store identity confirmed by the user in Partner Center (see `docs/msix.md`). Not changed.
- Claude bridge: **option A**. Store build has an off-by-default Settings toggle (`claude_bridge_listener_enabled`), because packaged apps cannot pass `--enable-claude-bridge` on Windows 10 1809. EXE builds ignore the toggle.
- Bridge opt-in survives turning Claude off and on (same as the Claude CLI refresh opt-in); only the user's toggle changes it. Reason: disabling Claude is not revoking the bridge, and the toggle stays operable for revocation.
- Runtime detection (`GetCurrentPackageFullName`) instead of a Cargo feature: one binary, unchanged EXE behaviour.
- Store data folder = `LocalState` via `ApplicationData` (not a computed path, not AppData virtualization). Reason: documented package-owned location, removed on uninstall, no dependence on undocumented rename virtualization.
- Import is a button, not automatic. Reason: the task forbids silent migration of security-sensitive settings; an explicit click with a clear description is understandable. Available only while the Store build has no settings, which guarantees "never overwrite Store settings".
- Alert history is not imported: the alert runtime loads its state at launch and would overwrite an imported file anyway; at worst one alert repeats.
- `RequestEnableAsync` on a worker (MTA): Microsoft's UI-thread note applies to UWP's consent dialog only; for packaged desktop apps no dialog is shown and the class is agile.
- Startup task is read-only at launch; the user's toggle is the only writer. Imported settings never enable it.
- Bridge rule extended to EXE builds (Claude off ⇒ listener off): one consistent rule; previously the EXE listener accepted and discarded data while Claude was off.
- `cmd.exe` stays for npm `.cmd` shims (batch files need it). `taskkill /T` stays for tree cleanup. No job-object rewrite.
- VCLibs framework dependency (EXEs import `VCRUNTIME140.dll`), packaged toasts with the package AUMID, manifest with `runFullTrust` only, `en-us` resources only, unsigned `.msix` (Partner Center signs). Unchanged.
- Singleton mutex unchanged (sharing it is intended: only one AUM runs).

## Checks on merge commit `0c31e22` (2026-11-22, mains power, this machine)
| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo check --locked --workspace --all-targets` | pass |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | pass |
| `cargo test --locked --workspace` (after the isolation gate) | pass: **152/152** — core 37, bridge bin 3, bridge integration test 1 (isolated pipe), UI 111, doc-tests 0 |
| `build-msix.ps1 -Jobs 2 -OutputDirectory rust\target\msix-0.1.1.0-merged` | pass: release build + unsigned `AgentUsageMonitor_0.1.1.0_x64.msix`, 6,780,508 B, SHA-256 `8A5EE1B7…1AC43009`; VC runtime guard: both EXEs import `VCRUNTIME140.dll`, VCLibs dependency declared |
| `build-msix.ps1 -ValidateManifestOnly` | pass; PFN `ValentynFetisov.AgentUsageMonitor_sfbqfhxtgjc58`, Name, Publisher, DisplayName, PublisherDisplayName, VisualElements DisplayName match |
| `makeappx unpack` (scratch, not installed) | Identity `ValentynFetisov.AgentUsageMonitor` / `CN=090E947F-…F9BA` / `0.1.1.0` / x64; `Microsoft.VCLibs.140.00.UWPDesktop` ≥ `14.0.33728.0`; `runFullTrust` only; `windows.startupTask` `AgentUsageMonitorStartup` `Enabled=false`; App Execution Alias `agent-usage-monitor-claude-bridge.exe`; 14 payload files; both EXEs byte-identical to the release build |
| Localization | UA/EN/PL/RU completeness test passes (in the UI suite) |
| `Cargo.lock` | unchanged (only crate features added) |

Earlier (pre-commit) results: review #2 worktree 137/137 unit tests; negative identity check on a scratch copy failed as intended.

NOT TESTED (needs a clean VM or second PC): see `docs/msix.md` → "Testing limits and risks".

## Open questions (need the user)
1. S mode / Store certification: Microsoft says Store apps "must be compatible with Windows 10 S"; AUM's CLIs cannot run there. Decide whether to test S mode on a VM before submission and how to word the dependency disclosure / certification notes.
2. Coexistence policy: keep "whichever starts first wins" (documented) or have one build defer to the other (product decision; would change installed behaviour).

## Known limitations
- Packaged runtime is untested (see NOT TESTED). CI only validates the manifest statically.
- Claude CLI paths containing `%` and `.bat`/`.ps1` Claude installs are refused (fail closed).
- Without LocalState the Store build runs with in-memory defaults and cannot save (visible warning).
- S mode: third-party CLIs cannot run; certification acceptance uncertain.
- EXE + Store installed together: whichever starts first at sign-in wins.

## Runtime QA on the user's machine (2026-11-22/23, outside the repo)
- With the user's approval, the user's Inno v0.1.1 was replaced by a test-signed MSIX 0.1.1.0 (cert `C63094CB…`, valid until 2026-12-06). Recovery backups live in `%USERPROFILE%\AUM-recovery\`. The full report is `%USERPROFILE%\AUM-recovery\qa\QA-REPORT.md`.
- PASS (automated): install/identity, registrations, tray, flyouts, settings UI, LocalState persistence, localization, StartupTask toggle, packaged toasts, Codex, Claude CLI, bridge alias and lifecycle, single instance, cancellation on exit.
- USER-OBSERVED PASS: start at sign-in (Q09b), DisabledByUser lock (Q10), 3–5 min sleep (Q17).
- Phase 2 (2026-11-23):
  - 12 static package checks (file-level WACK equivalents) pass;
  - event logs are clean, activation and startup consistency check out;
  - the full test suite passes 152/152 on `565dac4`.
- Final QA (2026-11-23, user-authorized, each test restored and verified before the next):
  - Q08 packaged import: PASS. Preferences imported; startup, the bridge and Claude refresh stay off; source unchanged; restored byte-identical.
  - Q27 Inno coexistence: PASS. One instance either way, the losing copy writes nothing, settings and toast identities are isolated, clean teardown. The sign-in race is MANUAL.
  - Q22 update 0.1.1.0→0.1.1.1 and in-place downgrade back: PASS.
  - Q25 WACK: **overall WARNING**. 21 PASS; optional "Blocked executables" FAIL (F-6: `cmd.exe`/PowerShell for CLI shims, S-mode risk); "DPIAwarenessValidation" WARNING (F-5: no embedded DPI manifest, awareness is set at run time). The package was not removed. Live limits afterwards: Codex 100/95 %, Claude 48/57 %.
  - Q18 Explorer restart: PASS. Icon back within 1 s, same PID, flyouts work.
- No MSIX functional defect. F-4 confirmed: Inno setup and uninstall are blocked while the Store AUM runs (shared `AppMutex`).

## Final Codex review (2026-11-23): PASS WITH CONDITIONS. Response committed and pushed in the review-fix commit
- **DPI (F-5):** `rust/agent-monitor-ui/windows/agent-usage-monitor.manifest` (asInvoker; PerMonitorV2 with the PerMonitor / `true/pm` fallbacks) is embedded by `build.rs` into the GUI binary only (`/MANIFEST:EMBED /MANIFESTINPUT /MANIFESTUAC:NO`).
  - Reason: one identical GUI binary for Inno, portable and MSIX; the bridge needs no manifest.
  - Winit's later `SetProcessDpiAwarenessContext` call fails with ACCESS_DENIED and is ignored by winit, so there is no conflict.
- **Codex `.cmd` (genuine defect):** the shim fallback passed a pre-quoted line through Rust's argument quoting and appended the arguments twice, so a `codex.cmd`-only install could not start. A regression test reproduced it (`codex_process_error`). Fix:
  - `process_tools::cmd_shim_command_line` (shared; refuses `%`, `"`, control characters and non-token arguments) + `raw_arg`;
  - Codex PATH search uses absolute entries only, the same as Claude; explicit paths must be absolute;
  - native `codex.exe` and `.ps1` behaviour unchanged (tested).
- **Tests:** 158/158 (2 consecutive runs). Codex mock test timeouts were widened because PowerShell start-up under load made one pre-existing cancellation test flaky.
- **QA corrections:** WACK 22/1/1; WACK history; static check M9 renamed; Q27 assertion fixed (repeat run pending); the Explorer script restores Explorer in a `finally` block. Recorded in `QA-REPORT.md` → "Corrections after the independent Codex review".
- **Docs:** `docs/msix.md`: test results, actual WACK result, mutex and installer behaviour, update vs forced downgrade, S mode, CLI dependency, certification notes, untested scenarios. The PR #2 description was updated.
- **Decisions:**
  - Claude's own `cmd_shim_command_line` stays unchanged, because it is already tested and in use. Moving it onto the shared helper is optional follow-up.
  - The user turned **Claude background refresh on** in the installed MSIX (2026-11-23 01:51) on purpose. That is the new baseline: do not restore the checkpoint value.

## PR readiness
PR #2 is open; **do not merge yet**. Still needed:
1. the final runtime QA of the fixed binary: replace the installed MSIX, re-run WACK and the live checks, check DPI (needs separate user approval for the replacement);
2. the Q27 repeat (needs mains power or more than 35 % battery);
3. the final Codex review.

## Next actions
1. With user approval, replace the installed MSIX with `AUM-recovery\test-signing\2026-11-23-review-fixes\…REVIEW-FIXES.TEST-SIGNED.msix`.
   - Same version 0.1.1.0, so use `-ForceUpdateFromAnyVersion`; data is kept.
   - Back up `LocalState` first; the user's current settings are the baseline.
   - Then re-run WACK and the live checks, and check DPI at 100/150/200 %.
2. Q27 repeat with the corrected assertion.
3. Final Codex review. Store submission stays blocked on the S-mode decision.
4. Remove the test certificate after QA (expires 2026-12-06).
5. Partner Center (only after the S-mode decision):
   - listing, with the dependency disclosure at the start of the description;
   - screenshots, privacy policy URL, age rating;
   - upload the unsigned `.msix` from a clean build of the merged branch.
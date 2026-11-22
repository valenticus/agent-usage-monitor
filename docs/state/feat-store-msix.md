# State: feat/store-msix

## Goal and constraints
- Prepare a Microsoft Store (MSIX) distribution of AgentUsageMonitor 0.1.1 next to the existing EXE builds, from the same binary.
- Worktree `C:\project\agent-usage-monitor-msix`, branch `feat/store-msix` from `main` at `daba890`. Not committed, pushed or merged.
- Do not touch the published v0.1.1 release, the Inno installer, WinGet PR #486060, the user's installed AUM or its settings. Do not submit to the Store.
- Build and test only on mains power. Never install the MSIX on the user's working machine, never run a generated package or AUM binary, and never start a second AUM instance there.

## Git state (2026-11-22)
- Branch HEAD `daba890` (unchanged). `main` is at `e4dbc26`, 5 commits ahead (PR #1 tray/resume/flyout reliability: `2d66f12`, `5920747`, `4c77301`, merge `555cd99`, docs `e4dbc26`).
- All MSIX work is uncommitted: 16 modified tracked files and 6 untracked paths (`docs/msix.md`, this file, `packaging/msix/`, `rust/agent-monitor-ui/src/packaged.rs`, `rust/agent-monitor-ui/src/winrt.rs`, `rust/agent-monitor-core/src/process_tools.rs`).
- Backups of the uncommitted work: the first one from the earlier session (`…\c7a95641-…\scratchpad\msix-backup`), patch + tar before the review #1 fixes (`…\8da2abdf-…\scratchpad\backup-before-codex-fixes`) and before the review #2 fixes (`…\8da2abdf-…\scratchpad\backup-before-review2-fixes`).
- Integration with `main` checked **without changing the branch**: a snapshot commit of the worktree built through a temporary index (real index, HEAD and refs untouched) merges with `main` **cleanly** (`git merge-tree`). Results of the merged tree: see Checks.
- **Hazard on this branch until `main` is merged:** `agent-monitor-core/tests/claude_bridge_process.rs` still uses the production pipe (fixed on `main` by `4c77301`). Never run the full `cargo test` here next to a live AUM. Safe command: `cargo test --locked --workspace --lib --bins -- --skip status_line_executable_forwards_allowlisted_data_and_exits_cleanly` (no integration tests at all).

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

## Checks (2026-11-22, mains power, this machine)
| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo check --locked --workspace --all-targets` | pass |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | pass |
| `cargo test --locked --workspace --lib --bins -- --skip status_line_executable_…` | pass: 137 (core 35, bridge 3, UI 99), incl. a real temp `claude.cmd` in a folder with spaces/Unicode/`&`/`^` and real `RoInitialize` guard tests on test threads; integration tests not run |
| `build-msix.ps1 -ValidateManifestOnly` | pass; PFN, Name, Publisher, DisplayName, PublisherDisplayName, VisualElements DisplayName match. Negative check on a scratch copy (changed PublisherDisplayName): fails as intended |
| `build-msix.ps1 -Jobs 2 -OutputDirectory rust\target\msix-0.1.1.0-review3` | pass: release build + unsigned `AgentUsageMonitor_0.1.1.0_x64.msix`, 6,778,948 B, SHA-256 `A84EFFAC…84AF459E`; nothing installed or run |
| Merged tree (snapshot after review #2 fixes + `main`, scratch copy) | `merge-tree` clean; fmt, Clippy `--all-targets -D warnings` pass; unit tests 151/151 (core 37, bridge 3, UI 111). Integration tests not run |
| `Cargo.lock` | unchanged (only crate features added) |

NOT TESTED (needs a clean VM or second PC): see `docs/msix.md` → "Testing limits and risks".

## Open questions (need the user)
1. S mode / Store certification: Microsoft says Store apps "must be compatible with Windows 10 S"; AUM's CLIs cannot run there. Decide whether to test S mode on a VM before submission and how to word the dependency disclosure / certification notes.
2. Coexistence policy: keep "whichever starts first wins" (documented) or have one build defer to the other (product decision; would change installed behaviour).

## Next actions
1. Codex: second independent review of this worktree.
2. After approval: commit on `feat/store-msix` (named paths), merge `main` into the branch (clean per `merge-tree`), run the full `cargo test` (bridge test isolated after the merge), push, check CI.
3. On a clean Windows 10 1809+ / Windows 11 VM: install the VCLibs framework, sideload with a test certificate, run WACK, then the items in `docs/msix.md` → "Testing limits and risks" (import, StartupTask states, bridge toggles, CLIs, coexistence, S mode).
4. Partner Center: listing (dependency disclosure at the start of the description), screenshots, privacy policy URL, age rating; upload the unsigned `.msix` from a clean build of the merged branch.

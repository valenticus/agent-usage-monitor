# State: feat/v0.3.0-smart-installer-phase-a

_Handoff file for this branch, in the Agent Kit `handoff` skill format. Only this branch's agent edits it._

## Goal and constraints
- Address independent Phase A and final review findings in the existing worktree.
- Do not create branches/worktrees, push, merge, launch the installer, or alter the installed RC/MSIX. A focused local commit is approved after all checks pass.
- Preserve the existing governance change in `AGENTS.md`.

## Done
<!-- each item: verified fact (with evidence) or assumption -->
- Updated bridge preference precedence to `claude_enabled && (cli_flag || saved_opt_in)` for EXE; CLI override remains process-scoped and is shown in Settings.
- Added isolated EXE bridge opt-in settings and marker consumption guarded by package identity and exact install path.
- Updated installer migration, mutex/blocker, downgrade/version parsing, shortcut ownership, legacy evidence, and uninstall safety logic. No-evidence `Done` is deferred until `ssPostInstall`; uninstall preflight does not access either Setup or uninstall form objects; helper replacement uses normal Inno retry behavior without `restartreplace`.
- Portable legacy `Run` migration verifies the exact current executable and exact bridge argument, requires no explicit EXE choice, persists ON before removing the argument, and skips autostart reconciliation when migration persistence fails.
- EXE CLI override is visible and process-scoped; MSIX uses only its independent saved setting.
- Added four-language strings, focused migration tests, and Phase A behavior/limitation documentation.
- Compiled the final Inno script with Inno Setup 6.7.3 into ignored `rust/target/installer-phase-a-review3`; compilation succeeded. The installer was not launched.
- Final review fixes: uninstall preflight no longer updates Setup's form; locked helper uses normal file error/retry behavior without reboot replacement; Portable Run migration persists opt-in before stripping the flag; no-evidence Done waits until successful `ssPostInstall`.

## Git state
<!-- Base HEAD and branch at task start; final implementation is the commit containing this handoff. -->
- Task started at base `1afac64914df62d2d7da07405123ec6e39f9ecc6` on `feat/v0.3.0-smart-installer-phase-a`. The implementation and this handoff were committed locally after checks; no push or merge.

## Changed files
- `AGENTS.md` (pre-existing approved governance change; preserved)
- `docs/claude-status-line.md`
- `docs/msix.md`
- `docs/smart-installer-phase-a.md` (new)
- `docs/state/feat-v0.3.0-smart-installer-phase-a.md` (new Agent Kit handoff)
- `packaging/windows/AgentUsageMonitor.iss`
- `rust/agent-monitor-ui/src/autostart.rs`
- `rust/agent-monitor-ui/src/installer_migration.rs` (new)
- `rust/agent-monitor-ui/src/localization.rs`
- `rust/agent-monitor-ui/src/main.rs`
- `rust/agent-monitor-ui/src/settings.rs`
- `rust/agent-monitor-ui/ui/main.slint`

## Checks
<!-- commands run with results; not-run or stale checks named as such -->
- `cargo fmt --manifest-path rust/Cargo.toml --all --check` — passed.
- Focused `installer_migration::tests` — passed (16 tests).
- `cargo test --locked --manifest-path rust/Cargo.toml --workspace` — passed (45 Core, 3 bridge unit, 1 bridge process, 181 UI tests).
- `cargo test --locked --release --manifest-path rust/Cargo.toml --workspace` — passed with the same suite counts; test profile reports the existing `QA_ABOUT_PREVIEW` dead-code warning.
- `cargo clippy --locked --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings` — passed.
- Windows MSVC debug and release workspace builds — passed.
- Final Inno Setup 6.7.3 `ISCC.exe` compilation — passed.
- `git diff --check` — passed before final staging.
- Native installer runtime/upgrade tests — not run; no isolated Windows VM/Sandbox available. Installer was not run on the host.

## Decisions (and why)
- Do not treat empty Selected Tasks as opt-out; migrate only positive verified legacy evidence and let no-evidence installs complete without persisting synthetic OFF.
- Preserve Pending and terminal Done markers; never reapply legacy evidence once a marker exists. Persist migration before file replacement so persistence failure safely blocks setup.
- Match migration marker and Run command executable paths exactly; only an unpackaged matching EXE consumes its marker.
- Use strict `major.minor.patch` version parsing and fail closed for unsupported suffixes.
- A locked bridge helper follows Inno's ordinary file error/retry flow; the installer does not force-close processes, schedule replacement at reboot, or promise atomic rollback.

## Blockers
- Native Inno runtime behavior remains unverified in isolated Windows; compile and static/unit tests do not establish fresh install, upgrade, Portable, lock/retry, UI, or rollback behavior.
- One full release-test invocation initially encountered a Git Bash subprocess exit `0xC000013A`; isolated rerun and subsequent complete release suite passed.

## Next actions
1. Independent review and isolated Windows native QA remain required for first install, v0.2.0 task/shortcut/Run migration, Portable migration, Store/Portable isolation, silent blockers, concurrent setup, locked helper retry, and uninstall data choices.

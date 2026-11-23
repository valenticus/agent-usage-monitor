# State: main

## Goal and constraints
- v0.2.0 is published and is the latest release. v0.1.0 and v0.1.1 stay published unchanged.
- Do not modify the published v0.1.0, v0.1.1 or v0.2.0 releases or tags.
- Microsoft Store submission of 0.2.0.0 is pending and needs explicit user approval.
- Never run authenticated Claude requests, read credentials or transcripts, use private OAuth APIs, or change global Claude settings (see `AGENTS.md`).
- Commit and push only on explicit request; never force-push.

## Releases
| Release | Tag → commit | Published (UTC) | Assets |
| --- | --- | --- | --- |
| v0.1.0 | `v0.1.0` → `ddd862b` | 2026-11-21 19:08:31 | setup 7,652,335 B; portable ZIP 7,821,645 B |
| v0.1.1 | `v0.1.1` (annotated, `c82e1bc`) → `b24d207` | 2026-11-21 20:43:30 | see below |
| v0.2.0 (Latest) | `v0.2.0` (annotated, `11f76e0`) → `e278a90` | 2026-11-23 16:00:35 | see "v0.2.0" below |

v0.1.1: https://github.com/valenticus/agent-usage-monitor/releases/tag/v0.1.1

| Asset | Size | SHA-256 |
| --- | --- | --- |
| `AgentUsageMonitor-0.1.1-win-x64-setup.exe` | 7,019,365 B | `1099D7223129C8199B413E94B711BFDF863335CCE3126DE0D20A9BCE5E337116` |
| `AgentUsageMonitor-0.1.1-win-x64-portable.zip` | 6,696,634 B | `89B781D933EC1C684D31E4535689C97ADF1A275C1B7AA77A174AA7C1C88D17E9` |

- Verified after publication: both assets download anonymously; the downloaded SHA-256 values match the local files; the README renders with the v0.1.1 banner; all README images resolve.
- The repository description was updated to: "Windows tray app that keeps Codex and Claude Code usage limits, reset countdowns and alerts in view. Built with Rust and Slint. MIT licensed."
- A local copy of the published assets is kept in `rust/target/installer-0.1.1-final/`.
- CI (`.github/workflows/rust.yml`) runs only for `feat/**` and `feature/**` pushes and for PRs to `main`, so pushes to `main` do not trigger it.

## What v0.1.1 contains
- Provider SVG normalization (`b297f0d`). The OpenAI Blossom fills 49.5 % of its viewBox and the Claude Spark 99.6 %, so Codex looked about half as large. All provider sources are now trimmed to their visible artwork, and the Codex-only 18 px flyout size was removed. Boxes are 14 px in the flyout and 16 px in Settings.
- Codex optical correction (`1edfd99`). Codex is drawn at 0.90 of the box and Claude at 1.00, centred in all themes and at all DPI scales. See `docs/provider-icons.md`.
- Release profile (`2ab3299`): `opt-level = "s"`, without LTO.
- Version 0.1.1 (`191c931`) and README for v0.1.1 (`ca02780`).
- Verification before release: `cargo test --locked --release --workspace` passed 104/104; release Clippy passed. The smoke test used the portable build with an isolated `APPDATA`. Icons were checked visually on a debug build at 100 % only.

## Size benchmark (Codex, 2026-11-21, built from `ddd862b`)
| Variant | App EXE | Portable ZIP | Note |
| --- | --- | --- | --- |
| A | 17,607,680 B | 7,821,668 B | baseline (= published v0.1.0) |
| B | 14,963,712 B | 6,693,310 B | `opt-level = "s"`; **adopted for v0.1.1** |
| C | 14,151,680 B | 6,357,298 B | further size experiment; not adopted |
| D | 13,709,312 B | 6,212,610 B | further size experiment; not adopted |

Decision: B (`opt-level = "s"`, no Thin LTO); no further size experiments. The shipped v0.1.1 app EXE is 14,967,808 B (B plus the icon fixes).

## v0.2.0 (released 2026-11-23)
- PR #2 (`feat/store-msix`, MSIX packaging) merged 2026-11-23 03:20 UTC: merge commit `89def68`. MSIX implementation is complete: one binary, Store behaviour chosen at run time from package identity (`docs/msix.md`).
- PR #3 (`feat/v0.2.0`) merged 2026-11-23 15:59 UTC: merge commit `7a23928`. Contents: F-7 solid stale tray bars, F-8 read-only Claude status-line guidance with Copy setting, F-10 amber warning colour, F-11 fixed tray column mapping, missing-CLI texts, clearer Claude wording, 4 languages. Details in `docs/state/feat-v0.2.0.md`.
- Release https://github.com/valenticus/agent-usage-monitor/releases/tag/v0.2.0 (binaries from CI-verified `e278a90`, run 45200211957):

| Asset | Size | SHA-256 |
| --- | --- | --- |
| `AgentUsageMonitor-0.2.0-win-x64-setup.exe` | 7,104,903 B | `DE852635CFF40E45D945C31552302150CB38D9762569CF3154315F0C282625D0` |
| `AgentUsageMonitor-0.2.0-win-x64-portable.zip` | 6,827,876 B | `7AE03F1AA7293CA6547B13EB4EA60B32A47AA85746CFA91363A97905D76F73C7` |

- No MSIX on GitHub. Store package (unsigned, `C347C544…`) and handoff notes: `AUM-recovery\store-handoff\v0.2.0\` (outside the repo). **Store submission pending user approval.** Open risk: Windows S mode untested.
- The installed test-signed MSIX on this PC is 0.2.0.0 rc1 (differs from release only in Claude wording).

## Git state
- `main` = merge commit for PR #3 plus the post-release docs PR; in sync with `origin/main`.
- The unpublished history was cleaned before the first push, so the obsolete design reference images were never published.
- Worktrees: `C:\project\agent-usage-monitor` and the Kilo Code pool worktree `flaxen-marmoset` (managed by Kilo; leave it). The merged worktrees `agent-usage-monitor-msix` and `agent-usage-monitor-tray` were removed in the post-v0.2.0 cleanup; remote branches were kept.

## PR #1 — tray resume and flyout reliability (merged 2026-11-22)
- PR https://github.com/valenticus/agent-usage-monitor/pull/1 was merged into `main` with a merge commit (no squash or rebase), preserving all three feature commits: `2d66f12`, `5920747`, and `4c77301`.
- Merge commit: `555cd990dd3df76c7c4110b148774c5c125cd49c`.
- Tray recovery: the hidden top-level tray window handles `TaskbarCreated` and power-resume notifications; icon registration recovery and a delayed background refresh restore state after Explorer restarts and sleep/resume.
- Flyout Z-order: the compact preview uses TOPMOST only while visible and remains non-activating; the full flyout is a normal activatable window and is not TOPMOST.
- Resume refresh coalescing: Codex and Claude CLI skip a resume refresh after a recent successful periodic refresh; manual refreshes still run and failed refreshes do not suppress a retry. Regression tests cover both timer/request orders, manual refresh, and retry after failure.
- Windows CI run `45021142186` passed formatting, Clippy, workspace tests (118 tests), and the Windows build. Runtime checks covered short sleep/resume, Explorer restart, hover focus/Z-order, and repeated flyout transitions. Long sleep, repeated sleep cycles, DPI/multiple monitors, Windows 10, and release-build runtime remain untested.

## Post-release cleanup (2026-11-21, done)
- Removed local build outputs: `rust/target/{debug,release,x86_64-pc-windows-msvc,tmp,installer,installer-0.1.1}`. Kept `rust/target/installer-0.1.1-final/`, a byte-identical copy of the published v0.1.1 assets. The next build of any kind starts from a clean cache.
- Removed the Codex benchmark worktree (`git worktree remove`) and the `%TEMP%\AUM-size-bench-ddd862b9-20261121` folder (results recorded above). Also removed `%TEMP%\aum-preview-appdata` and the Claude session scratchpad.
- Deleted the local branch `backup/pre-history-cleanup-v0.1.1`. All its commits are in `main` with identical patches, except the replaced image add/remove cycle. The reflog was not expired and no aggressive GC was run, so its objects stay until normal Git expiry.
- Deleted the merged remote branch `feat/ui-redesign-themes` (`ddd862b`, an ancestor of `main` and the `v0.1.0` tag) and its state file.
- About 20.3 GB of disk space was freed on C:.
- Kept: tags and releases, `assets/`, the user's `%APPDATA%\Agent Usage Monitor`, the Kilo worktree, and the ignored local provider PNG overrides.

## WinGet submission (2026-11-21)
- PR https://github.com/microsoft/winget-pkgs/pull/486060 — "New package: ValentynFetisov.AgentUsageMonitor version 0.1.1", from the fork `valenticus/winget-pkgs`, branch `ValentynFetisov.AgentUsageMonitor-0.1.1` (commit `66e6ce3`), targeting `master`.
- Manifests (schema 1.28.0, `winget validate` passed) are kept in `C:\project\winget-aum\manifests\v\ValentynFetisov\AgentUsageMonitor\0.1.1\`. The sparse clone is in `C:\project\winget-pkgs`.
- Status checked 2026-11-22: PR remains open and is awaiting manual repository-admin review. The CLA requirement was cleared. Automated validation reported a false positive for Policy-Test-1.2 (the word “reset” matched “ESET”); a maintainer noted the false positive and requested a waiver. Do not describe the PR as merged or fully validated until its current checks and review are complete. Local `winget install --manifest` was not tested, to protect the existing installation.

## Microsoft Store / MSIX feasibility audit (2026-11-21; implemented and merged in PR #2, see "v0.2.0")
Store identity: Name `ValentynFetisov.AgentUsageMonitor`, Publisher `CN=090E947F-DD2D-4592-BD86-2187AAA0F9BA`, PublisherDisplayName `Valentyn Fetisov`, PFN `ValentynFetisov.AgentUsageMonitor_sfbqfhxtgjc58`, Store ID `9N20DHG8SSS1`. Planned package version: `0.1.1.0`, x64, full-trust Win32 (`runFullTrust` only).

Verdict: packaging is feasible. MSIX is preferred over an EXE submission, because the Store requires EXE/MSI installers to be signed with a trusted code-signing certificate (the Inno setup is unsigned), while the Store signs MSIX packages itself. Tools present: SDK 10.0.26100 `makeappx`/`makepri`/`signtool` and the App Certification Kit.

| Area | Behaviour inside MSIX | Needed |
| --- | --- | --- |
| Tray icon, menu, background running | works (full trust) | nothing |
| Launch at sign-in (`autostart.rs`, HKCU `Run`) | **breaks**: HKCU writes go to the package's private registry | packaged-only branch using `Windows.ApplicationModel.StartupTask` plus a `windows.startupTask` manifest extension |
| Toasts (`notifications.rs`) | **likely breaks**: custom AUMID `Valenticus.AgentUsageMonitor`, the `SetCurrentProcessExplicitAppUserModelID` call and the HKCU `AppUserModelId` registration conflict with package identity | packaged-only branch: skip registration, use `CreateToastNotifier()` (package AUMID), no `appLogoOverride` file |
| Settings in `%APPDATA%\Agent Usage Monitor` | work, but writes are redirected to the package's private AppData; not shared with the Inno install; removed on uninstall | document |
| Codex `app-server` child process | probably works (`~/.codex` is not virtualized) | **untested**; child writes under `%APPDATA%` would be virtualized |
| Claude CLI `/usage` child process | probably works (`~/.claude` is not virtualized) | **untested** |
| Claude status-line bridge | packaged app cannot receive `--enable-claude-bridge` on Windows 10 1809 (`uap10:Parameters` needs 2004+); Claude Code also cannot run an EXE inside `WindowsApps` directly | App Execution Alias and Store-only, off-by-default Settings toggle selected and implemented in the MSIX worktree; still requires compile and clean-VM testing |
| Single instance (`Local\AgentUsageMonitor.Singleton`) | works, **shared with the Inno install**: only one of the two can run | document |
| Updates | through the Store; applied when the app is closed | nothing |
| Windows 10 1809+ / 11 x64 | supported (desktop bridge, startupTask, uap3 alias) | `MinVersion 10.0.17763.0` |

Decisions since the audit:
1. **Claude bridge: option (a) selected**: an off-by-default Settings toggle in the Store build only.
2. **Store identity confirmed**: reserved name `AgentUsageMonitor`.
3. **Still open: no isolated install testing on this PC.** It runs Windows 11 Home (no Sandbox, no VM). Installing the MSIX next to the Inno install would share the mutex and the settings view, and a full WACK run also installs the package. A clean Windows 10 1809+ and Windows 11 VM or a second PC is needed.

MSIX implementation is **complete** and merged (PR #2, `89def68`). Tests, Clippy, MSIX build, WACK (22 PASS / 1 optional FAIL "Blocked executables" / 1 DPI WARNING) and runtime checks on this PC were done; a clean VM / second PC was not available. Details: `docs/state/feat-store-msix.md`, architecture and build steps: `docs/msix.md`. The table above is the pre-implementation audit.

## Decisions (and why)
- Provider marks: tracked approved SVGs (`rust/agent-monitor-ui/assets/providers/`) are embedded by `build.rs`; provenance and approval are in `docs/design/brand-assets.md`. Local overrides in `%APPDATA%\Agent Usage Monitor\provider-icons` win over bundled files; build-time overrides apply only with `AUM_PROVIDER_ICONS_DIR`. All sources are trimmed to visible artwork; the optical scale is per provider (Codex 0.90, Claude 1.00).
- Release size: `opt-level = "s"` without LTO, chosen from the benchmark above.
- Claude integration stays opt-in: StatusLine bridge plus optional `claude -p --no-session-persistence /usage` refresh (90 s timeout, hidden process, process-tree cleanup, 5 min success interval, 10 min failure cooldown). Undocumented CLI text is parsed best-effort; unknown values stay unavailable. Disabling during a refresh cancels and awaits the child process.
- Tray startup falls back to the generated ICO and then the stock icon when the resource compiler is missing.
- Single-instance mutex `Local\AgentUsageMonitor.Singleton`: a dev build cannot start while the installed AUM runs. For visual QA, use the debug flags `--demo`, `--preview-theme=<graphite|bluish|light>`, `--preview-flyout` and `--preview-settings=<tab>` (in memory only) with an isolated `APPDATA`.
- Every AUM start rewrites `HKCU\Software\Classes\AppUserModelId\Valenticus.AgentUsageMonitor` `IconUri` to `<APPDATA>\Agent Usage Monitor\notification-icon.png`. A test run with an isolated `APPDATA` redirects it until the installed app starts again, so snapshot and restore it around tests.
- Packaging always goes into a new empty output folder (`build-installer.ps1 -OutputDirectory …`); the script refuses to overwrite.

## Next actions
1. Microsoft Store: submit 0.2.0.0 in Partner Center — only after explicit user approval (steps in `AUM-recovery\store-handoff\v0.2.0\STORE-HANDOFF.md`).
2. WinGet: PR #486060 is for 0.1.1; check its status (Policy-Test-1.2 false-positive waiver, admin review). A 0.2.0 manifest is a separate, not yet approved step.
3. Untested in v0.2.0: Inno installer on a clean machine, tray on a light taskbar, Explorer restart, launch at real sign-in, Windows S mode, provider icons at 125/150/200 %.
4. Deferred feature: configurable refresh interval (fixed 5 min now; see design spec).

# State: main

## Goal and constraints
- v0.1.0 is publicly released (tag `v0.1.0` → `ddd862b`). v0.1.1 artifacts are built and validated locally; v0.1.1 is not tagged, pushed or published yet.
- Do not modify the published v0.1.0 release or tag.
- Never run authenticated Claude requests, read credentials or transcripts, use private OAuth APIs, or change global Claude settings (see `AGENTS.md`).
- Commit and push only on explicit request.

## Done since v0.1.0
- Provider SVG normalization (`b297f0d`):
  - Cause: the OpenAI Blossom artwork fills 49.5 % of its 716-unit viewBox; the Claude Spark fills 99.6 % of its 94-unit viewBox. At the same box size Codex looked about half as large.
  - Fix: SVGs are rasterized with `resvg` (the same 0.49 crate Slint already builds) and go through the PNG trim → square → area-resample pipeline in `rust/agent-monitor-ui/src/provider_icons.rs`. The Codex-only 18 px flyout size was removed; every mark uses a 14 px box in the flyout and a 16 px box in Settings.
- Codex optical correction (`2b38b90`): after trimming, the dense closed Blossom still read slightly larger and heavier than the open Spark. Only the Codex master is padded so its artwork fills 90 % of the same box (`optical_scale`: Codex 0.90, Claude 1.0), centred for all themes and DPI. Claude rendering is unchanged.
- Release profile (`6453b6f`): `[profile.release] opt-level = "s"` in `rust/Cargo.toml`. Thin LTO was not adopted; no further size experiments are planned.
- Version 0.1.1 (`9f0f74d`): both crates, the two workspace entries in `Cargo.lock`, and the `AppVersion` fallback and manual-build example in `packaging/windows/AgentUsageMonitor.iss`. The app reads its version from `CARGO_PKG_VERSION` (About and the Codex `clientInfo`); the packaging script reads it from `agent-monitor-ui/Cargo.toml`. Historical v0.1.0 references in the design spec are unchanged.
- Design references: 10 reference images were added in `7b3e7c4` and removed as obsolete in `843747d`. `docs/design/AUM-design-spec-FINAL-v4.md` and the implemented UI are the design source of truth; the cardless flyout, Variant A, is implemented. App screenshots live in `assets/`.
- Repository cleanup: stale `battle-corleggy` worktree entry pruned; merged local branch `feat/ui-redesign-themes` deleted (the remote branch still exists). `CODEX-PROMPT-AUM.md` was moved to `C:\project\_archive\agent-usage-monitor\`.

## v0.1.1 artifact validation (2026-11-21, `-j 2`, built from `9f0f74d`)
- `cargo fmt --all --check` — passed.
- `cargo test --locked --release --workspace` — 104/104 passed (Core 31, bridge 3, bridge process 1, UI 69, doc 0).
- `cargo clippy --locked --workspace --all-targets --release -- -D warnings` — passed.
- `cargo build --locked --release --target x86_64-pc-windows-msvc -p agent-usage-monitor-ui -p agent-usage-monitor-core --bins` — passed.
- Packaging: `packaging/windows/build-installer.ps1 -SkipBuild` with Inno Setup 6 from `%LOCALAPPDATA%\Programs\Inno Setup 6`, always into a new empty folder (the v0.1.0 outputs in `rust/target/installer` are untouched).
- **Final artifacts** (after the README update `38b4393`) in `rust/target/installer-0.1.1-final/`:
  - `AgentUsageMonitor-0.1.1-win-x64-setup.exe` — 7,019,365 B, SHA-256 `1099D7223129C8199B413E94B711BFDF863335CCE3126DE0D20A9BCE5E337116`.
  - `AgentUsageMonitor-0.1.1-win-x64-portable.zip` — 6,696,634 B, SHA-256 `89B781D933EC1C684D31E4535689C97ADF1A275C1B7AA77A174AA7C1C88D17E9`.
  - The README in the ZIP and in the installer stage is byte-identical to the committed `README.md`.
- Superseded (old README inside): `rust/target/installer-0.1.1/` — do not publish.
- Binary sizes: `agent-usage-monitor.exe` 14,967,808 B (v0.1.0: 17,686,528 B, −15.4 %); `agent-usage-monitor-claude-bridge.exe` 377,856 B (v0.1.0: 442,368 B, −14.6 %).
- Package checks: the ZIP holds the two EXEs, `LICENSE` and `README.md`. Both EXEs in the stage and the ZIP are byte-identical to the fresh release build and differ from the v0.1.0 binaries. The app EXE contains `0.1.1` and both bundled provider SVGs. Setup version info: FileVersion 0.1.1.0, ProductVersion 0.1.1.
- Dependencies: since v0.1.0, `Cargo.lock` only gains a direct `resvg` edge (no new packages) plus the version bump. Since v0.1.0, `rust/` changes are limited to provider icon rendering, the flyout icon size, the release profile and the version, so no product functionality was removed.
- Smoke test (run for both package sets, last on the final ZIP): the portable EXE ran with `--demo` and an isolated `APPDATA` in the scratchpad. It created the flyout, Settings and tray-menu windows, was responsive (≈24 MB working set), wrote only to the isolated folder, and was then stopped. The installed v0.1.0 EXE and the user's `settings.json` were unchanged. The installer was not run, because it shares the AppId with the user's installed v0.1.0 and would upgrade it.
- Toast identity: every AUM start rewrites `HKCU\Software\Classes\AppUserModelId\Valenticus.AgentUsageMonitor` (`IconUri` = `<APPDATA>\Agent Usage Monitor\notification-icon.png`). The smoke test snapshotted and restored it. Before the test it already pointed to `%TEMP%\aum-preview-appdata\…` from an earlier isolated preview run; the installed app resets it on its next start.
- Visual: provider icons were checked on a debug build at 100 % only. The release build and 125/150/200 % are not visually checked.

## Release readiness
- README updated for v0.1.1 (`38b4393`); release notes and the proposed repo description were prepared outside the repo, in the session report.
- No hard blockers. Optional: a visual check of the release binary and at higher DPI.
- Decision pending: whether to remove the obsolete reference-image blobs from unpublished history before the first push (below).

## Unpublished history
- The 10 design reference images (7,106,066 B in 10 blobs) were added in `7b3e7c4` and removed in `843747d`. Neither commit is pushed, the blobs are not reachable from `origin/main`, `v0.1.0` or `origin/feat/ui-redesign-themes`, and the HEAD tree does not contain them. Pushing `main` as is would publish them permanently.
- Backup branch `backup/pre-history-cleanup-v0.1.1` was created before any history edit.
- Recommended (not done): rebuild the range on `b297f0d` without the images, verify the tree is identical to the backup, then move `main`. Commit SHAs from `1a0e1cb` on would change; SHAs quoted in this file must then be updated. The release binaries are unaffected (identical tree).

## Git state
- Branch: `main`. HEAD before the state commit: `38b4393` (`docs: update README for v0.1.1`).
- `main` is ahead of `origin/main` (`ddd862b`) and not pushed.
- Other worktrees: Kilo Code pool `flaxen-marmoset` (`ef5b3a5`, clean) and the Codex size benchmark at `%TEMP%\AUM-size-bench-ddd862b9-20261121` (`ddd862b`). Leave both untouched.

## Decisions (and why)
- Provider marks: tracked approved SVGs (`rust/agent-monitor-ui/assets/providers/`) are embedded by `build.rs`; provenance and approval are in `docs/design/brand-assets.md`. Local overrides in `%APPDATA%\Agent Usage Monitor\provider-icons` win over bundled files; build-time overrides apply only with `AUM_PROVIDER_ICONS_DIR`. All sources are trimmed to visible artwork; the Codex optical scale (0.90) is per provider and also applies to a local Codex override.
- Release size: `opt-level = "s"` without LTO, chosen from the Codex size benchmark.
- Claude integration stays opt-in: StatusLine bridge plus optional `claude -p --no-session-persistence /usage` refresh (90 s timeout, hidden process, process-tree cleanup, 5 min success interval, 10 min failure cooldown). Undocumented CLI text is parsed best-effort; unknown values stay unavailable. Disabling during a refresh cancels and awaits the child process.
- Tray startup falls back to the generated ICO and then the stock icon when the resource compiler is missing.
- Single-instance mutex `Local\AgentUsageMonitor.Singleton`: a dev build cannot start while the installed AUM runs. For visual QA, use the debug flags `--demo`, `--preview-theme=<graphite|bluish|light>`, `--preview-flyout` and `--preview-settings=<tab>` (in memory only) with an isolated `APPDATA`.

## Next actions
1. Decide on the unpublished-history cleanup (above); if approved, do it before the first push.
2. Optionally check the release binary visually (both themes; ideally 125/150/200 %).
3. When approved: push `main`, create the annotated tag `v0.1.1` on the final commit, push the tag, and publish the release with the two artifacts from `rust/target/installer-0.1.1-final/`.
4. Push `main` when approved; decide whether to delete `origin/feat/ui-redesign-themes` (and then remove `docs/state/feat-ui-redesign-themes.md`).

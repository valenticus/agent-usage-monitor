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
- Packaging: `packaging/windows/build-installer.ps1 -SkipBuild -OutputDirectory rust/target/installer-0.1.1` (a new folder; the v0.1.0 outputs in `rust/target/installer` were left untouched). Inno Setup 6 from `%LOCALAPPDATA%\Programs\Inno Setup 6`.
- Artifacts in `rust/target/installer-0.1.1/`:
  - `AgentUsageMonitor-0.1.1-win-x64-setup.exe` — 7,019,351 B, SHA-256 `974AAB21764B2D50A8CB37C71AC96F5A4EEF4A5934E02DD24FB11ADE8EBAC6D4`.
  - `AgentUsageMonitor-0.1.1-win-x64-portable.zip` — 6,696,606 B, SHA-256 `CA62135EE54B2767899E1D0FC5FDF5FCBC881BFA4DDACD1508A2BD4DB336AFD1`.
- Binary sizes: `agent-usage-monitor.exe` 14,967,808 B (v0.1.0: 17,686,528 B, −15.4 %); `agent-usage-monitor-claude-bridge.exe` 377,856 B (v0.1.0: 442,368 B, −14.6 %).
- Package checks: the ZIP holds the two EXEs, `LICENSE` and `README.md`. Both EXEs in the stage and the ZIP are byte-identical to the fresh release build and differ from the v0.1.0 binaries. The app EXE contains `0.1.1` and both bundled provider SVGs. Setup version info: FileVersion 0.1.1.0, ProductVersion 0.1.1.
- Dependencies: since v0.1.0, `Cargo.lock` only gains a direct `resvg` edge (no new packages) plus the version bump. Since v0.1.0, `rust/` changes are limited to provider icon rendering, the flyout icon size, the release profile and the version, so no product functionality was removed.
- Smoke test: the portable EXE from the ZIP ran with `--demo` and an isolated `APPDATA` in the scratchpad. It created the flyout, Settings and tray-menu windows, was responsive (≈24 MB working set), wrote only to the isolated folder, and was then stopped. The installer was not run, because it shares the AppId with the user's installed v0.1.0 and would upgrade it.
- Toast identity: every AUM start rewrites `HKCU\Software\Classes\AppUserModelId\Valenticus.AgentUsageMonitor` (`IconUri` = `<APPDATA>\Agent Usage Monitor\notification-icon.png`). The smoke test snapshotted and restored it. Before the test it already pointed to `%TEMP%\aum-preview-appdata\…` from an earlier isolated preview run; the installed app resets it on its next start.
- Visual: provider icons were checked on a debug build at 100 % only. The release build and 125/150/200 % are not visually checked.

## Release blockers for v0.1.1
- `README.md` still describes v0.1.0 as an unpublished release candidate ("The release is not public yet"), and both packages ship that README. After updating it, repackage (the binaries can be reused with `-SkipBuild`, into a new empty output folder).
- No visual check of the release binary or at higher DPI (optional before release).

## Git state
- Branch: `main`. HEAD before the state commit: `9f0f74d` (`chore(release): bump version to 0.1.1`).
- `main` is ahead of `origin/main` (`ddd862b`) and not pushed.
- Other worktrees: Kilo Code pool `flaxen-marmoset` (`ef5b3a5`, clean) and the Codex size benchmark at `%TEMP%\AUM-size-bench-ddd862b9-20261121` (`ddd862b`). Leave both untouched.

## Decisions (and why)
- Provider marks: tracked approved SVGs (`rust/agent-monitor-ui/assets/providers/`) are embedded by `build.rs`; provenance and approval are in `docs/design/brand-assets.md`. Local overrides in `%APPDATA%\Agent Usage Monitor\provider-icons` win over bundled files; build-time overrides apply only with `AUM_PROVIDER_ICONS_DIR`. All sources are trimmed to visible artwork; the Codex optical scale (0.90) is per provider and also applies to a local Codex override.
- Release size: `opt-level = "s"` without LTO, chosen from the Codex size benchmark.
- Claude integration stays opt-in: StatusLine bridge plus optional `claude -p --no-session-persistence /usage` refresh (90 s timeout, hidden process, process-tree cleanup, 5 min success interval, 10 min failure cooldown). Undocumented CLI text is parsed best-effort; unknown values stay unavailable. Disabling during a refresh cancels and awaits the child process.
- Tray startup falls back to the generated ICO and then the stock icon when the resource compiler is missing.
- Single-instance mutex `Local\AgentUsageMonitor.Singleton`: a dev build cannot start while the installed AUM runs. For visual QA, use the debug flags `--demo`, `--preview-theme=<graphite|bluish|light>`, `--preview-flyout` and `--preview-settings=<tab>` (in memory only) with an isolated `APPDATA`.

## Next actions
1. Update `README.md` for v0.1.1, then repackage into a new folder and record the new checksums.
2. Optionally check the release binary visually (both themes; ideally 125/150/200 %).
3. Tag and publish v0.1.1 only when approved.
4. Push `main` when approved; decide whether to delete `origin/feat/ui-redesign-themes` (and then remove `docs/state/feat-ui-redesign-themes.md`).

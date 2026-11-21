# State: main

## Goal and constraints
- v0.1.0 is publicly released (tag `v0.1.0` → `ddd862b`). Preparing the v0.1.1 release candidate; v0.1.1 is not tagged or published yet.
- Do not modify the published v0.1.0 release or tag.
- Never run authenticated Claude requests, read credentials or transcripts, use private OAuth APIs, or change global Claude settings (see `AGENTS.md`).
- Commit and push only on explicit request.

## Done since v0.1.0
- Provider SVG normalization (`b297f0d`):
  - Cause: the OpenAI Blossom artwork fills 49.5 % of its 716-unit viewBox; the Claude Spark fills 99.6 % of its 94-unit viewBox. At the same box size Codex looked about half as large.
  - Fix: SVGs are rasterized with `resvg` (the same 0.49 crate Slint already builds) and go through the PNG trim → square → area-resample pipeline in `rust/agent-monitor-ui/src/provider_icons.rs`. The Codex-only 18 px flyout size was removed; every mark uses a 14 px box in the flyout and a 16 px box in Settings.
- Codex optical correction (`2b38b90`): after trimming, the dense closed Blossom still read slightly larger and heavier than the open Spark. Only the Codex master is padded so its artwork fills 90 % of the same box (`optical_scale`: Codex 0.90, Claude 1.0), centred for all themes and DPI. Claude rendering is unchanged.
- Release profile (`6453b6f`): `[profile.release] opt-level = "s"` in `rust/Cargo.toml`. Thin LTO was not adopted; no further size experiments are planned.
- Design references: 10 reference images were added in `7b3e7c4` and removed as obsolete in `843747d`. `docs/design/AUM-design-spec-FINAL-v4.md` and the implemented UI are the design source of truth; the cardless flyout, Variant A, is implemented. App screenshots live in `assets/`.
- Repository cleanup: stale `battle-corleggy` worktree entry pruned; merged local branch `feat/ui-redesign-themes` deleted (the remote branch still exists). `CODEX-PROMPT-AUM.md` was moved to `C:\project\_archive\agent-usage-monitor\`.

## v0.1.1 release-candidate verification (2026-11-21, `-j 2`)
- `cargo fmt --all --check` — passed.
- `cargo test --locked --release --workspace` — 104/104 passed (Core 31, bridge 3, bridge process 1, UI 69, doc 0).
- `cargo clippy --locked --workspace --all-targets --release -- -D warnings` — passed.
- `cargo build --locked --release --target x86_64-pc-windows-msvc -p agent-usage-monitor-ui -p agent-usage-monitor-core --bins` (the packaging script's command) — passed in 4 m 37 s.
- Binary sizes (`rust/target/x86_64-pc-windows-msvc/release/`) vs the v0.1.0 staged binaries:
  - `agent-usage-monitor.exe`: 14,967,808 B (v0.1.0: 17,686,528 B, −15.4 %).
  - `agent-usage-monitor-claude-bridge.exe`: 377,856 B (v0.1.0: 442,368 B, −14.6 %).
- Binary checks: both bundled provider SVGs (Blossom White/Black, Claude Spark) are embedded; the binaries were built after the latest source change and the dep-info lists `provider_icons.rs`.
- Dependencies: since v0.1.0, `Cargo.lock` gains only a direct `resvg` edge from the UI crate; no new packages. Since v0.1.0, `rust/` changes are limited to provider icon rendering, the flyout icon size and the release profile, so no product functionality was removed.
- Visual: provider icons were checked on a debug build at 100 % only. The release build and 125/150/200 % are not visually checked.

## Release blockers for v0.1.1
- Version is still `0.1.0` in `rust/agent-monitor-core/Cargo.toml`, `rust/agent-monitor-ui/Cargo.toml` and the `AppVersion` fallback in `packaging/windows/AgentUsageMonitor.iss`, so About, artifact names and the installer would say 0.1.0.
- `README.md` still describes v0.1.0 as an unpublished release candidate ("The release is not public yet").
- No visual check of the release binary, and none at higher DPI.

## Git state
- Branch: `main`. HEAD before the state commit: `6453b6f` (`build: optimize release profile for size`).
- `main` is ahead of `origin/main` (`ddd862b`) and not pushed.
- Other worktrees: Kilo Code pool `flaxen-marmoset` (`ef5b3a5`, clean) and the Codex size benchmark at `%TEMP%\AUM-size-bench-ddd862b9-20261121` (`ddd862b`). Leave both untouched.

## Decisions (and why)
- Provider marks: tracked approved SVGs (`rust/agent-monitor-ui/assets/providers/`) are embedded by `build.rs`; provenance and approval are in `docs/design/brand-assets.md`. Local overrides in `%APPDATA%\Agent Usage Monitor\provider-icons` win over bundled files; build-time overrides apply only with `AUM_PROVIDER_ICONS_DIR`. All sources are trimmed to visible artwork; the Codex optical scale (0.90) is per provider and also applies to a local Codex override.
- Release size: `opt-level = "s"` without LTO, chosen from the Codex size benchmark.
- Claude integration stays opt-in: StatusLine bridge plus optional `claude -p --no-session-persistence /usage` refresh (90 s timeout, hidden process, process-tree cleanup, 5 min success interval, 10 min failure cooldown). Undocumented CLI text is parsed best-effort; unknown values stay unavailable. Disabling during a refresh cancels and awaits the child process.
- Tray startup falls back to the generated ICO and then the stock icon when the resource compiler is missing.
- Single-instance mutex `Local\AgentUsageMonitor.Singleton`: a dev build cannot start while the installed AUM runs. For visual QA, use the debug flags `--demo`, `--preview-theme=<graphite|bluish|light>`, `--preview-flyout` and `--preview-settings=<tab>` (in memory only) with an isolated `APPDATA`.

## Next actions
1. Bump the version to 0.1.1 (both crates, `Cargo.lock`, the installer fallback) and update `README.md`.
2. Visually check the release binary (both themes; ideally 125/150/200 %).
3. Package installer and portable ZIP, then tag and publish v0.1.1 only when approved.
4. Push `main` when approved; decide whether to delete `origin/feat/ui-redesign-themes` (and then remove `docs/state/feat-ui-redesign-themes.md`).

# State: main

## Goal and constraints
- v0.1.0 is publicly released (tag `v0.1.0` → `ddd862b`). Next planned release: v0.1.1.
- Do not modify the published v0.1.0 release or tag.
- Never run authenticated Claude requests, read credentials or transcripts, use private OAuth APIs, or change global Claude settings (see `AGENTS.md`).
- Commit and push only on explicit request.

## Done since v0.1.0
- Provider SVG normalization (`b297f0d`, `fix(ui): normalize provider SVG icon sizing`):
  - Cause: the OpenAI Blossom artwork fills 49.5 % of its 716-unit viewBox; the Claude Spark fills 99.6 % of its 94-unit viewBox. At the same box size Codex looked about half as large; the earlier Codex-only 18 px flyout size did not compensate.
  - Fix: SVGs are rasterized with `resvg` (same 0.49 crate Slint already builds) and go through the existing PNG trim → square → area-resample pipeline in `rust/agent-monitor-ui/src/provider_icons.rs`. The 18 px special case was removed; every provider mark is 14 px in the flyout and 16 px in Settings.
  - Optical factor after trimming ≈ 1.0 (convex hull 76.6 % vs 75.9 % of the box, ink 39.7 % vs 36.8 %), so there is no per-provider scale.
- Design references: the 10 reference images were added in `7b3e7c4` and then removed as obsolete. `docs/design/AUM-design-spec-FINAL-v4.md` and the implemented v0.1.0 UI are now the design source of truth; the cardless flyout, Variant A, is marked as implemented. Current app screenshots live in `assets/`.
- Repository cleanup: stale `battle-corleggy` worktree entry pruned; merged local branch `feat/ui-redesign-themes` deleted (the remote branch still exists). `CODEX-PROMPT-AUM.md` was moved to `C:\project\_archive\agent-usage-monitor\` (outside the repo).

## Verification
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- `cargo test --workspace`: 103/103 pass (Core 31, bridge 3, bridge process 1, UI 68).
- Icon appearance verified on real windows at 100 % scaling (Graphite and Light flyout, Settings → General → Providers): both marks 14×14 px (flyout) and 16×16 px (Settings) at the same x; provider names start at the same x; icon/name vertical centres within 0.5 px; no clipping.
- 125/150/200 % scaling is visually unverified (unit test `bundled_provider_marks_fill_the_same_visible_box` covers the sizes, but there are no screenshots).

## Git state
- Branch: `main`. Last code commit: `b297f0d`.
- HEAD before the state commit: `7b3e7c4` (`docs(design): add visual reference images for v4 spec`).
- `main` is ahead of `origin/main` (`ddd862b`) and not pushed.
- Other worktrees: Kilo Code pool `flaxen-marmoset` (`ef5b3a5`, clean); the Codex size benchmark at `%TEMP%\AUM-size-bench-ddd862b9-20261121` (`ddd862b`). Leave both untouched.

## Decisions (and why)
- Provider marks: tracked approved SVGs (`rust/agent-monitor-ui/assets/providers/`) are embedded by `build.rs`; provenance and approval are recorded in `docs/design/brand-assets.md`. Local overrides in `%APPDATA%\Agent Usage Monitor\provider-icons` win over bundled files. Build-time overrides apply only with `AUM_PROVIDER_ICONS_DIR`. All sources are trimmed to visible artwork so different padding never changes visible size.
- Claude integration stays opt-in: StatusLine bridge plus optional `claude -p --no-session-persistence /usage` refresh (90 s timeout, hidden process, process-tree cleanup, 5 min success interval, 10 min failure cooldown). Undocumented CLI text is parsed best-effort; unknown values stay unavailable. Disabling during a refresh cancels and awaits the child process to prevent overlapping results.
- Tray startup falls back to the generated ICO and then the stock icon when the resource compiler is missing.
- Single-instance mutex `Local\AgentUsageMonitor.Singleton`: a dev build cannot start while the installed AUM runs. For visual QA, use the debug flags `--demo`, `--preview-theme=<graphite|bluish|light>`, `--preview-flyout` and `--preview-settings=<tab>` (in memory only) with an isolated `APPDATA`.

## In progress
- A binary size benchmark is being run by Codex in its isolated worktree; results are pending. Do not interfere with its processes or files.

## Next actions
1. Review the Codex size benchmark results and decide what goes into v0.1.1.
2. Visually verify provider icons and layouts at 125/150/200 % scaling.
3. Push `main` when approved; decide whether to delete `origin/feat/ui-redesign-themes` (and then remove `docs/state/feat-ui-redesign-themes.md`).
4. Prepare the v0.1.1 release.

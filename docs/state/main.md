# State: main

## Goal and constraints
- v0.1.1 is published and is the latest release. v0.1.0 stays published unchanged.
- Do not modify the published v0.1.0 or v0.1.1 releases or tags.
- Never run authenticated Claude requests, read credentials or transcripts, use private OAuth APIs, or change global Claude settings (see `AGENTS.md`).
- Commit and push only on explicit request; never force-push.

## Releases
| Release | Tag → commit | Published (UTC) | Assets |
| --- | --- | --- | --- |
| v0.1.0 | `v0.1.0` → `ddd862b` | 2026-11-21 19:08:31 | setup 7,652,335 B; portable ZIP 7,821,645 B |
| v0.1.1 (Latest) | `v0.1.1` (annotated, `c82e1bc`) → `b24d207` | 2026-11-21 20:43:30 | see below |

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

## Git state
- Only branch: `main`, pushed normally (never forced) and in sync with `origin/main`. The remote has `main` only.
- The unpublished history was cleaned before the first push, so the obsolete design reference images were never published.
- Worktrees: the main checkout and the Kilo Code pool worktree `flaxen-marmoset` (`ef5b3a5`, clean). The Kilo worktree is managed by Kilo; leave it.

## Post-release cleanup (2026-11-21, done)
- Removed local build outputs: `rust/target/{debug,release,x86_64-pc-windows-msvc,tmp,installer,installer-0.1.1}`. Kept `rust/target/installer-0.1.1-final/`, a byte-identical copy of the published v0.1.1 assets. The next build of any kind starts from a clean cache.
- Removed the Codex benchmark worktree (`git worktree remove`) and the `%TEMP%\AUM-size-bench-ddd862b9-20261121` folder (results recorded above). Also removed `%TEMP%\aum-preview-appdata` and the Claude session scratchpad.
- Deleted the local branch `backup/pre-history-cleanup-v0.1.1`. All its commits are in `main` with identical patches, except the replaced image add/remove cycle. The reflog was not expired and no aggressive GC was run, so its objects stay until normal Git expiry.
- Deleted the merged remote branch `feat/ui-redesign-themes` (`ddd862b`, an ancestor of `main` and the `v0.1.0` tag) and its state file.
- About 20.3 GB of disk space was freed on C:.
- Kept: tags and releases, `assets/`, the user's `%APPDATA%\Agent Usage Monitor`, the Kilo worktree, and the ignored local provider PNG overrides.

## Decisions (and why)
- Provider marks: tracked approved SVGs (`rust/agent-monitor-ui/assets/providers/`) are embedded by `build.rs`; provenance and approval are in `docs/design/brand-assets.md`. Local overrides in `%APPDATA%\Agent Usage Monitor\provider-icons` win over bundled files; build-time overrides apply only with `AUM_PROVIDER_ICONS_DIR`. All sources are trimmed to visible artwork; the optical scale is per provider (Codex 0.90, Claude 1.00).
- Release size: `opt-level = "s"` without LTO, chosen from the benchmark above.
- Claude integration stays opt-in: StatusLine bridge plus optional `claude -p --no-session-persistence /usage` refresh (90 s timeout, hidden process, process-tree cleanup, 5 min success interval, 10 min failure cooldown). Undocumented CLI text is parsed best-effort; unknown values stay unavailable. Disabling during a refresh cancels and awaits the child process.
- Tray startup falls back to the generated ICO and then the stock icon when the resource compiler is missing.
- Single-instance mutex `Local\AgentUsageMonitor.Singleton`: a dev build cannot start while the installed AUM runs. For visual QA, use the debug flags `--demo`, `--preview-theme=<graphite|bluish|light>`, `--preview-flyout` and `--preview-settings=<tab>` (in memory only) with an isolated `APPDATA`.
- Every AUM start rewrites `HKCU\Software\Classes\AppUserModelId\Valenticus.AgentUsageMonitor` `IconUri` to `<APPDATA>\Agent Usage Monitor\notification-icon.png`. A test run with an isolated `APPDATA` redirects it until the installed app starts again, so snapshot and restore it around tests.
- Packaging always goes into a new empty output folder (`build-installer.ps1 -OutputDirectory …`); the script refuses to overwrite.

## Next actions
1. Optional: visually check provider icons at 125/150/200 % on the installed v0.1.1.
2. Plan the next version.

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

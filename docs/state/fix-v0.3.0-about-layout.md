# State: fix/v0.3.0-about-layout

_Handoff file for this branch, in the Agent Kit `handoff` skill format. Only this branch's agent edits it._

## Goal and constraints
- Fix the Settings → About update status layout shift for v0.3.0.
- Preserve the installed RC, provider behavior/settings, work-area sizing, small-display scrolling and DPI handling.
- Do not touch the Kilo-managed worktree or perform GUI/process manipulation.

## Done
<!-- each item: verified fact (with evidence) or assumption -->
- Inspected `main.slint` and settings sizing in `main.rs`: status visibility collapsed its row to 0 px when empty; non-empty statuses used 24–72 px based on wrapped text. About preferred height feeds shared settings sizing; its button position therefore moved with the status.
- Added one fixed 32 px update-status slot and an 8 px fixed gap before the update actions. Idle and available versions now use `CARGO_PKG_VERSION`; added translated idle labels in Ukrainian, English, Polish and Russian.
- Expanded Slint layout coverage for repeated Idle/Checking/Current/Available/Error transitions (including offline, HTTP, invalid metadata and timeout), all four locales, three themes and tab switching.

## Git state
<!-- HEAD, branch, `git status --short`, `git diff --stat` as captured -->
- Branch: `fix/v0.3.0-about-layout`, created from synchronized `main` (`8648085`).
- Pre-commit HEAD: `8648085`.

## Changed files
- `rust/agent-monitor-ui/ui/main.slint`
- `rust/agent-monitor-ui/src/main.rs`
- `rust/agent-monitor-ui/src/localization.rs`
- `docs/state/fix-v0.3.0-about-layout.md`

## Checks
<!-- commands run with results; not-run or stale checks named as such -->
- `cargo fmt --all --check` — passed.
- `cargo test --locked --workspace` — passed (45 Core, 3 bridge, 1 process integration, 152 UI; 201 tests total; doc tests passed).
- `cargo test --locked --release --workspace` — passed (same counts). Release test build reports three existing dead-code warnings in the QA preview code.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed.
- `cargo check --locked --workspace --target x86_64-pc-windows-msvc` — passed.
- `cargo check --locked --workspace --release --target x86_64-pc-windows-msvc` — passed.
- `git diff --check` — passed.
- Manual GUI verification was not performed; the installed RC and GUI were left untouched. Headless Slint tests are not visual confirmation.

## Decisions (and why)
- The status slot is fixed at 32 logical px with an 8 logical px action gap: sufficient for the concise localized status copy at the standard Settings content width while keeping the button close and stable.

## Blockers
- None known.

## Next actions
1. Commit and push this branch, then open a PR against `main` without merging.

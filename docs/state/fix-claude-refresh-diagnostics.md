# State: fix-claude-refresh-diagnostics

## Goal and constraints
- Improve Claude background refresh failure classification and localized diagnostics for a future AUM release.
- Never invoke authenticated Claude requests, inspect credentials/transcripts, modify Claude settings or hooks, or touch the installed Store package.
- Preserve partial quota results and keep CLI output, stderr, and session data out of logs and UI.
- Do not merge this branch automatically.

## Changes
- Added a diagnostic parser result that distinguishes unrecognized output from recognized labels without quota values, while the existing parser API retains its prior fallback behavior.
- Carried typed failure categories from process launch/output parsing through `ClaudeRefreshEvent` and the UI adapter. StatusLine bridge failures now have their own state and localized copy.
- Added Ukrainian, English, Polish, and Russian messages for refresh categories; successful partial quota data remains valid and absent windows remain unavailable.
- Added parser, process, event, adapter, and localization regression coverage, including empty versus unfamiliar nonempty output.
- Parser remains intentionally strict about existing `/usage` labels and `% used` formatting. No actual CLI request was made; compatibility with the installed CLI's live output remains unverified.

## Verification
- `cargo fmt --all --check` passed.
- `cargo test --locked -p agent-usage-monitor-core claude_usage::tests` passed (8 tests).
- `cargo test --locked -p agent-usage-monitor-ui claude_refresher::tests` passed (15 tests).
- `cargo test --locked -p agent-usage-monitor-ui claude_refresh_categories_survive_adapter_and_localize_separately_from_bridge_errors` passed (1 test).
- `cargo test --locked --workspace -- --test-threads=2` passed (core 45, bridge unit 3, bridge process integration 1, UI 152; doctests 0).
- `cargo clippy --locked --workspace --all-targets -- -D warnings` passed.
- `cargo check --locked --workspace --target x86_64-pc-windows-msvc` passed.
- `git diff --check` passed.

## Git state
- Feature branch created from current `origin/main`.
- Changes are intended for PR to `main`; not merged.
- Installed Microsoft Store AUM v0.2.0 is unchanged and does not include these diagnostics.

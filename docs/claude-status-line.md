# Claude Code integration notes

## Safe input

The official [Claude Code status-line documentation](https://code.claude.com/docs/en/statusline) describes a local command integration: Claude Code sends a JSON object to the configured command's standard input and renders its standard output. The documented input contains session and workspace metadata, context-window usage, cost, and (when available) rate-limit windows. Agent Usage Monitor parses only the JSON passed directly to the opted-in command and retains supported usage fields; it must not open `transcript_path`, inspect Claude settings or credential stores, or make network/API calls.

An additional, separate background refresh is available only when the user turns on **Background Claude refresh** in AUM settings. It runs the installed Claude CLI as `claude -p --no-session-persistence /usage`, parses only the reported session and all-model weekly quota lines, and never handles credentials or calls a private API itself. This is a CLI text-output integration and may stop working if Claude changes that output. It is disabled by default, uses a five-minute refresh interval and bounded retries, and retains old values as stale when it cannot get a valid report. No Claude settings are modified.

For Claude.ai Pro/Max sessions, `rate_limits` appears only after the first API response. `five_hour` and `seven_day` can be absent independently and expose `used_percentage` and `resets_at` (Unix epoch seconds). An expired window may be omitted. `spend_limit` is documented for Claude apps gateways, with version-dependent optional fields. Missing windows mean unavailable data, not zero usage. This gives a direct, local, documented source for usage and reset values without private APIs or credentials.

The status-line command is configured in user or project settings and executes as a shell command. Installation or configuration therefore requires a distinct, explicit user request. No Claude settings are changed by this milestone.

## Implemented opt-in bridge

The Rust bridge executable consumes the documented JSON passed on stdin,
normalizes only `rate_limits.five_hour` and `rate_limits.seven_day`, and sends a
bounded versioned-by-name JSON frame over the local Windows named pipe. Its
stdout contains only a small 5-hour/weekly percentage summary; if Agent Usage Monitor
isn't running, the command still exits successfully and prints that summary.
The desktop listener validates current-user access, payload size, percentage
ranges and timestamps before updating only Claude's provider state. The UI
marks the last snapshot stale after 15 minutes without new status-line input.

Enable the listener explicitly with `--enable-claude-bridge` and build the
`agent-usage-monitor-claude-bridge` executable. Configure it as Claude Code's documented
`statusLine` command only if no other command is in use or an explicitly approved,
command-specific wrapper is ready. The executable path and app startup options are in
[the Rust UI README](../rust/agent-monitor-ui/README.md). Agent Usage Monitor never
writes Claude settings.

Build the bridge from the repository root with:

```powershell
cargo build --manifest-path rust/Cargo.toml --bin agent-usage-monitor-claude-bridge
```

The standalone bridge command prints its own small usage summary to stdout. It does not
invoke or preserve an existing status-line command. Since status-line stdout replaces
the user's displayed line, do not replace an existing Nook command with the bridge
executable. There is no generic wrapper that can guarantee byte-for-byte behavior for
an arbitrary command. Any future composition must be user-authored or reviewed for the
specific command, reversible, and explicitly approved before settings are changed.

Alert deduplication state is persisted separately in the app-owned state file via an atomic same-directory replacement. The application should choose a private per-user data directory and apply appropriate filesystem permissions; this core accepts an explicit path and does not discover user directories or modify settings.

## Background CLI refresh

The CLI refresh toggle is off by default. When enabled and Claude monitoring is enabled, AUM starts the installed CLI with the exact `/usage` command, a 90-second timeout, process-tree cleanup, and a five-minute success interval (ten-minute cooldown after failures). It does not run tests by making authenticated CLI calls. The report parser requires a recognized timezone in the reset text; unknown or ambiguous timezone data is left unavailable. Each quota window retains its own timestamp and source, and only newer valid window values replace the current value. A failed or partial report never fabricates missing percentages or overwrites a newer window.

The CLI owns any authentication and network activity required to produce `/usage`; AUM does not read credentials or call a private OAuth endpoint. The command's human-readable output is not a documented stable API, and its use of a network request to obtain usage does not establish whether model tokens are ever consumed by every Claude Code version. Treat CLI refresh as best-effort and retain the status-line bridge as the documented primary source.

## Limits

- Status-line input is session-driven; it is not a standalone account-wide polling API.
- Background `/usage` retrieval depends on undocumented CLI output formatting and is not guaranteed across Claude Code versions or account types.
- Values may be absent before a session's first API response, for unsupported account types, or after a reset.
- The command receives other metadata including transcript path and session ID. A consumer must ignore these fields and never dereference the transcript path.
- Status-line integration feasibility is high for data supplied by a running Claude Code session; always display missing/stale values as unavailable.

# Claude integration research for Agent Usage Monitor

Checked 20 November 2026. Sources include Anthropic's published documentation and read-only inspection of source files in the five public repositories listed below. No user credentials, cookies, transcripts, local Claude configuration, or authenticated Anthropic endpoints were accessed. The OAuth endpoint review was source-code inspection only.

## Recommendation

Keep the opt-in, event-driven local status-line bridge as AUM's only Claude usage source. It consumes only the documented `rate_limits` fields passed on stdin by Claude Code. This gives real 5-hour and 7-day remaining values and their reset epochs when supplied, without AUM collecting authentication material. It is not an account poller: data is scoped to active sessions that invoke the command, may be absent, and becomes stale if no new status-line input arrives.

Do not add a headless `/usage` poll or direct OAuth endpoint client. Headless print mode is a documented way to run a prompt, but `/usage` as a non-interactive usage-reporting API is not documented. A real third-party project contains an experimental CLI attempt; that does not establish reliability or safety. Direct OAuth polling depends on an undocumented endpoint and reuses subscription credentials, which Anthropic's published policy does not permit third-party apps to collect, store, or intermediate.

## Method comparison

| Source | 5-hour | Weekly | Reset time | Account-wide | No active client | Refresh | Official support | Security / suitability |
|---|---|---|---|---|---|---|---|---|
| Claude Code `statusLine` JSON | Yes, `five_hour` when present | Yes, `seven_day` when present | Unix seconds per window | No guarantee; snapshot from an invoking session | No | Event-driven; optional `refreshInterval` while the session runs | **OFFICIALLY SUPPORTED** local command contract | Recommended. AUM keeps only allowlisted rate-limit values and receipt timestamp; missing fields remain missing. |
| Claude Code hooks | No documented quota fields | No documented quota fields | No documented reset fields | No | No | Lifecycle events | **OFFICIALLY SUPPORTED** hook mechanism, not a quota source | Not useful for quota monitoring. Do not infer rates from event payloads. |
| Headless CLI `claude -p /usage` | Source-code experiment parses text output | Source-code experiment parses text output | Not a documented machine-readable field contract | Unverified | Can launch a separate CLI process | Would require periodic CLI invocations | Print mode is supported; `/usage` in print mode is **UNVERIFIED** | Not suitable for AUM: can invoke a model/session path, make provider requests, and interact with local CLI auth. Never run it as a monitor poll. |
| `GET /api/oauth/usage` | Observed code reads 5-hour utilization | Observed code reads 7-day utilization | Observed ISO timestamps | Appears tied to subscription identity, not an official coverage guarantee | Technically polled without an active CLI in third-party code | Third-party polling; limits/behavior are undocumented and reports of 429 exist | **OBSERVED BUT UNDOCUMENTED** | Not suitable: bearer-token reuse, private contract, policy restrictions, instability, and no redistribution guarantee. AUM must not implement it. |
| Official Usage / Cost / Analytics APIs | No subscription rolling quota | No subscription rolling quota | No subscription reset timers | Organization API usage/cost | Yes, with organization API access | Historical analytics/report queries | **OFFICIALLY SUPPORTED** for organization API reporting | Not a replacement. Requires organization-level API authorization and reports API product activity, not personal subscription windows. |
| Claude Desktop / IDE integration | Only if an underlying session invokes documented status-line JSON | Same | Same | No guarantee | No | Host/session controlled | Host integration is official; a universal status-line invocation contract is not documented | Treat per-host delivery as **UNVERIFIED**. Do not inspect private app state. |
| Direct app state / transcript mining | Unverified | Unverified | Unverified | Unverified | Maybe | Unknown | **NOT SUITABLE** | Private, sensitive, unstable and outside AUM's data-source policy. |

### Client and platform coverage

| Host | Windows | Source coverage | No-client mode |
|---|---|---|---|
| Claude Code CLI | Supported; status-line commands use Git Bash when installed or PowerShell otherwise | Status-line docs define stdin JSON/stdout display and Windows path quoting | No |
| VS Code extension | Supported integration | Official docs describe VS Code integration, but do not promise the status-line command is called for every extension-hosted session; integrated-terminal CLI sessions can use CLI settings | No |
| Claude Desktop | Windows support exists for Desktop/Code workflows | Desktop docs do not define a separate quota API or guarantee status-line invocation for every Desktop mode; local Code may share Claude Code settings/engine | No |
| JetBrains | Plugin integration is documented | Plugin works with Claude Code; no independent quota contract. A CLI-launched session may invoke status line | No |
| Other IDEs | Depends on a Claude Code CLI session | No IDE-neutral quota integration documented | No |
| No client/session | N/A | No supported subscription-quota source found | No supported availability |

## Official status-line contract

Claude Code documents `rate_limits.five_hour.used_percentage`, `rate_limits.seven_day.used_percentage`, and each window's `resets_at` Unix epoch seconds. The percentages are **used**, so AUM computes remaining as `clamp(100 - used, 0, 100)`. The fields are only present for claude.ai Pro/Max/Ultra subscribers or some Claude apps gateway configurations, and only after the first API response; windows can be independently missing or dropped after reset. AUM must show unavailable for missing percentages and must not convert a missing percentage to zero.

Status-line commands receive JSON via stdin; stdout is rendered by Claude Code. Updates occur after session events and a window reset, with an optional `refreshInterval` that runs only while that session is active. Event updates can stop when a session is idle. On Windows the command runner uses Git Bash or PowerShell; official docs advise forward slashes in configured paths because Git Bash may treat backslashes as escapes.

The official [hooks reference](https://code.claude.com/docs/en/hooks) documents lifecycle JSON and command hooks, but it does not document subscription quota fields in those payloads. Hooks therefore cannot replace status-line rate-limit input.

## Headless CLI `/usage` feasibility

Official CLI docs define `claude -p "query"` as a non-interactive query through the SDK, then exit. They do not define `/usage` as a print-mode quota endpoint, a stable output schema, or a no-model/no-session operation. `/usage` is an interactive command and there is no published guarantee that placing it after `-p` returns quota data without prompt interpretation, model/API work, session side effects, or local authentication activity. AUM did not run such a command.

Source review found Clawdometer's `usage_refresher.rs` launching a periodic headless `claude -p /usage` process, parsing its text, and retrying. This is evidence of one project's experiment and its own environment-dependent behavior, not Anthropic's supported contract. Even with a 90-second timeout, its periodic subprocess strategy is not an acceptable production integration for AUM. Do not use it while VS Code/Desktop sessions are active or rely on it to refresh credentials. It cannot establish universal Windows, client, or plan coverage.

## OAuth endpoint findings (source inspection only)

`https://api.anthropic.com/api/oauth/usage` does not appear in the official public API documentation reviewed. Third-party source shows 0–100 `utilization` values, 5-hour and 7-day windows, and ISO-8601 `resets_at` values. It uses OAuth bearer authorization and a beta header; other source reads locally stored Claude OAuth credentials before making requests. That implementation pattern would require AUM to handle subscription credentials or rely on another process's token storage, which AUM's policy forbids. No authenticated request was sent and no credentials were read.

The direct endpoint has no public promise of account-wide scope, compatibility, stable refresh cadence, availability, or redistribution rights. Source and issue reports show rate-limit failures, reinforcing that it cannot be treated as a stable contract. OAuth quota polling is **OBSERVED BUT UNDOCUMENTED** and **NOT SUITABLE** for AUM.

Official Usage and Cost APIs report historical organization API usage and cost. They require organization API authorization and do not expose the personal Pro/Max rolling subscription windows or their reset times. The Agent SDK is an agent execution interface, not an account subscription quota API. Anthropic's published guidance says third-party developers may not offer Claude.ai login into their own app or collect, store, or intermediate Claude.ai credentials/session tokens.

## Source-code review of requested repositories

All five requested GitHub repositories exist and were checked using source files, not README descriptions alone:

| Repository | Source inspected | Verified approach | Relevance to AUM |
|---|---|---|---|
| [cthpAiden/Clawdometer](https://github.com/cthpAiden/Clawdometer) | [`usage_refresher.rs`](https://github.com/cthpAiden/Clawdometer/blob/main/app/src-tauri/src/usage_refresher.rs), [`hook.rs`](https://github.com/cthpAiden/Clawdometer/blob/main/crates/core/src/hook.rs) | Implements a status-line hook path and a separate experimental background CLI `/usage` text parser. Its source describes the headless path as periodic and environment-sensitive. | Confirms status-line bridge is a useful local pattern; does not make CLI `/usage` officially supported. |
| [nate-i-think/claude-usage-tracker](https://github.com/nate-i-think/claude-usage-tracker) | [`fetch.rs`](https://github.com/nate-i-think/claude-usage-tracker/blob/main/src/fetch.rs), [`poller.rs`](https://github.com/nate-i-think/claude-usage-tracker/blob/main/src/poller.rs) | Poller fetches the OAuth usage URL and supports profile polling; fetch layer handles OAuth response fields. | Direct credential-backed undocumented endpoint; outside AUM's approved boundary. Credential source files were not opened. |
| [paris-paraskevas/claude-usage-tracker](https://github.com/paris-paraskevas/claude-usage-tracker) | [`relay/src/index.js`](https://github.com/paris-paraskevas/claude-usage-tracker/blob/main/relay/src/index.js), [`relay/src/widget.js`](https://github.com/paris-paraskevas/claude-usage-tracker/blob/main/relay/src/widget.js) | Relay source describes pushed snapshots, a remote team connector, and an encrypted phone-sync path; it is a cloud relay architecture rather than an official personal quota API. | Adds network/service, account, and key-management complexity; not a fit for AUM's local-only bridge. |
| [chunnylab/agent-usage-widget](https://github.com/chunnylab/agent-usage-widget) | [`src/usage.js`](https://github.com/chunnylab/agent-usage-widget/blob/main/src/usage.js) | Source explicitly reads Claude's local OAuth credential file and calls the OAuth usage URL. | Do not reproduce: credential file access and undocumented endpoint conflict with AUM policy. |
| [nerlichman/claudebar](https://github.com/nerlichman/claudebar) | [`OAuthUsageFetcher.swift`](https://github.com/nerlichman/claudebar/blob/main/Sources/ClaudeBar/Usage/OAuthUsageFetcher.swift), [`StatuslineUsageReader.swift`](https://github.com/nerlichman/claudebar/blob/main/Sources/ClaudeBar/Usage/StatuslineUsageReader.swift) | macOS menu-bar source contains both direct OAuth usage fetching and a local status-line reader. | The status-line reader is the relevant pattern; the OAuth fetcher is undocumented and not for AUM. No transcript content was inspected. |

Popularity, licensing, or public source availability does not establish official Anthropic support or permission to redistribute a credential-handling design.

## Safe AUM architecture

1. Claude Code runs a user-opted-in status-line command and passes its documented input JSON.
2. The command normalizes only 5-hour and seven-day percentages/reset epochs plus the local snapshot timestamp.
3. It forwards a bounded frame to AUM's current-user-only Windows named pipe; it never forwards raw input, session/workspace fields, transcript paths, or credential material.
4. AUM validates fields and updates Claude independently from Codex. Missing values remain unavailable; no receipt for 15 minutes makes the last-known data stale.
5. AUM does not poll Claude, perform OAuth, read settings, or change a Claude command automatically.

An opt-in compatibility wrapper for an existing arbitrary `statusLine` command cannot guarantee exact stdin, stdout bytes, exit code, stderr, quoting, or timeout behavior. AUM therefore should offer only a user-reviewed, reversible, command-specific composition after explicit consent; it must preserve the original command and not depend on Nook. No global or project Claude setting was modified during this research.

## Classification and sources

- **OFFICIALLY SUPPORTED:** status-line JSON as a local session integration; hooks as lifecycle mechanisms; organization Usage and Cost APIs for API reporting; published CLI print mode for prompts.
- **OBSERVED BUT UNDOCUMENTED:** `/api/oauth/usage` direct polling; headless `/usage` output parsing.
- **UNVERIFIED:** quota coverage/invocation for all VS Code, Desktop, JetBrains, and other IDE modes; headless `/usage` output stability, model/session side effects, and active-client safety; standalone account-wide subscription polling.
- **NOT SUITABLE:** reading local credentials, cookies, transcripts, app databases, private endpoints, or periodic headless polling as an AUM quota source.

Primary documentation:

- [Claude Code status line](https://code.claude.com/docs/en/statusline)
- [Claude Code hooks](https://code.claude.com/docs/en/hooks)
- [Claude Code CLI reference](https://code.claude.com/docs/en/cli-reference)
- [Claude Code settings scopes](https://code.claude.com/docs/en/settings)
- [Claude Code VS Code integration](https://code.claude.com/docs/en/vs-code)
- [Claude Code Desktop](https://code.claude.com/docs/en/desktop)
- [Claude Code JetBrains integration](https://code.claude.com/docs/en/jetbrains)
- [Claude Code Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview)
- [Claude Code legal and compliance](https://code.claude.com/docs/en/legal-and-compliance)
- [Usage and Cost API](https://platform.claude.com/docs/en/manage-claude/usage-cost-api)
- [Usage and Cost Analytics API](https://platform.claude.com/docs/en/manage-claude/usage-cost-analytics-api)

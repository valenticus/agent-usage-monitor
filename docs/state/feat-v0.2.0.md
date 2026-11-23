# State: feat/v0.2.0

## Goal and constraints
- AUM v0.2.0 release features on top of merged `main` `89def68` (PR #2, MSIX). Focused scope, no expansion.
- Do not modify the installed (test-signed) MSIX AUM or live Claude/Codex/Nook/Agent Kit settings. Tests use temporary fixtures only.
- No commit, push, tag, release or Store submission without explicit approval.

## Decisions (and why)
- **F-8 "one-click Claude Bridge" implemented as GUIDED, NO WRITES** (user decision, 2026-11-23).
  - Reason: AGENTS.md forbids changing global Claude settings, and the UI and docs promise "AUM never writes Claude settings".
  - AUM reads only the `statusLine` member of the user settings file. It shows the state and offers a **Copy setting** button only when the status line is free.
  - Another tool's status line (Nook here) is reported as a conflict; Background refresh is suggested instead.
  - Nothing is written, chained or replaced. Backups, atomic writes and restore logic are therefore not needed.
- **F-10:** warning changed from yellow `#FACC15` to **amber**. Bar, text, chip, legend and tray use one hue per state: green / amber / red. Text uses a darker shade on Light and a brighter one on the dark themes. The tray keeps one palette for both taskbars; the old `dark_taskbar` flag is the app theme, not the taskbar. Threshold logic is unchanged.
- **F-7:** the stale tray bar was drawn with every third row skipped, which read as stripes at 16–20 px. It is now one solid muted fill (`TRAY_STALE`). The compact flyout is unchanged.

## Done (in the v0.2.0 release-candidate commit)
- **F-7** `tray.rs`: solid stale fill; test `stale_bars_are_solid_and_muted_at_every_size` (16/20/24/32 px, 5–100 %).
- **F-10** `ui/main.slint` (`Theme.level-bar/level-text/level-chip`) and `tray.rs` (`TRAY_WARNING` amber). Test `bar_text_and_tray_colours_share_one_hue_per_state` reads the colours from `main.slint`: bar, text and tray per state within 15° hue, and warning is amber (30–46°).
- **F-8 guided:**
  - new `claude_statusline.rs`: read-only, bounded (1 MiB), BOM-tolerant; only `statusLine` is kept; another tool is shown by program name only; Win32 clipboard copy.
  - Settings → Claude card: "Claude Code status line" row, plus Copy setting when free.
  - `main.rs`: the file is read when Settings opens, never on each UI refresh; pure `statusline_row`.
  - `windows-sys` features `Win32_System_DataExchange`, `Win32_System_Memory`; no new crates.
  - Tests cover: missing, empty and unconfigured files; AUM in alias, bare, path and quoted forms; Nook conflict with no secret or argument leak; chained command not treated as AUM; unexpected shapes; malformed, oversized and directory paths; BOM; the file never modified; snippet valid JSON and recognised as AUM; row mapping in 4 locales.
- **Settings polish:**
  - The Claude bridge description and the source note no longer repeat "AUM does not change Claude settings"; one concise note remains: "AUM only reads the statusLine setting and never changes Claude settings".
  - The "Setup required" description is shortened.
- **Clean install UX:**
  - Codex without a CLI: "CLI not found / Install the Codex CLI and run codex login, then refresh limits", instead of "app-server has not returned data".
  - Claude with Background refresh on and no CLI: the new `cli_missing` flag gives "CLI not found / Install Claude Code and sign in (claude), or turn off Background refresh" instead of the generic "Error".
  - Status-line data, if any, is never overwritten by that state. No zero values; nothing is installed.
  - Test `missing_clis_show_not_installed_with_instructions_and_no_values`.
- **Docs:** `docs/claude-status-line.md` describes the read-only statusLine check.

## Verification (2026-11-23, this machine, battery/AC as available)
| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | pass. The first runs failed on a wrong `GlobalFree` import path and on `bool_assert_comparison` in a new test; both fixed |
| `cargo test --locked --workspace` | **169/169**: core 43, bridge 3, bridge integration 1, UI 122 (+11 new UI tests, 2 updated) |
| Translation coverage | `every_supported_locale_has_complete_settings_copy` extended with the 10 new statusLine strings: pass (UA/EN/PL/RU) |
| Release build `cargo build --locked --release --target x86_64-pc-windows-msvc` (the Inno/MSIX build command) | pass; GUI 15,357,952 B with the PerMonitorV2 manifest (checked with `mt.exe`) |
| `packaging\msix\build-msix.ps1 -ValidateManifestOnly` | pass |
| `Cargo.lock` | unchanged; only `windows-sys` features were added |
| Live state | installed MSIX unchanged (GUI `0BAEF71D…`, 1 process); the live `~/.claude/settings.json` was not opened by the tests and was last modified 2026-11-14 |

Not run here:
- The new UI was not shown at runtime. A second AUM instance would exit on the single-instance mutex, and replacing the installed MSIX needs approval. Not seen yet: amber and stale tray bars on real light/dark taskbars, the statusLine row, the Copy setting button with the clipboard, and the new "CLI not found" texts.
- Explorer restart with the new tray: not needed for F-7, because the icon registration code is unchanged; only pixel rendering changed.

Expected F-8 result on this machine: the user's `statusLine` is Nook's `nook-hook-standalone.exe` (recorded during MSIX QA as F-3). The row will say "Used by another command (nook-hook-standalone.exe)… use Background refresh instead", with no Copy button and no change to the file. The fixture test with the same command shape covers this.

## Codex review 1 (verdict FAIL): fixes (in the v0.2.0 RC commit)
1. **Clipboard (critical).** `copy_to_clipboard(owner, text) -> Result<(), ClipboardError>`:
   - The owner is the Settings window's `HWND`; with no owner AUM refuses to open the clipboard at all.
   - The text is prepared in movable global memory before the clipboard is opened. `OpenClipboard` is retried up to 10 × 15 ms.
   - `EmptyClipboard` is called only after a successful open. The memory is freed on every path except a successful `SetClipboardData`. Open and close are balanced.
   - The button shows "Copied" or "Copy failed" (4 languages) according to the real result.
2. **Shell command (critical).** `suggested_command(…) -> Result<String, CommandError>` has one fixed shape:
   - the Store alias, or `"C:/…/agent-usage-monitor-claude-bridge.exe"`, always double-quoted, with forward slashes and no arguments;
   - an allow-list of Unicode letters and digits, space and `/ : . _ - ( ) + , @`. That is literal inside double quotes for bash (Git Bash, Claude Code's shell on Windows) and for cmd.exe;
   - everything else, including `& ; $ % ! ^ \` ' " | < > * ? ~ #` and control characters, is refused with `UnsupportedPathCharacter`. The row then says the path is unsupported and offers no Copy;
   - snippets are never executed in tests.
3. **Bounded read (minor).** One handle with `take(MAX + 1)`: exactly 1 MiB is accepted, 1 MiB + 1 byte and 8 MiB are refused. Missing file, BOM, malformed JSON, unknown fields and the Nook shape behave as before.
4. **Version 0.2.0.**
   - `agent-monitor-core` and `agent-monitor-ui` `Cargo.toml` and `Cargo.lock` (local packages only).
   - The Inno `.iss` default and comment (BOM kept).
   - `docs/msix.md` example.
   - The MSIX version (`0.2.0.0`), the installer and portable file names and About are derived from the UI `Cargo.toml`. CI has no version. README release links stay at v0.1.1 until the v0.2.0 release.

### Validation after the fixes (2026-11-23)
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: pass. The first run flagged the non-Windows-only `ClipboardError::Unsupported` as dead code; it is now `cfg(not(windows))`.
- `cargo test --locked --workspace -- --test-threads=1`: **173/173** (core 43, bridge 3, integration 1, UI 126).
  - New: `settings_read_is_strictly_bounded`, `bridge_paths_with_shell_metacharacters_are_refused_not_escaped`, `suggested_command_has_one_fixed_quoted_shape`, `snippet_is_valid_json_and_recognised_as_aum`, `clipboard_never_changes_without_a_valid_owner` (non-destructive; checks the clipboard sequence number).
  - Extended: `statusline_row_states_and_copy_offer` (unsupported path).
- Release build `--target x86_64-pc-windows-msvc`: pass, no warnings.
- `build-msix.ps1 -ValidateManifestOnly`: **`AgentUsageMonitor MSIX 0.2.0.0 (x64)`**, identity unchanged.
- Not touched: the installed MSIX (GUI `0BAEF71D…`) and the live `~/.claude/settings.json` (last modified 2026-11-14).
- Known, pre-existing and not in scope: the EXEs carry no Win32 VERSIONINFO resource. File Explorer shows no file version; the installer's own VersionInfo comes from `.iss`.

## Codex focused re-review: PowerShell blocker (fixed, in the v0.2.0 RC commit)
- **Confirmed fixed by Codex:** clipboard ownership and errors, the bounded read, version 0.2.0 / 0.2.0.0.
- **Root cause.** Claude Code docs: on Windows, status lines run "through Git Bash when Git Bash is installed, or through PowerShell when Git Bash is absent"; Git for Windows is optional. The generated `"C:/…/bridge.exe"` is a string expression in PowerShell, so it is printed and not run. Measured: Git Bash ran it; Windows PowerShell 5.1 and PowerShell 7 only printed the path.
- **Fix.** One shape that runs in all three shells: the drive letter unquoted, the rest of the path single-quoted, e.g. `C':/Program Files/AUM/agent-usage-monitor-claude-bridge.exe'`.
  - Same allow-list as before. The path must now be on a drive letter (`X:/`); UNC and other prefixes give `UnsupportedLocation`.
  - The Store alias stays a bare word; checked to start in Git Bash, PowerShell 5.1 and 7.
  - Also measured: the plain single-quoted form fails in PowerShell, and a `powershell -Command "& '…'"` wrapper works but would start an extra PowerShell process for every status-line update.
- **Recognition.** `program_name` now reads the first word the way the shells join it (quoted segments, an optional PowerShell `&`). AUM's own generated command, including folders with spaces, is therefore recognised as "set up for AUM". Nook detection is unchanged.
- **New tests:** `generated_command_runs_in_every_available_shell` runs the generated command with stdin through every shell found (here Windows PowerShell 5.1, Git Bash and PowerShell 7 via the Store alias). It also asserts that the old double-quoted form does **not** run in PowerShell. The bridge is a temporary `sort.exe` copy in `plain` and `AUM dir (x86) Іван+v2,@x`.
- **Updated tests:** the fixed shape, refused UNC and verbatim prefixes, recognition of `C':/…'` and `& '…'`, and the metacharacter allow-list.
- **Docs:** `docs/claude-status-line.md` now describes Git Bash or PowerShell; the "Git Bash is required" claim is removed.
- **Validation:**
  - `cargo fmt --check` and `cargo clippy --workspace --all-targets -D warnings` pass. One `while_let_on_iterator` lint in the new word splitter was fixed.
  - `cargo test --workspace -- --test-threads=1`: **174/174** (core 43, bridge 3, integration 1, UI 127).
  - Release build `--target x86_64-pc-windows-msvc`: pass.
  - The installed MSIX and the live `~/.claude/settings.json` were not touched. The temporary shell-probe files were deleted.

## F-11: tray column mapping (fixed, in the v0.2.0 RC commit)
- **Root cause.** `UiSnapshot::tray_value` took the minimum remaining percentage over a provider's primary and secondary windows (`reduce(f64::min)`), so a low weekly value replaced the 5-hour value. Example: Codex 100/94, Claude 96/44 showed 94/44 instead of 100/96.
- **Fix** (`adapter.rs`): each tray column is one fixed window (`TrayWindow::FiveHour` = primary, `TrayWindow::Weekly` = secondary):

  | Enabled | Left | Right |
  | --- | --- | --- |
  | Codex and Claude | Codex 5 h | Claude 5 h |
  | Codex only | Codex 5 h | Codex weekly |
  | Claude only | Claude 5 h | Claude weekly |
  | neither | unavailable | unavailable |

  - Missing or invalid windows make only their own column unavailable.
  - Remaining = 100 − used.
  - Freshness (fresh / stale / unavailable) and F-7 rendering are unchanged.
  - The flyout, provider toggles and thresholds are unchanged.
- **Tests.** The old `tray_state_uses_each_providers_tightest_window_…` was replaced by:
  - `tray_columns_follow_the_fixed_window_contract`: the 100/94 + 96/44 example in all 4 modes, column order, and different weekly values;
  - `missing_windows_affect_only_their_own_column`: missing primary or secondary, a provider without data, and invalid percentages;
  - `stale_last_known_good_values_keep_their_window`: stale in every mode, and last-known-good after an error.
- **Validation:** fmt and clippy (`-D warnings`) pass; **176/176** (core 43, bridge 3, integration 1, UI 129) with `--test-threads=1`; release build passes.

### Manual clipboard smoke test (not automated: it replaces the user's clipboard)
1. Run the v0.2.0 build on a machine where `statusLine` is free (or in a test profile with `CLAUDE_CONFIG_DIR` pointing to an empty folder).
2. Settings → General → Claude: the row says "Not set up" or "settings not found" and shows **Copy setting**.
3. Click it. The button shows "Copied" for 2 s. Paste into Notepad: you should get `"statusLine": { "type": "command", "command": "\"C:/…/agent-usage-monitor-claude-bridge.exe\"" }` (Store build: the alias).
4. Hold the clipboard open from another tool (for example a clipboard manager in a modal state) and click again. The button shows "Copy failed", and the previous clipboard content is still there.
5. With Nook in `statusLine`: the row shows the conflict text and there is **no** Copy button. `~/.claude/settings.json` is byte-identical before and after (compare the SHA-256).

## Next actions
1. Review the diff, then Codex review.
2. With approval: commit and push `feat/v0.2.0`.
3. Runtime check of the new UI on a test install (needs approval to replace the installed MSIX or to install a separate EXE build):
   - tray stale and amber colours on dark and light taskbars;
   - the statusLine row with Nook present.

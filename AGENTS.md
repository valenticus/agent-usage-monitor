# Agent Usage Monitor Rust project instructions

- Keep the Rust workspace under `rust/`; use stable Rust and preserve the package metadata.
- Keep provider protocols in the Core crate and UI behavior in the Slint crate.
- Keep Claude integration opt-in. Preserve the official local status-line JSON bridge and allow optional background retrieval through the installed Claude CLI's `claude -p --no-session-persistence /usage` command only when explicitly enabled by the user. Never read credentials/transcripts, access private OAuth APIs, issue model prompts, or change global Claude settings. The status-line listener is enabled only by `--enable-claude-bridge` (EXE builds) or, in the Microsoft Store (MSIX) build that cannot take launch arguments, by the off-by-default Settings toggle `claude_bridge_listener_enabled`.
- Keep one binary for all distributions; choose MSIX-specific behaviour at run time from package identity (`packaged::is_packaged()`), never by changing the EXE builds' behaviour (see `docs/msix.md`).
- Do not add installer or autostart behavior without an approved project scope.
- Never commit secrets, generated binaries, or local build output.

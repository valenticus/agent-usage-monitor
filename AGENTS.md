# Agent Usage Monitor Rust project instructions

- Keep the Rust workspace under `rust/`; use stable Rust and preserve the package metadata.
- Keep provider protocols in the Core crate and UI behavior in the Slint crate.
- Keep Claude integration opt-in. Preserve the official local status-line JSON bridge and allow optional background retrieval through the installed Claude CLI's `claude -p --no-session-persistence /usage` command only when explicitly enabled by the user. Never read credentials/transcripts, access private OAuth APIs, issue model prompts, or change global Claude settings.
- Do not add installer or autostart behavior without an approved project scope.
- Never commit secrets, generated binaries, or local build output.

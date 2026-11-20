# Agent Usage Monitor Rust project instructions

- Keep the Rust workspace under `rust/`; use stable Rust and preserve the package metadata.
- Keep provider protocols in the Core crate and UI behavior in the Slint crate.
- Keep Claude integration opt-in and limited to official local status-line JSON input. Never read credentials/transcripts or change global Claude settings.
- Do not add installer or autostart behavior without an approved project scope.
- Never commit secrets, generated binaries, or local build output.

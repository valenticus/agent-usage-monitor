# State: release/v0.3.0-prep

## Goal and constraints
- Resolve the approved AUM v0.3.0 release-readiness issues on a dedicated branch from current `origin/main`.
- Keep v0.3.0 unreleased. Do not create tags, publish releases, build final release packages, install packages, or modify the installed Microsoft Store package.
- Preserve runtime behavior; only adjust the `AboutPreview` conditional compilation gate needed by release tests.

## Starting point
- Branch `release/v0.3.0-prep` created from clean `origin/main` at `d91ab70` (PR #12 merge).
- `main` was synchronized with `origin/main` before branching.

## Progress
- Changed `AboutPreview`, its static storage, and its impl to compile under `cfg(any(debug_assertions, test))`, matching the release test references. `AboutPreviewStatus` and its helper functions already use that gate.
- Updated README v0.3.0 preparation badge/link, upcoming asset names, and update-check description while clearly identifying v0.2.0 as the latest published release.
- Updated `docs/state/main.md` with v0.3.0 unreleased status, v0.2.0 Store status, merged PRs through #12, and separate v0.4.0/v0.5.0 roadmap items.
- Updated the Inno Setup fallback and manual example to 0.3.0; clarified its comment refers to automatic update installation.

## Validation and next actions
- `cargo fmt --all --check` passed.
- `cargo test --locked --workspace` passed: Core 45, bridge unit 3, bridge process integration 1, UI 152, doctests 0.
- `cargo test --locked --release --workspace` passed with the same counts. Rust emitted dead-code warnings for the test-only `QA_ABOUT_PREVIEW` static, `AboutPreview.status` field, and `AboutPreview::from_args`; there were no test failures.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` passed.
- `cargo check --locked --workspace --target x86_64-pc-windows-msvc` passed.
- `cargo check --locked --release --workspace --target x86_64-pc-windows-msvc` passed.
- `git diff --check` passed.
- Final review: the Rust diff changes only cfg gates; documentation describes the upcoming asset names while keeping v0.2.0 as the latest published GitHub release; the Inno version fallback is 0.3.0.
- Commit, push, and open a PR to `main` after final status review; do not merge.
- No release packages, tags, credentials, or provider commands were touched.

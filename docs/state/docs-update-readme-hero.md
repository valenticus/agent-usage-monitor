# State: docs/update-readme-hero

## Goal and constraints
- Replace the README hero image with the user-supplied Downloads/hero.png byte-for-byte.
- Keep this docs-only; no README text or other assets changed.

## Done
- Replaced assets/hero.png with the supplied file, unchanged. SHA-256: 59CDB97BB2BD9D99DB577D94DDEA920ED066B710A580DB1D6B39456758BCF561.
- git diff --check passed.
- Commit 5ad0497ad5d32a55466f8412d2a24e7e0b8738a9 pushed.
- PR https://github.com/valenticus/agent-usage-monitor/pull/11 opened against main; not merged.

## Git state
- Branch: docs/update-readme-hero
- HEAD before this state update: 5ad0497ad5d32a55466f8412d2a24e7e0b8738a9

## Changed files
- assets/hero.png
- docs/state/docs-update-readme-hero.md

## Checks
- Source and destination SHA-256 equality: passed.
- git diff --check: passed.

## Decisions (and why)
- Kept assets/hero.png so the README link remains unchanged.
- PR #9 had already merged, so this update is in a new branch and PR.

## Blockers
- None.

## Next actions
1. Await PR review; do not merge without the user request.

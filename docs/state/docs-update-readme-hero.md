# State: docs/update-readme-hero

## Goal and constraints
- Replace the README hero image with the user-supplied Downloads/hero.png byte-for-byte.
- Keep the change on a docs-only branch; do not alter README text or other assets.

## Done
- Replaced assets/hero.png with C:/Users/fetis/Downloads/hero.png.
- SHA-256 match verified: 59CDB97BB2BD9D99DB577D94DDEA920ED066B710A580DB1D6B39456758BCF561.
- git diff --check passed.

## Git state
- Branch: docs/update-readme-hero
- Base HEAD: 9059caf2e1a4f9928555a1b2d99665d1bcfc9757 (updated main/origin/main after PR #9 merged).

## Changed files
- assets/hero.png
- docs/state/docs-update-readme-hero.md

## Checks
- Source/asset SHA-256 equality: passed.
- git diff --check: passed.

## Decisions (and why)
- Used the supplied file as-is and retained assets/hero.png so the README link stays unchanged.
- PR #9 was already merged, so this follow-up uses a fresh branch and PR.

## Blockers
- None.

## Next actions
1. Commit and push this replacement; open a PR targeting main.

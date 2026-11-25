# State: docs/unify-readme-screenshot-design

## Goal and constraints
- Replace README visuals with exact supplied archive originals; preserve README structure and content except the outdated screenshot note.
- Documentation and image asset changes only; no app code or UI edits.

## Done
- Replaced all 11 matching files under assets/ with corresponding ZIP originals; SHA-256 equality verified.
- Removed the obsolete v0.1.0 screenshot note from README.md; image paths and README structure remain unchanged.
- Verified each local README image link exists and git diff --check passes.
- Committed the work; branch push and PR creation are next.

## Git state
- Branch: docs/unify-readme-screenshot-design
- Base HEAD before this change: cff1a83941adc9765d2d1728cccb015cd6e1382b
- Working tree clean after commit.

## Changed files
- README.md
- assets/about.png, assets/hero.png, assets/languages.png, assets/main-dark.png, assets/main-light.png, assets/notifications.png, assets/settings-dark.png, assets/settings-light.png, assets/social-preview.jpg, assets/themes.png, assets/tray.png
- docs/state/docs-unify-readme-screenshot-design.md

## Checks
- SHA-256 equality against all 11 ZIP files: passed.
- Local README image path existence: passed.
- git diff --check: passed.

## Decisions (and why)
- Kept existing filenames and links to avoid unnecessary README edits.
- Replaced social-preview.jpg with the supplied counterpart because it is a matching repository image asset.

## Blockers
- None.

## Next actions
1. Push branch and open a PR targeting main.

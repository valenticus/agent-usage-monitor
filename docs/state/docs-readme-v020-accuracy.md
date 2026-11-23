# State: docs/readme-v020-accuracy

_Handoff file for this branch, in the Agent Kit handoff skill format. Only this branch's agent edits it._

## Goal and constraints
- Replace only the 10 showcase PNGs from the user-provided v0.2.0 archive, preserve app/source/release assets, commit and push to PR #5 branch, inspect CI and PR preview, do not merge.

## Done
- Confirmed source archive exists at C:\Users\fetis\Downloads\aum-showcase-v0.2.0.zip and contains all requested PNG files.
- Replaced exactly 10 showcase PNGs from the archive. README, icon, social preview, store assets and source are unchanged.
- Verified all 10 PNGs decode and dimensions; all README image references resolve.
- PR #5 was OPEN; windows CI was SUCCESS before this change. Post-push check remains pending.

## Git state
- HEAD before handoff commit: c8f32f695513594d8ea95b49646355b333671e6d
- Branch: docs/readme-v020-accuracy
- Status before commit:
M assets/about.png
 M assets/hero.png
 M assets/languages.png
 M assets/main-dark.png
 M assets/main-light.png
 M assets/notifications.png
 M assets/settings-dark.png
 M assets/settings-light.png
 M assets/themes.png
 M assets/tray.png
?? docs/state/docs-readme-v020-accuracy.md
- Diff stat:
assets/about.png          | Bin 319673 -> 300199 bytes
 assets/hero.png           | Bin 878566 -> 851520 bytes
 assets/languages.png      | Bin 585612 -> 650226 bytes
 assets/main-dark.png      | Bin 567374 -> 567182 bytes
 assets/main-light.png     | Bin 499562 -> 499415 bytes
 assets/notifications.png  | Bin 311380 -> 303662 bytes
 assets/settings-dark.png  | Bin 307041 -> 360892 bytes
 assets/settings-light.png | Bin 278819 -> 317517 bytes
 assets/themes.png         | Bin 426236 -> 425389 bytes
 assets/tray.png           | Bin 610381 -> 614005 bytes
 10 files changed, 0 insertions(+), 0 deletions(-)

## Changed files
- ssets/about.png, ssets/hero.png, ssets/languages.png, ssets/main-dark.png, ssets/main-light.png, ssets/notifications.png, ssets/settings-dark.png, ssets/settings-light.png, ssets/themes.png, ssets/tray.png
- docs/state/docs-readme-v020-accuracy.md

## Checks
- System.Drawing.Image.FromFile verified each PNG and reported dimensions (all decoded).
- README image reference scan: no missing referenced images.
- git diff --quiet -- README.md assets/icon.png assets/social-preview.jpg assets/store: unchanged.
- Post-push Actions and PR preview: pending.

## Decisions (and why)
- Used only the exact archive assets requested; excluded README-snippet.md, video files, icon and social-preview.
- Added this branch state file using the local Agent Kit template because global rules require a state file.

## Blockers
- None.

## Next actions
1. Commit the 10 PNG replacements and this state file, push branch.
2. Check updated PR #5 Actions and preview; report merge readiness if checks pass. Do not merge.
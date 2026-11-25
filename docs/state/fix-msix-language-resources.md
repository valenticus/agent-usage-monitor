# State: fix/msix-language-resources

## Goal and constraints
- Declare the four supported UI languages in the MSIX package manifest.
- No app translation, Rust/UI, package identity, publisher, version, signing, Store listing, installed app, or Store submission changes.

## Done
- Changed AppxManifest.xml to declare en-US, uk-UA, pl-PL, and ru-RU, with en-US first as the default language.
- Added build-time validation in build-msix.ps1 requiring this exact ordered language list. The existing CI MSIX validation invokes this script.
- Confirmed the UI language strings are embedded in Rust source; the package does not use language-specific PRI resource files.

## Git state
- Branch: fix/msix-language-resources
- Base HEAD: cff1a83941adc9765d2d1728cccb015cd6e1382b (main, equal to origin/main at task start).
- The README screenshots PR remains on its own branch.

## Changed files
- packaging/msix/AppxManifest.xml
- packaging/msix/build-msix.ps1
- docs/state/fix-msix-language-resources.md

## Checks
- XML parse and exact manifest language assertion: passed.
- packaging/msix/build-msix.ps1 -ValidateManifestOnly: passed; MakePri and MakeAppx created a temporary unsigned package using placeholder EXEs.
- git diff --check: pending final commit review.
- No Rust or full UI tests run; application code was not changed.

## Decisions (and why)
- Kept MakePri /dq en-US: it sets the default qualifier for PRI indexing and does not replace the manifest supported-language list. The app has no separate localized PRI assets.
- The manifest declaration and local package validation cannot establish Store recognition; that requires publishing/submitting a package containing this manifest.

## Blockers
- None.

## Next actions
1. Run final diff checks, commit and push, then open a PR targeting main.

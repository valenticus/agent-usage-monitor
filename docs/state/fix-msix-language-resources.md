# State: fix/msix-language-resources

## Goal and constraints
- Declare the four supported UI languages in the MSIX package manifest.
- No app translation, Rust/UI, package identity, publisher, version, signing, Store listing, installed app, or Store submission changes.

## Done
- Updated AppxManifest.xml to declare en-US, uk-UA, pl-PL, and ru-RU, with en-US first as the default language.
- Added build-time validation in build-msix.ps1 requiring this exact ordered language list; existing CI invokes this script.
- Kept MakePri /dq en-US because it is the default PRI qualifier; AUM uses compiled Rust UI strings and has no language-specific PRI resources.
- XML parse and exact language list check passed. build-msix.ps1 -ValidateManifestOnly passed using MakePri and MakeAppx with placeholder EXEs. git diff --check passed.
- Commit 5d875c1700804919eccaf5284f52efc48588bec2 pushed to the branch.
- PR https://github.com/valenticus/agent-usage-monitor/pull/10 opened against main; not merged.

## Git state
- Branch: fix/msix-language-resources
- HEAD before this state update: 5d875c1700804919eccaf5284f52efc48588bec2
- No application runtime or Store package installation/submission was performed.

## Changed files
- packaging/msix/AppxManifest.xml
- packaging/msix/build-msix.ps1
- docs/state/fix-msix-language-resources.md

## Checks
- XML parsing and language list assertion: passed.
- Existing manifest and identity validation: passed.
- MakePri indexing and MakeAppx packaging: passed.
- git diff --check: passed.
- Full Rust tests not run; application code was not changed.

## Decisions (and why)
- Keep en-US first because Microsoft documents the first manifest Resource as the default language.
- Do not modify MakePri configuration: /dq chooses the PRI default qualifier and does not replace the AppxManifest supported-language list.

## Blockers
- Store recognition is unverified until a package containing the changed manifest is submitted/published.

## Next actions
1. Await PR review. After merge, submit a new MSIX package for Store language recognition; no submission was made in this task.

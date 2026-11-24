# State: feat/v0.3.0

## Goal
- Implement AUM v0.3.0 F-9 Phase 1 on a dedicated feature branch.
- Checking and notification only; no downloads, installation, self-replacement, credential access, or changes to the Store submission.

## Starting point
- Branch created from clean, up-to-date `main` at `df64609`.
- `origin/main` fast-forward check: already up to date.
- `agent-init` was unavailable; repository `AGENTS.md` exists. State file created manually per global Agent Kit requirement.

## Progress
- Review findings D1-D7/R1-R6 implemented: removed compile-time distribution markers; runtime classification uses package identity, Unknown fails closed, and Installer uses the Inno uninstaller marker.
- SemVer precedence ignores build metadata and prerelease tags are rejected independently of the API flag.
- Added persisted automatic-check opt-out (off by default), last-success/last-attempt timestamps, periodic tray scheduling and manual status handling.
- Added bounded HTTPS metadata-only checks, four-language UI copy, checking/error/update states, release-page links, and README/privacy documentation.
- Review #2 N1–N5: WinHTTP timeout budget is applied to the request before each blocking phase, with one session budget for connection setup; 20-second deadline arithmetic has a controlled phase-budget test. Automatic scheduling treats timestamps more than five minutes in the future as invalid. Settings writes are serialized across UI callbacks and update-check timestamps, with a concurrent persistence regression test. Duplicate Store/Unknown status messages and unused update state/localization were removed.
- Validation after Review #2 fixes: fmt, all workspace tests (43 Core, 137 UI, 3 bridge, 1 integration, doctests), Clippy with `-D warnings`, Windows-target check, and `git diff --check` pass. Tests used `-j 2 -- --test-threads=2`; Git Bash/PowerShell shell integration passed under that load.
- MSIX manifest-only packaging validation passes with temporary placeholder executables; no install or Store submission was performed.
- No release, Store submission, installed app, credentials, or provider settings changed.

## Remaining / limitations
- F-9 Phase 2: authenticated/publisher-verified asset and checksum metadata as appropriate, download UX, explicit user-confirmed installer handoff, Store update pathway, rollout/error/retry decisions; no self-replacement should be assumed.
- Branch is ready for commit and pull request; this state file is included with the F-9 changes.

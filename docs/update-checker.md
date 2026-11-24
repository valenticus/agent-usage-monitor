# F-9 Phase 1: update discovery

AUM 0.3.0 can check public GitHub Release metadata and report a newer stable version in Settings → About. Checking is metadata-only: it never downloads a release asset, installer, or portable archive and never replaces or installs files.

## Network access and privacy

The checker requests `https://api.github.com/repos/valenticus/agent-usage-monitor/releases/latest` over HTTPS through Windows WinHTTP. The request uses the OS proxy configuration, one User-Agent, no redirects, a 20-second overall deadline, and a 1 MiB response limit. No credentials, tokens, CLI data, or personal settings are sent. GitHub will receive the request IP and standard HTTP metadata. A successful check date and the user's automatic-check opt-in are stored in the existing AUM settings JSON. Automatic checks are **off by default**; the user can enable them on About. Manual checks are always explicit.

## Scheduling and distributions

For Installer and Portable, an enabled automatic check runs shortly after startup if the 24-hour interval is due, then the tray process checks periodically and runs once the persisted interval has elapsed. Both last-success and last-automatic-attempt timestamps are persisted. A failed automatic request waits 24 hours before retrying, avoiding repeated requests while offline and after restarts. Manual checks bypass the automatic schedule; a successful manual check also advances the success time. A single in-flight request prevents duplicates.

MSIX is identified at runtime by package identity and never queries GitHub; About explains that Microsoft Store manages its updates. A package-identity error is treated as Unknown and fails closed, with GitHub checking unavailable. For confirmed unpackaged processes, the adjacent Inno `unins000.exe` identifies Installer; without it the app is classified as Portable. No compile-time distribution flags or second binary are used.

The checker compares SemVer precedence (build metadata is ignored), rejects prerelease version tags even if GitHub's prerelease boolean is inconsistent, excludes drafts and prereleases, constructs the release URL from the validated SemVer tag, and shows distinct offline, HTTP, timeout, and invalid-data states.

## Limitations

Phase 1 provides discovery and a release-page link only. It does not validate or download assets, check checksums/signatures, launch an installer, self-replace, or call Store update APIs. The automatic opt-in and check timestamps use existing settings storage and follow the app's distribution-specific data folder. A manual check may be repeated by the user; automatic requests remain limited by the persisted 24-hour schedule.

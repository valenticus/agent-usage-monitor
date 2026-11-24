//! Checks GitHub Releases metadata only. This module never downloads release assets.

use std::time::Duration;

pub const AUTOMATIC_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const CLOCK_SKEW_TOLERANCE_SECONDS: i64 = 5 * 60;

const RELEASE_PAGE: &str = "https://github.com/valenticus/agent-usage-monitor/releases";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

fn remaining_timeout_ms(
    started: std::time::Instant,
    deadline: Duration,
    now: std::time::Instant,
) -> Result<i32, CheckError> {
    let remaining = deadline
        .checked_sub(now.saturating_duration_since(started))
        .ok_or(CheckError::Timeout)?;
    if remaining.is_zero() {
        return Err(CheckError::Timeout);
    }
    Ok(remaining.as_millis().clamp(1, i32::MAX as u128) as i32)
}

pub fn automatic_check_due(
    now_unix: i64,
    last_success_unix: Option<i64>,
    last_attempt_unix: Option<i64>,
) -> bool {
    let interval = AUTOMATIC_CHECK_INTERVAL.as_secs() as i64;
    if timestamp_is_recent(now_unix, last_attempt_unix, interval) {
        return false;
    }
    !timestamp_is_recent(now_unix, last_success_unix, interval)
}

fn timestamp_is_recent(now_unix: i64, timestamp: Option<i64>, interval: i64) -> bool {
    timestamp.is_some_and(|timestamp| {
        let age = now_unix.saturating_sub(timestamp);
        if age < -CLOCK_SKEW_TOLERANCE_SECONDS {
            false
        } else {
            age.max(0) < interval
        }
    })
}

fn unpackaged_distribution(has_inno_uninstaller: bool) -> Distribution {
    if has_inno_uninstaller {
        Distribution::GitHubInstaller
    } else {
        Distribution::GitHubPortable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    GitHubInstaller,
    GitHubPortable,
    MicrosoftStore,
    Unknown,
}

impl Distribution {
    pub fn current() -> Self {
        match crate::packaged::state() {
            crate::packaged::PackageState::Packaged => Self::MicrosoftStore,
            crate::packaged::PackageState::Unpackaged => {
                let installed = std::env::current_exe()
                    .ok()
                    .is_some_and(|exe| exe.with_file_name("unins000.exe").is_file());
                unpackaged_distribution(installed)
            }
            crate::packaged::PackageState::Unknown => Self::Unknown,
        }
    }

    pub fn can_offer_github_update(self) -> bool {
        matches!(self, Self::GitHubInstaller | Self::GitHubPortable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub update_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    Offline,
    Http(u16),
    Timeout,
    InvalidMetadata,
}

#[derive(Debug, Clone, Default)]
pub struct UiState {
    pub release_url: Option<String>,
    pub update_available: bool,
}

#[derive(Debug, Clone, Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Available {
        current: String,
        latest: String,
    },
    Current,
    Offline,
    Http(u16),
    Invalid,
    Timeout,
}

pub fn distribution_label(
    distribution: Distribution,
    locale: crate::localization::Locale,
) -> &'static str {
    use crate::localization::Locale::*;
    match (distribution, locale) {
        (Distribution::MicrosoftStore, Uk) => "Дистрибутив: Microsoft Store (MSIX)",
        (Distribution::MicrosoftStore, En) => "Distribution: Microsoft Store (MSIX)",
        (Distribution::MicrosoftStore, Pl) => "Dystrybucja: Microsoft Store (MSIX)",
        (Distribution::MicrosoftStore, Ru) => "Дистрибутив: Microsoft Store (MSIX)",
        (Distribution::GitHubInstaller, Uk) => "Дистрибутив: інсталятор GitHub",
        (Distribution::GitHubInstaller, En) => "Distribution: GitHub Installer",
        (Distribution::GitHubInstaller, Pl) => "Dystrybucja: instalator GitHub",
        (Distribution::GitHubInstaller, Ru) => "Дистрибутив: установщик GitHub",
        (Distribution::GitHubPortable, Uk) => "Дистрибутив: портативна версія GitHub",
        (Distribution::GitHubPortable, En) => "Distribution: GitHub Portable",
        (Distribution::GitHubPortable, Pl) => "Dystrybucja: wersja przenośna GitHub",
        (Distribution::GitHubPortable, Ru) => "Дистрибутив: портативная версия GitHub",
        (Distribution::Unknown, Uk) => "Дистрибутив: невідомий",
        (Distribution::Unknown, En) => "Distribution: unknown",
        (Distribution::Unknown, Pl) => "Dystrybucja: nieznana",
        (Distribution::Unknown, Ru) => "Дистрибутив: неизвестен",
    }
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn parse_latest_release(current: &str, body: &[u8]) -> Result<UpdateInfo, CheckError> {
    let current = semver::Version::parse(current).map_err(|_| CheckError::InvalidMetadata)?;
    let release: Release = serde_json::from_slice(body).map_err(|_| CheckError::InvalidMetadata)?;
    if release.draft || release.prerelease {
        return Err(CheckError::InvalidMetadata);
    }
    let tag = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let latest = semver::Version::parse(tag).map_err(|_| CheckError::InvalidMetadata)?;
    if !latest.pre.is_empty() {
        return Err(CheckError::InvalidMetadata);
    }
    let release_url = format!("{RELEASE_PAGE}/tag/v{latest}");
    Ok(UpdateInfo {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        release_url,
        update_available: latest.cmp_precedence(&current).is_gt(),
    })
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

pub fn check_latest() -> Result<UpdateInfo, CheckError> {
    let body = platform::get(Duration::from_secs(20))?;
    finish_response(current_version(), Ok(body))
}

fn finish_response(
    current: &str,
    response: Result<Vec<u8>, CheckError>,
) -> Result<UpdateInfo, CheckError> {
    parse_latest_release(current, &response?)
}

#[cfg(windows)]
mod platform {
    use super::CheckError;
    use std::{
        ffi::OsStr,
        os::windows::ffi::OsStrExt,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::Networking::WinHttp::*;

    struct Handle(*mut std::ffi::c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(Some(0)).collect()
    }

    fn set_remaining_timeout(
        handle: *mut std::ffi::c_void,
        started: Instant,
        deadline: Duration,
    ) -> Result<(), CheckError> {
        let timeout_ms = super::remaining_timeout_ms(started, deadline, Instant::now())?;
        if unsafe { WinHttpSetTimeouts(handle, timeout_ms, timeout_ms, timeout_ms, timeout_ms) }
            == 0
        {
            return Err(CheckError::Offline);
        }
        Ok(())
    }

    pub fn get(deadline: Duration) -> Result<Vec<u8>, CheckError> {
        unsafe {
            let started = Instant::now();
            let agent = wide("AgentUsageMonitor update checker");
            let session = Handle(WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            ));
            if session.0.is_null() {
                return Err(CheckError::Offline);
            }
            let host = wide("api.github.com");
            set_remaining_timeout(session.0, started, deadline)?;
            let connection = Handle(WinHttpConnect(session.0, host.as_ptr(), 443, 0));
            if connection.0.is_null() {
                return Err(CheckError::Offline);
            }
            let path = wide("/repos/valenticus/agent-usage-monitor/releases/latest");
            let request = Handle(WinHttpOpenRequest(
                connection.0,
                wide("GET").as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ));
            if request.0.is_null() {
                return Err(last_network_error());
            }
            let redirect_policy: u32 = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
            if WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_REDIRECT_POLICY,
                (&redirect_policy as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            ) == 0
            {
                return Err(CheckError::Offline);
            }
            let headers = wide("Accept: application/vnd.github+json\r\n");
            set_remaining_timeout(request.0, started, deadline)?;
            if WinHttpSendRequest(
                request.0,
                headers.as_ptr(),
                u32::MAX,
                std::ptr::null(),
                0,
                0,
                0,
            ) == 0
            {
                return Err(last_network_error());
            }
            set_remaining_timeout(request.0, started, deadline)?;
            if WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0 {
                return Err(last_network_error());
            }
            let mut status = 0u32;
            let mut status_size = std::mem::size_of::<u32>() as u32;
            if WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                (&mut status as *mut u32).cast(),
                &mut status_size,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(last_network_error());
            }
            if !(200..300).contains(&status) {
                return Err(CheckError::Http(status.min(u16::MAX as u32) as u16));
            }
            let mut body = Vec::new();
            loop {
                set_remaining_timeout(request.0, started, deadline)?;
                let mut available = 0u32;
                if WinHttpQueryDataAvailable(request.0, &mut available) == 0 {
                    return Err(last_network_error());
                }
                if available == 0 {
                    break;
                }
                if body.len().saturating_add(available as usize) > super::MAX_RESPONSE_BYTES {
                    return Err(CheckError::InvalidMetadata);
                }
                let start = body.len();
                body.resize(start + available as usize, 0);
                let mut read = 0u32;
                set_remaining_timeout(request.0, started, deadline)?;
                if WinHttpReadData(
                    request.0,
                    body[start..].as_mut_ptr().cast(),
                    available,
                    &mut read,
                ) == 0
                {
                    return Err(last_network_error());
                }
                body.truncate(start + read as usize);
            }
            Ok(body)
        }
    }

    fn last_network_error() -> CheckError {
        use windows_sys::Win32::Foundation::GetLastError;
        let error = unsafe { GetLastError() };
        if error == ERROR_WINHTTP_TIMEOUT {
            CheckError::Timeout
        } else {
            CheckError::Offline
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::CheckError;
    use std::time::Duration;
    pub fn get(_: Duration) -> Result<Vec<u8>, CheckError> {
        Err(CheckError::Offline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool, prerelease: bool) -> Vec<u8> {
        serde_json::json!({
            "tag_name": tag,
            "html_url": format!("{RELEASE_PAGE}/tag/{tag}"),
            "draft": draft,
            "prerelease": prerelease,
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn detects_newer_same_and_older_versions() {
        assert!(
            parse_latest_release("0.2.0", &release("v0.3.0", false, false))
                .unwrap()
                .update_available
        );
        assert!(
            !parse_latest_release("0.3.0", &release("v0.3.0", false, false))
                .unwrap()
                .update_available
        );
        assert!(
            !parse_latest_release("0.3.0", &release("v0.2.0", false, false))
                .unwrap()
                .update_available
        );
    }

    #[test]
    fn rejects_invalid_metadata_and_prereleases_or_drafts() {
        assert_eq!(
            parse_latest_release("0.2.0", b"{}"),
            Err(CheckError::InvalidMetadata)
        );
        assert_eq!(
            parse_latest_release("0.2.0", &release("v0.3.0-rc.1", false, true)),
            Err(CheckError::InvalidMetadata)
        );
        assert_eq!(
            parse_latest_release("0.2.0", &release("v0.3.0", true, false)),
            Err(CheckError::InvalidMetadata)
        );
        let foreign_url = String::from_utf8(release("v0.3.0", false, false))
            .unwrap()
            .replace(
                &format!("{RELEASE_PAGE}/tag/v0.3.0"),
                "https://github.com.evil.example/tag/v0.3.0",
            )
            .into_bytes();
        assert_eq!(
            parse_latest_release("0.2.0", &foreign_url)
                .unwrap()
                .release_url,
            format!("{RELEASE_PAGE}/tag/v0.3.0")
        );
    }

    #[test]
    fn reports_offline_network_error() {
        assert_eq!(
            finish_response("0.2.0", Err(CheckError::Offline)),
            Err(CheckError::Offline)
        );
    }

    #[test]
    fn store_cannot_offer_a_github_update() {
        assert!(!Distribution::MicrosoftStore.can_offer_github_update());
        assert!(Distribution::GitHubInstaller.can_offer_github_update());
        assert!(Distribution::GitHubPortable.can_offer_github_update());
        assert!(!Distribution::Unknown.can_offer_github_update());
        assert_eq!(unpackaged_distribution(true), Distribution::GitHubInstaller);
        assert_eq!(unpackaged_distribution(false), Distribution::GitHubPortable);
    }

    #[test]
    fn automatic_checks_follow_success_timestamp_and_respect_clock_skew() {
        let interval = AUTOMATIC_CHECK_INTERVAL.as_secs() as i64;
        assert!(automatic_check_due(10_000, None, None));
        assert!(!automatic_check_due(10_000, Some(10_000), None));
        assert!(!automatic_check_due(
            10_000,
            Some(10_000 - interval + 1),
            None
        ));
        assert!(automatic_check_due(10_000, Some(10_000 - interval), None));
        assert!(!automatic_check_due(10_000, Some(10_001), None));
        assert!(!automatic_check_due(10_000, Some(10_000 + 60), None));
        assert!(automatic_check_due(
            10_000,
            Some(10_000 + CLOCK_SKEW_TOLERANCE_SECONDS + 1),
            None
        ));
        assert!(automatic_check_due(
            10_000,
            Some(10_000 + 24 * 60 * 60),
            None
        ));
        assert!(!automatic_check_due(
            10_000,
            Some(10_000 + 60),
            Some(10_000)
        ));
        assert!(!automatic_check_due(10_000, None, Some(10_000)));
        assert!(automatic_check_due(10_000, None, Some(10_000 - interval)));
        assert!(automatic_check_due(
            10_000,
            None,
            Some(10_000 + interval * 10)
        ));
        assert!(!automatic_check_due(10_000, None, Some(10_000 + 60)));
    }

    #[test]
    fn overall_deadline_budget_shrinks_across_request_phases() {
        use std::time::Instant;
        let started = Instant::now();
        let budget = Duration::from_secs(20);
        assert_eq!(
            remaining_timeout_ms(started, budget, started).unwrap(),
            20_000
        );
        let after_first_phase = started + Duration::from_secs(7);
        assert_eq!(
            remaining_timeout_ms(started, budget, after_first_phase).unwrap(),
            13_000
        );
        assert_eq!(
            remaining_timeout_ms(started, budget, started + budget),
            Err(CheckError::Timeout)
        );
    }

    #[test]
    fn semver_precedence_ignores_build_metadata_and_rejects_prerelease_flag_mismatch() {
        let current = release("v1.2.0+ci.5", false, false);
        assert!(
            !parse_latest_release("1.2.0+ci.1", &current)
                .unwrap()
                .update_available
        );
        assert_eq!(
            parse_latest_release("1.1.0", &release("v1.2.0-rc.1", false, false)),
            Err(CheckError::InvalidMetadata)
        );
    }
}

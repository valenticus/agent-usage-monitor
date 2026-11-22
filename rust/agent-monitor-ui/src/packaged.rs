//! Packaged (MSIX / Microsoft Store) versus unpackaged (Inno Setup, portable)
//! execution.
//!
//! One binary serves both distributions. Store-specific behaviour is chosen at
//! run time from the process's package identity, so the EXE builds keep their
//! existing behaviour and no separate code path is compiled in.
//!
//! Detection has three outcomes. Only a *confirmed* unpackaged process may
//! write the unpackaged per-user registry state (HKCU `Run` value, toast AUMID
//! registration) or use the EXE builds' settings folder. An unexpected API
//! result is `Unknown`: it gets neither the Store behaviour nor those writes,
//! and its data goes to an isolated folder (see `settings::settings_directory`).

/// `TaskId` of the `windows.startupTask` extension in `packaging/msix/AppxManifest.xml`.
pub const STARTUP_TASK_ID: &str = "AgentUsageMonitorStartup";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageState {
    /// The process has MSIX package identity (Store build).
    Packaged,
    /// Windows confirmed there is no package identity (Inno, portable).
    Unpackaged,
    /// The identity query failed unexpectedly. Treated conservatively.
    Unknown,
}

/// Package identity of this process, cached for the process lifetime: package
/// identity cannot change while a process runs.
pub fn state() -> PackageState {
    static STATE: std::sync::OnceLock<PackageState> = std::sync::OnceLock::new();
    *STATE.get_or_init(detect)
}

/// True only for confirmed MSIX execution.
pub fn is_packaged() -> bool {
    state() == PackageState::Packaged
}

/// True only for confirmed unpackaged execution: the EXE builds' registry and
/// settings locations may be written.
pub fn is_confirmed_unpackaged() -> bool {
    state() == PackageState::Unpackaged
}

/// Maps the `GetCurrentPackageFullName` status of a size query (zero length,
/// null buffer) to a package state.
#[cfg(windows)]
fn classify(status: u32) -> PackageState {
    use windows_sys::Win32::Foundation::{
        APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
    };
    match status {
        // The documented answer for a packaged process is "buffer too small".
        ERROR_INSUFFICIENT_BUFFER | ERROR_SUCCESS => PackageState::Packaged,
        APPMODEL_ERROR_NO_PACKAGE => PackageState::Unpackaged,
        _ => PackageState::Unknown,
    }
}

#[cfg(windows)]
fn detect() -> PackageState {
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    let mut length = 0u32;
    // SAFETY: a zero length with a null buffer only queries the required size.
    let status = unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) };
    let state = classify(status);
    if state == PackageState::Unknown {
        eprintln!(
            "PACKAGE: identity query returned {status}; Store behaviour and unpackaged registry writes are both disabled"
        );
    }
    state
}

#[cfg(not(windows))]
fn detect() -> PackageState {
    PackageState::Unpackaged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processes_are_confirmed_unpackaged() {
        // `cargo test` binaries never run with package identity.
        assert_eq!(state(), PackageState::Unpackaged);
        assert!(!is_packaged());
        assert!(is_confirmed_unpackaged());
    }

    #[cfg(windows)]
    #[test]
    fn unexpected_identity_errors_are_unknown_not_unpackaged() {
        use windows_sys::Win32::Foundation::{
            APPMODEL_ERROR_NO_PACKAGE, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER,
            ERROR_INVALID_PARAMETER,
        };
        assert_eq!(classify(ERROR_INSUFFICIENT_BUFFER), PackageState::Packaged);
        assert_eq!(
            classify(APPMODEL_ERROR_NO_PACKAGE),
            PackageState::Unpackaged
        );
        assert_eq!(classify(ERROR_INVALID_PARAMETER), PackageState::Unknown);
        assert_eq!(classify(ERROR_ACCESS_DENIED), PackageState::Unknown);
    }
}

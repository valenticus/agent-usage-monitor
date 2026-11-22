//! Locating the programs AUM starts.
//!
//! * Windows utilities (`cmd.exe`, `powershell.exe`, `taskkill.exe`) are
//!   started from the system directory by absolute path. A bare name would be
//!   searched in the application directory first, so a file planted next to a
//!   portable copy could run instead. If the system directory cannot be read,
//!   the lookup **fails**; there is no fallback to a bare name.
//! * Provider CLIs are found on `PATH`, using absolute `PATH` entries only and
//!   never the current directory.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Absolute path of a Windows utility below the system directory (for example
/// `cmd.exe` or `WindowsPowerShell\v1.0\powershell.exe`). In a 64-bit process
/// this is the native `System32`. Errors when the system directory cannot be
/// determined or `relative` is not a plain relative path.
#[cfg(windows)]
pub fn system_tool_path(relative: &str) -> Result<PathBuf, String> {
    static SYSTEM_DIRECTORY: std::sync::OnceLock<Result<PathBuf, String>> =
        std::sync::OnceLock::new();
    let directory = SYSTEM_DIRECTORY.get_or_init(|| {
        read_system_directory(|buffer| {
            use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
            // SAFETY: the pointer and length describe `buffer` exactly (a
            // zero-length query with a dangling pointer only returns the size).
            unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) }
        })
    });
    join_tool(directory.as_ref().map_err(Clone::clone)?, relative)
}

#[cfg(not(windows))]
pub fn system_tool_path(relative: &str) -> Result<PathBuf, String> {
    Err(format!(
        "{relative}: Windows system utilities are unavailable"
    ))
}

fn join_tool(directory: &Path, relative: &str) -> Result<PathBuf, String> {
    let tool = Path::new(relative);
    if relative.is_empty()
        || tool.is_absolute()
        || tool
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!("invalid system utility name: {relative}"));
    }
    Ok(directory.join(tool))
}

/// Reads a directory through a `GetSystemDirectoryW`-style call: it returns
/// the length without the terminating NUL when the buffer is large enough,
/// the required size including the NUL when it is too small, and 0 on error.
/// Grows the buffer as needed (long paths included) and requires an absolute
/// result.
fn read_system_directory(mut query: impl FnMut(&mut [u16]) -> u32) -> Result<PathBuf, String> {
    let mut buffer: Vec<u16> = vec![0; 260];
    for _ in 0..4 {
        let length = query(&mut buffer) as usize;
        if length == 0 {
            return Err("GetSystemDirectoryW failed".into());
        }
        if length < buffer.len() {
            let path =
                PathBuf::from(String::from_utf16(&buffer[..length]).map_err(|_| {
                    "GetSystemDirectoryW returned an invalid UTF-16 path".to_string()
                })?);
            if !path.is_absolute() {
                return Err(format!("system directory is not absolute: {path:?}"));
            }
            return Ok(path);
        }
        if length > 32_768 {
            return Err("system directory path is too long".into());
        }
        buffer = vec![0; length];
    }
    Err("system directory length kept changing".into())
}

/// Finds a CLI on the process `PATH`; see [`resolve_cli_in`].
pub fn resolve_cli_executable(name: &str) -> Option<PathBuf> {
    resolve_cli_in(&std::env::var_os("PATH")?, name)
}

/// Finds `<name>.exe`, then `<name>.cmd`, in each **absolute** directory of
/// `path_variable`, in order. Relative or empty entries (including `.`) are
/// skipped, so the current directory is never searched. Returns the absolute
/// path of the first regular file found.
pub fn resolve_cli_in(path_variable: &OsStr, name: &str) -> Option<PathBuf> {
    if name.is_empty() || Path::new(name).components().count() != 1 {
        return None;
    }
    std::env::split_paths(path_variable)
        .filter(|directory| directory.is_absolute())
        .find_map(|directory| {
            ["exe", "cmd"].iter().find_map(|extension| {
                let candidate = directory.join(format!("{name}.{extension}"));
                std::fs::metadata(&candidate)
                    .is_ok_and(|metadata| metadata.is_file())
                    .then_some(candidate)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake `GetSystemDirectoryW` backed by `value`.
    fn fake(value: &'static str) -> impl FnMut(&mut [u16]) -> u32 {
        move |buffer| {
            let units: Vec<u16> = value.encode_utf16().collect();
            if units.len() < buffer.len() {
                buffer[..units.len()].copy_from_slice(&units);
                buffer[units.len()] = 0;
                units.len() as u32
            } else {
                units.len() as u32 + 1
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn system_directory_is_read_and_long_paths_grow_the_buffer() {
        assert_eq!(
            read_system_directory(fake(r"C:\Windows\system32")).unwrap(),
            PathBuf::from(r"C:\Windows\system32")
        );
        // Exactly at and beyond the initial 260-unit buffer.
        let at_limit: &'static str = Box::leak(format!(r"C:\{}", "a".repeat(257)).into_boxed_str());
        assert_eq!(at_limit.len(), 260);
        assert_eq!(
            read_system_directory(fake(at_limit)).unwrap(),
            PathBuf::from(at_limit)
        );
        let long: &'static str = Box::leak(format!(r"C:\{}", "b".repeat(600)).into_boxed_str());
        assert_eq!(
            read_system_directory(fake(long)).unwrap(),
            PathBuf::from(long)
        );
    }

    #[test]
    fn system_directory_failures_never_fall_back_to_a_bare_name() {
        assert!(read_system_directory(|_| 0).is_err());
        assert!(read_system_directory(fake(r"system32")).is_err());
        // A size that keeps growing (or is absurd) is rejected, not looped on.
        let mut size = 300;
        assert!(
            read_system_directory(|_| {
                size += 100;
                size
            })
            .is_err()
        );
        assert!(read_system_directory(|_| 70_000).is_err());
        assert!(join_tool(Path::new(r"C:\Windows\System32"), "").is_err());
        assert!(join_tool(Path::new(r"C:\Windows\System32"), r"..\cmd.exe").is_err());
        assert!(join_tool(Path::new(r"C:\Windows\System32"), r"C:\cmd.exe").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn system_tools_resolve_to_absolute_paths_in_the_system_directory() {
        for tool in ["cmd.exe", "taskkill.exe"] {
            let path = system_tool_path(tool).unwrap();
            assert!(path.is_absolute(), "{path:?}");
            assert!(path.is_file(), "{path:?}");
            assert!(
                path.to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(&format!(r"\system32\{tool}")),
                "{path:?}"
            );
        }
        let powershell = system_tool_path(r"WindowsPowerShell\v1.0\powershell.exe").unwrap();
        assert!(powershell.is_absolute());
    }

    #[test]
    fn cli_resolution_uses_absolute_path_entries_only() {
        let root = tempfile::tempdir().unwrap();
        let exe_dir = root.path().join("native bin ünï");
        let shim_dir = root.path().join("npm (x86) & co");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&shim_dir).unwrap();
        std::fs::write(shim_dir.join("claude.cmd"), b"@echo off").unwrap();
        std::fs::write(exe_dir.join("claude.exe"), b"").unwrap();
        let path = |dirs: &[&Path]| std::env::join_paths(dirs).unwrap();

        // PATH order decides; .exe before .cmd within one directory.
        assert_eq!(
            resolve_cli_in(&path(&[&shim_dir, &exe_dir]), "claude"),
            Some(shim_dir.join("claude.cmd"))
        );
        assert_eq!(
            resolve_cli_in(&path(&[&exe_dir, &shim_dir]), "claude"),
            Some(exe_dir.join("claude.exe"))
        );
        // Relative entries (the current directory) are never searched, even
        // when a same-named program would be found there.
        let relative = std::env::join_paths([".", "", "relative-bin"]).unwrap();
        assert_eq!(resolve_cli_in(&relative, "claude"), None);
        // Absent from PATH.
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(resolve_cli_in(&path(&[&empty]), "claude"), None);
        // Directories with the right name are not programs; names with
        // separators are rejected.
        std::fs::create_dir_all(empty.join("claude.exe")).unwrap();
        assert_eq!(resolve_cli_in(&path(&[&empty]), "claude"), None);
        assert_eq!(resolve_cli_in(&path(&[&exe_dir]), r"..\claude"), None);
    }
}

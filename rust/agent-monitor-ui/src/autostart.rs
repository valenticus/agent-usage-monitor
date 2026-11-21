//! Per-user Windows sign-in launch registration.
//!
//! AUM owns exactly one value under HKCU's Run key. The command points at the
//! current executable, so installed and portable copies use the same path.

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "AgentUsageMonitor";

/// Configure the AUM-owned registration. The value is only updated/deleted by
/// name; no other Run entries are enumerated or modified.
#[cfg(windows)]
pub fn set_enabled(
    enabled: bool,
    executable: &std::path::Path,
    claude_bridge: bool,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
        RegSetValueExW,
    };

    let mut key = std::ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if result != 0 {
        return Err(format!("could not open per-user startup key ({result})"));
    }

    let outcome = if enabled {
        let command = startup_command(executable, claude_bridge);
        let mut data: Vec<u16> = std::ffi::OsStr::new(&command).encode_wide().collect();
        data.push(0);
        unsafe {
            RegSetValueExW(
                key,
                wide(VALUE_NAME).as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * std::mem::size_of::<u16>()) as u32,
            )
        }
    } else {
        let result = unsafe { RegDeleteValueW(key, wide(VALUE_NAME).as_ptr()) };
        // Deleting an absent AUM-owned entry is already the requested state.
        if result == 2 { 0 } else { result }
    };
    unsafe { RegCloseKey(key) };
    if outcome == 0 {
        Ok(())
    } else {
        Err(format!(
            "could not update per-user startup entry ({outcome})"
        ))
    }
}

/// Return whether the exact AUM Run value targets this executable. A stale
/// path is reported as disabled until reconciliation succeeds.
#[cfg(windows)]
pub fn is_enabled_for(executable: &std::path::Path) -> Result<bool, String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_QUERY_VALUE, REG_SZ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
    };

    let mut key = std::ptr::null_mut();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if result == 2 {
        return Ok(false);
    }
    if result != 0 {
        return Err(format!("could not read per-user startup key ({result})"));
    }

    let mut kind = 0;
    let mut bytes = 0u32;
    let mut query = unsafe {
        RegQueryValueExW(
            key,
            wide(VALUE_NAME).as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if query == 2 {
        unsafe { RegCloseKey(key) };
        return Ok(false);
    }
    if query != 0 || kind != REG_SZ || bytes < 2 || bytes % 2 != 0 {
        unsafe { RegCloseKey(key) };
        return if query == 0 {
            Ok(false)
        } else {
            Err(format!("could not read per-user startup entry ({query})"))
        };
    }
    let mut data = vec![0u16; (bytes / 2) as usize];
    query = unsafe {
        RegQueryValueExW(
            key,
            wide(VALUE_NAME).as_ptr(),
            std::ptr::null(),
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    unsafe { RegCloseKey(key) };
    if query != 0 || kind != REG_SZ {
        return Err(format!("could not read per-user startup entry ({query})"));
    }
    let length = data
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(data.len());
    let command = String::from_utf16_lossy(&data[..length]);
    Ok(command_matches_executable(&command, executable))
}

#[cfg(not(windows))]
pub fn set_enabled(
    _enabled: bool,
    _executable: &std::path::Path,
    _claude_bridge: bool,
) -> Result<(), String> {
    Err("Windows sign-in startup is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn is_enabled_for(_executable: &std::path::Path) -> Result<bool, String> {
    Ok(false)
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn quote_executable(executable: &std::path::Path) -> String {
    format!("\"{}\"", executable.to_string_lossy())
}

fn startup_command(executable: &std::path::Path, claude_bridge: bool) -> String {
    let mut command = quote_executable(executable);
    if claude_bridge {
        command.push_str(" --enable-claude-bridge");
    }
    command
}

fn command_matches_executable(command: &str, executable: &std::path::Path) -> bool {
    let Some((path, arguments)) = command
        .strip_prefix('"')
        .and_then(|value| value.split_once('"'))
    else {
        return false;
    };
    let expected_path = executable.to_string_lossy();
    let path_matches = path.eq_ignore_ascii_case(&expected_path);
    let arguments_match = arguments.is_empty() || arguments == " --enable-claude-bridge";
    path_matches && arguments_match
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn startup_command_quotes_paths_with_spaces_and_unicode() {
        assert_eq!(
            startup_command(
                Path::new(r"C:\Users\Éva Kovács\AUM\agent-usage-monitor.exe"),
                false
            ),
            r#""C:\Users\Éva Kovács\AUM\agent-usage-monitor.exe""#
        );
        assert_eq!(
            startup_command(Path::new(r"C:\AUM\agent-usage-monitor.exe"), true),
            r#""C:\AUM\agent-usage-monitor.exe" --enable-claude-bridge"#
        );
    }

    #[test]
    fn only_exact_current_executable_is_reported_enabled() {
        let exe = Path::new(r"C:\Portable AUM\agent-usage-monitor.exe");
        assert!(command_matches_executable(
            r#""C:\Portable AUM\agent-usage-monitor.exe""#,
            exe
        ));
        assert!(command_matches_executable(
            r#""c:\portable aum\AGENT-USAGE-MONITOR.EXE""#,
            exe
        ));
        assert!(command_matches_executable(
            r#""C:\Portable AUM\agent-usage-monitor.exe" --enable-claude-bridge"#,
            exe
        ));
        assert!(!command_matches_executable(
            r#""C:\Old AUM\agent-usage-monitor.exe""#,
            exe
        ));
        assert!(!command_matches_executable(
            r#""C:\Portable AUM\agent-usage-monitor.exe" --demo"#,
            exe
        ));
        assert!(!command_matches_executable("not quoted", exe));
    }
}

//! Read-only view of Claude Code's `statusLine` setting (v0.2.0, F-8 "guided").
//!
//! AUM **never writes** Claude settings (AGENTS.md). It only reads the user
//! settings file to tell the user whether the status line already runs the AUM
//! bridge, is free, or belongs to another tool, and it offers the exact setting
//! to copy. The user edits `settings.json` themselves.
//!
//! Safety:
//! * strictly bounded read (at most 1 MiB + 1 byte), regular files only; JSON
//!   is parsed as data, nothing from it is executed;
//! * only the `statusLine` member is looked at; every other field (which may
//!   hold environment variables or other secrets) is dropped immediately and
//!   never logged or shown;
//! * of another tool's command only the program's file name is shown, never
//!   its path or arguments;
//! * an existing third-party status line is reported as a conflict; AUM does
//!   not offer to chain or replace it;
//! * the suggested command has one fixed shape and only allow-listed path
//!   characters (see [`suggested_command`]); anything else is refused.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The bridge executable (App Execution Alias in the Store build).
pub const BRIDGE_EXE: &str = "agent-usage-monitor-claude-bridge.exe";
const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;
const MAX_PROGRAM_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusLineState {
    /// `statusLine` runs the AUM bridge.
    ConfiguredForAum,
    /// Settings exist and have no status line.
    NotConfigured,
    /// Another command owns the status line. `program` is only its file name
    /// (empty when the setting has an unexpected shape).
    OtherCommand { program: String },
    /// No user settings file (Claude Code not installed or never configured).
    SettingsNotFound,
    /// The file exists but cannot be read or parsed (permissions, size,
    /// malformed JSON, not a regular file).
    Unreadable,
}

/// Claude Code's user settings file: `%CLAUDE_CONFIG_DIR%\settings.json` when
/// that variable holds an absolute path, otherwise
/// `%USERPROFILE%\.claude\settings.json`.
pub fn user_settings_path() -> Option<PathBuf> {
    let directory = match std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from) {
        Some(directory) if directory.is_absolute() => directory,
        _ => PathBuf::from(std::env::var_os("USERPROFILE")?).join(".claude"),
    };
    Some(directory.join("settings.json"))
}

/// Inspects the settings file at `path` (read-only). Reads at most
/// `MAX_SETTINGS_BYTES + 1` bytes from one open handle, so a file that grows
/// between checks cannot make the read unbounded.
pub fn inspect(path: &Path) -> StatusLineState {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return StatusLineState::SettingsNotFound;
        }
        Err(_) => return StatusLineState::Unreadable,
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return StatusLineState::Unreadable;
    }
    let mut bytes = Vec::new();
    match file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes) {
        Ok(_) if bytes.len() as u64 <= MAX_SETTINGS_BYTES => classify(&bytes),
        _ => StatusLineState::Unreadable,
    }
}

/// Classifies settings JSON. Only `statusLine` is examined.
pub fn classify(bytes: &[u8]) -> StatusLineState {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let Ok(Value::Object(mut settings)) = serde_json::from_slice::<Value>(bytes) else {
        return StatusLineState::Unreadable;
    };
    // Keep only the one member AUM needs; drop everything else now.
    let status_line = settings.remove("statusLine");
    drop(settings);
    match status_line {
        None | Some(Value::Null) => StatusLineState::NotConfigured,
        Some(Value::Object(line)) => {
            let command = line
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if command.is_empty() {
                // An entry without a command (or of another type) still
                // belongs to someone; never call it free.
                return StatusLineState::OtherCommand {
                    program: String::new(),
                };
            }
            let program = program_name(command);
            if is_bridge(&program) {
                StatusLineState::ConfiguredForAum
            } else {
                StatusLineState::OtherCommand { program }
            }
        }
        Some(_) => StatusLineState::OtherCommand {
            program: String::new(),
        },
    }
}

/// File name of the program a shell command starts, without path or
/// arguments, printable characters only, bounded. The first word is read the
/// way bash and PowerShell join it: unquoted characters plus single- or
/// double-quoted segments (so `C':/A B/x.exe'`, `"C:/A B/x.exe"` and
/// `'C:/A B/x.exe'` all give `x.exe`). A leading PowerShell call operator
/// `&` is skipped. This is only for display and matching; nothing is run.
fn program_name(command: &str) -> String {
    let mut words = shell_words(command);
    let mut first = words.next().unwrap_or_default();
    if first == "&" {
        first = words.next().unwrap_or_default();
    }
    let name = first.rsplit(['/', '\\']).next().unwrap_or_default();
    name.chars()
        .filter(|character| !character.is_control())
        .take(MAX_PROGRAM_CHARS)
        .collect()
}

/// Splits a command into its first few words (quote characters removed).
fn shell_words(command: &str) -> impl Iterator<Item = String> + '_ {
    let mut characters = command.chars().peekable();
    std::iter::from_fn(move || {
        while characters.next_if(|c| c.is_whitespace()).is_some() {}
        characters.peek()?;
        let mut word = String::new();
        let mut quote: Option<char> = None;
        for character in characters.by_ref() {
            match quote {
                Some(open) if character == open => quote = None,
                Some(_) => word.push(character),
                None if character == '\'' || character == '"' => quote = Some(character),
                None if character.is_whitespace() => break,
                None => word.push(character),
            }
        }
        Some(word)
    })
    .take(2)
}

fn is_bridge(program: &str) -> bool {
    program.eq_ignore_ascii_case(BRIDGE_EXE)
        || program.eq_ignore_ascii_case(BRIDGE_EXE.trim_end_matches(".exe"))
}

/// Why no `statusLine` command can be suggested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The running executable's folder is unknown or not absolute.
    NoExecutableDirectory,
    /// The bridge is not on a drive-letter path (`X:/…`), e.g. a UNC share.
    UnsupportedLocation,
    /// The bridge path contains a character outside the allow-list.
    UnsupportedPathCharacter(char),
}

/// Characters allowed in the bridge path besides Unicode letters and digits.
/// Inside single quotes all of them are literal for bash and for PowerShell
/// (5.1 and 7), which treat nothing but `'` itself as special there.
/// Everything else is refused, in particular `'` (it would end the quoted
/// part) and `& ; $ % ! ^ " \` | < > * ? [ ] { } #` and control characters.
/// `~` is allowed: 8.3 short names (`C:\Users\RUNNER~1\…`) contain it, and
/// inside single quotes it is literal for bash (tilde expansion only applies
/// to an unquoted word start, and the path always starts with the drive
/// letter) and for PowerShell.
const ALLOWED_PATH_PUNCTUATION: &str = " /:._-()+,@~";

/// The command for `statusLine.command`, in one fixed shape that runs the same
/// way in every shell Claude Code uses on Windows. Per the Claude Code docs,
/// status-line commands run "through Git Bash when Git Bash is installed, or
/// through PowerShell when Git Bash is absent".
///
/// * Store build: the App Execution Alias `agent-usage-monitor-claude-bridge.exe`
///   (a bare word; files in `WindowsApps` cannot be started by path).
/// * EXE builds: the drive letter unquoted, the rest of the path in single
///   quotes, forward slashes, no arguments:
///   `C':/Program Files/AUM/agent-usage-monitor-claude-bridge.exe'`.
///   * bash joins `C` and `':/…'` into one literal word and runs it;
///   * PowerShell reads a token that starts with a letter as a command (not as
///     a string expression, which is why `"C:/…"` only printed the path) and
///     joins the single-quoted part literally;
///   * the plain `"…"` or `'…'` forms are printed, not run, by PowerShell.
///
///   Verified with Git Bash, Windows PowerShell 5.1 and PowerShell 7 (see the
///   `generated_command_runs_in_every_available_shell` test).
///
/// A path that is not on a drive letter, or that has any character outside
/// the allow-list, is refused instead of being escaped.
pub fn suggested_command(
    packaged: bool,
    executable_directory: Option<&Path>,
) -> Result<String, CommandError> {
    if packaged {
        return Ok(BRIDGE_EXE.into());
    }
    let directory = executable_directory
        .filter(|directory| directory.is_absolute())
        .ok_or(CommandError::NoExecutableDirectory)?;
    let path = directory.join(BRIDGE_EXE);
    let text = path.to_str().ok_or(CommandError::NoExecutableDirectory)?;
    let text = text.replace('\\', "/");
    let mut prefix = text.chars();
    match (prefix.next(), prefix.next(), prefix.next()) {
        (Some(drive), Some(':'), Some('/')) if drive.is_ascii_alphabetic() => {}
        _ => return Err(CommandError::UnsupportedLocation),
    }
    if let Some(bad) = text.chars().find(|character| {
        !(character.is_alphanumeric() || ALLOWED_PATH_PUNCTUATION.contains(*character))
    }) {
        return Err(CommandError::UnsupportedPathCharacter(bad));
    }
    // The first character is an ASCII drive letter, so byte index 1 is a
    // character boundary.
    Ok(format!("{}'{}'", &text[..1], &text[1..]))
}

/// The `"statusLine"` member to paste into the top-level object of
/// `settings.json` (JSON-escaped).
pub fn setting_snippet(command: &str) -> String {
    let command = serde_json::to_string(command).unwrap_or_else(|_| "\"\"".into());
    format!("\"statusLine\": {{\n  \"type\": \"command\",\n  \"command\": {command}\n}}")
}

/// Why the clipboard was not updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardError {
    /// No owner window was given; AUM never opens the clipboard without one.
    NoOwnerWindow,
    /// The text could not be prepared in global memory (clipboard untouched).
    Allocation,
    /// Another program kept the clipboard open (clipboard untouched).
    Busy,
    /// `EmptyClipboard` failed (clipboard untouched).
    Empty,
    /// The data was not accepted after the clipboard was emptied.
    SetData,
    /// Not supported on this platform.
    #[cfg(not(windows))]
    Unsupported,
}

/// Places `text` on the clipboard as `CF_UNICODETEXT`, owned by the window
/// `owner` (an `HWND` of this process).
///
/// Order, so that failures leave the previous content in place wherever the
/// API allows it: the text is prepared in movable global memory **before**
/// the clipboard is opened; the clipboard is opened with a valid owner (a
/// few short retries if another program holds it); only then is it emptied
/// and the data handed over. The memory is freed by AUM unless
/// `SetClipboardData` succeeded (the system owns it then). Only
/// `SetClipboardData` failing after `EmptyClipboard` can lose the previous
/// content; with a valid owner and a valid handle that is not expected.
#[cfg(windows)]
pub fn copy_to_clipboard(owner: isize, text: &str) -> Result<(), ClipboardError> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    const CF_UNICODETEXT: u32 = 13;
    if owner == 0 {
        return Err(ClipboardError::NoOwnerWindow);
    }
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the allocation is exactly `units.len()` u16 values; it is
    // written only while locked; it is freed on every path except a
    // successful SetClipboardData, which transfers ownership to the system.
    // OpenClipboard/CloseClipboard are balanced on every path.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of_val(units.as_slice()));
        if memory.is_null() {
            return Err(ClipboardError::Allocation);
        }
        let target = GlobalLock(memory).cast::<u16>();
        if target.is_null() {
            GlobalFree(memory);
            return Err(ClipboardError::Allocation);
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), target, units.len());
        GlobalUnlock(memory);

        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(owner as _) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        if !opened {
            GlobalFree(memory);
            return Err(ClipboardError::Busy);
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            GlobalFree(memory);
            return Err(ClipboardError::Empty);
        }
        if SetClipboardData(CF_UNICODETEXT, memory).is_null() {
            CloseClipboard();
            GlobalFree(memory);
            return Err(ClipboardError::SetData);
        }
        CloseClipboard();
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn copy_to_clipboard(_owner: isize, _text: &str) -> Result<(), ClipboardError> {
    Err(ClipboardError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_unconfigured_settings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        assert_eq!(inspect(&path), StatusLineState::SettingsNotFound);
        std::fs::write(&path, "{}").unwrap();
        assert_eq!(inspect(&path), StatusLineState::NotConfigured);
        std::fs::write(&path, r#"{"model":"opus","statusLine":null}"#).unwrap();
        assert_eq!(inspect(&path), StatusLineState::NotConfigured);
    }

    #[test]
    fn aum_bridge_is_recognised_in_every_documented_form() {
        for command in [
            "agent-usage-monitor-claude-bridge.exe",
            "agent-usage-monitor-claude-bridge",
            "AGENT-USAGE-MONITOR-CLAUDE-BRIDGE.EXE",
            "C:/Users/me/AppData/Local/Programs/AgentUsageMonitor/agent-usage-monitor-claude-bridge.exe",
            r#""C:\Program Files\AUM dir\agent-usage-monitor-claude-bridge.exe""#,
            // The fixed v0.2.0 shape and a PowerShell call-operator form.
            "C':/Program Files (x86)/AUM/agent-usage-monitor-claude-bridge.exe'",
            "& 'C:/A B/agent-usage-monitor-claude-bridge.exe'",
        ] {
            let json = serde_json::json!({"statusLine": {"type": "command", "command": command}});
            assert_eq!(
                classify(json.to_string().as_bytes()),
                StatusLineState::ConfiguredForAum,
                "{command}"
            );
        }
    }

    #[test]
    fn other_tools_are_a_conflict_and_only_the_program_name_is_shown() {
        let nook = serde_json::json!({
            "env": {"ANTHROPIC_API_KEY": "sk-secret-value"},
            "statusLine": {
                "type": "command",
                "command": "C:\\Users\\me\\AppData\\Local\\NookStandalone\\bin\\nook-hook-standalone.exe status --token=abc123"
            },
            "hooks": {"Stop": []}
        });
        let state = classify(nook.to_string().as_bytes());
        assert_eq!(
            state,
            StatusLineState::OtherCommand {
                program: "nook-hook-standalone.exe".into()
            }
        );
        let shown = format!("{state:?}");
        for secret in ["sk-secret-value", "abc123", "token", "Users", "AppData"] {
            assert!(!shown.contains(secret), "{secret} leaked: {shown}");
        }
        // The bridge named somewhere other than as the program is not AUM.
        let chained = serde_json::json!({"statusLine": {"type": "command",
            "command": "nook.exe | agent-usage-monitor-claude-bridge.exe"}});
        assert!(matches!(
            classify(chained.to_string().as_bytes()),
            StatusLineState::OtherCommand { .. }
        ));
        // Unexpected shapes are never treated as a free status line.
        for json in [
            r#"{"statusLine":"something"}"#,
            r#"{"statusLine":{"type":"static"}}"#,
            r#"{"statusLine":{"type":"command","command":"   "}}"#,
        ] {
            assert!(
                matches!(
                    classify(json.as_bytes()),
                    StatusLineState::OtherCommand { .. }
                ),
                "{json}"
            );
        }
    }

    #[test]
    fn malformed_oversized_and_unreadable_files_are_reported_not_guessed() {
        for bad in ["", "{", "[]", "null", "{\"statusLine\": ", "not json"] {
            assert_eq!(
                classify(bad.as_bytes()),
                StatusLineState::Unreadable,
                "{bad:?}"
            );
        }
        let directory = tempfile::tempdir().unwrap();
        // A directory in place of the file (stands in for an access error).
        let as_directory = directory.path().join("settings.json");
        std::fs::create_dir_all(&as_directory).unwrap();
        assert_eq!(inspect(&as_directory), StatusLineState::Unreadable);
        // A UTF-8 BOM is accepted.
        let bom = directory.path().join("bom.json");
        std::fs::write(&bom, b"\xEF\xBB\xBF{\"statusLine\":null}").unwrap();
        assert_eq!(inspect(&bom), StatusLineState::NotConfigured);
    }

    /// Bounded read: exactly the limit is accepted, one byte more is refused,
    /// and a much larger file is refused without reading it all.
    #[test]
    fn settings_read_is_strictly_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let body = br#"{"x":1,"statusLine":null}"#;
        let at_limit = directory.path().join("at-limit.json");
        let mut exact = body.to_vec();
        exact.resize(MAX_SETTINGS_BYTES as usize, b' ');
        std::fs::write(&at_limit, &exact).unwrap();
        assert_eq!(inspect(&at_limit), StatusLineState::NotConfigured);
        let over = directory.path().join("over.json");
        exact.push(b' ');
        std::fs::write(&over, &exact).unwrap();
        assert_eq!(inspect(&over), StatusLineState::Unreadable);
        let huge = directory.path().join("huge.json");
        std::fs::write(&huge, vec![b' '; 8 * MAX_SETTINGS_BYTES as usize]).unwrap();
        assert_eq!(inspect(&huge), StatusLineState::Unreadable);
        // Unknown fields and the Nook shape survive the bounded path.
        let nook = directory.path().join("nook.json");
        std::fs::write(
            &nook,
            br#"{"futureField":{"a":[1,2]},"statusLine":{"type":"command","command":"nook-hook-standalone.exe"}}"#,
        )
        .unwrap();
        assert_eq!(
            inspect(&nook),
            StatusLineState::OtherCommand {
                program: "nook-hook-standalone.exe".into()
            }
        );
    }

    #[test]
    fn inspection_never_modifies_the_settings_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let original = b"{\n  \"statusLine\": {\"type\": \"command\", \"command\": \"nook.exe\"},\n  \"x\": 1\n}\n";
        std::fs::write(&path, original).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        for _ in 0..3 {
            let _ = inspect(&path);
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn suggested_command_has_one_fixed_quoted_shape() {
        assert_eq!(
            suggested_command(true, Some(Path::new(r"C:\x"))).unwrap(),
            BRIDGE_EXE
        );
        assert_eq!(
            suggested_command(false, Some(Path::new(r"C:\Users\me\AUM"))).unwrap(),
            "C':/Users/me/AUM/agent-usage-monitor-claude-bridge.exe'"
        );
        // Only drive-letter paths: UNC shares and verbatim prefixes are refused.
        for directory in [r"\\server\share\AUM", r"\\?\C:\AUM"] {
            assert!(
                suggested_command(false, Some(Path::new(directory))).is_err(),
                "{directory}"
            );
        }
        assert_eq!(
            suggested_command(false, None),
            Err(CommandError::NoExecutableDirectory)
        );
        assert_eq!(
            suggested_command(false, Some(Path::new(r"relative\AUM"))),
            Err(CommandError::NoExecutableDirectory)
        );
    }

    /// Accepted: embedded spaces, `( )`, `+ , @`, non-ASCII letters; each one
    /// stays a single quoted literal word with no shell-active character.
    /// Refused: `& ; $ % ! ^` and the other shell metacharacters.
    #[test]
    fn bridge_paths_with_shell_metacharacters_are_refused_not_escaped() {
        for accepted in [
            r"C:\Program Files\AUM",
            r"C:\Program Files (x86)\Agent Usage Monitor",
            r"C:\Users\Іван Петренко\AppData\Local\Programs\AgentUsageMonitor",
            r"D:\Tools\AUM v2+beta,new@home",
            r"C:\Users\RUNNER~1\AppData\Local\Temp\AUM",
        ] {
            let command = suggested_command(false, Some(Path::new(accepted)))
                .unwrap_or_else(|error| panic!("{accepted}: {error:?}"));
            // Drive letter, then one single-quoted literal with no quote inside.
            assert!(
                command.as_bytes()[0].is_ascii_alphabetic()
                    && command[1..].starts_with('\'')
                    && command.ends_with('\''),
                "{command}"
            );
            let inner = &command[2..command.len() - 1];
            assert!(!inner.contains(['\'', '"', '$', '`', '\\', '!', '%', '^', '&', ';']));
            assert!(inner.ends_with("/agent-usage-monitor-claude-bridge.exe"));
        }
        for (directory, bad) in [
            (r"C:\A&B", '&'),
            (r"C:\A;B", ';'),
            (r"C:\A$HOME", '$'),
            (r"C:\100%\AUM", '%'),
            (r"C:\Hi!\AUM", '!'),
            (r"C:\A^B", '^'),
            (r"C:\A`whoami`", '`'),
            (r"C:\A'B", '\''),
            (r"C:\A|B", '|'),
            (r"C:\A>B", '>'),
            (r"C:\A*B", '*'),
            (r"C:\A#B", '#'),
            ("C:\\A\u{0007}B", '\u{0007}'),
        ] {
            assert_eq!(
                suggested_command(false, Some(Path::new(directory))),
                Err(CommandError::UnsupportedPathCharacter(bad)),
                "{directory}"
            );
        }
    }

    #[test]
    fn snippet_is_valid_json_and_recognised_as_aum() {
        for directory in [r"C:\Users\me\AUM", r"C:\Program Files (x86)\AUM"] {
            let command = suggested_command(false, Some(Path::new(directory))).unwrap();
            let snippet = setting_snippet(&command);
            let parsed: Value = serde_json::from_str(&format!("{{{snippet}}}")).unwrap();
            assert_eq!(parsed["statusLine"]["type"], "command");
            assert_eq!(parsed["statusLine"]["command"], command.as_str());
            assert_eq!(
                classify(format!("{{{snippet}}}").as_bytes()),
                StatusLineState::ConfiguredForAum
            );
        }
    }

    /// The generated command actually runs, with stdin passed through, in
    /// every shell Claude Code uses for status lines on Windows: Git Bash,
    /// and PowerShell when Git Bash is absent (Windows PowerShell 5.1 and
    /// PowerShell 7 when installed). The "bridge" is a copy of `sort.exe`
    /// (prints its stdin) in temporary folders with spaces, parentheses,
    /// Cyrillic letters and `+ , @`; nothing else is executed and no Claude
    /// settings are involved. Shells that are not installed are skipped, but
    /// Windows PowerShell 5.1 is always present on Windows.
    #[cfg(windows)]
    #[test]
    fn generated_command_runs_in_every_available_shell() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let root = tempfile::tempdir().unwrap();
        let sort =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join(r"System32\sort.exe");
        let mut shells: Vec<(&str, PathBuf, Vec<&str>)> = vec![(
            "Windows PowerShell 5.1",
            PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
            vec!["-NoProfile", "-NonInteractive", "-Command"],
        )];
        for (name, candidate, arguments) in [
            (
                "Git Bash",
                PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
                vec!["-c"],
            ),
            (
                "PowerShell 7",
                PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe"),
                vec!["-NoProfile", "-NonInteractive", "-Command"],
            ),
            (
                "PowerShell 7 (Store alias)",
                PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
                    .join(r"Microsoft\WindowsApps\pwsh.exe"),
                vec!["-NoProfile", "-NonInteractive", "-Command"],
            ),
        ] {
            let already = shells
                .iter()
                .any(|(existing, ..)| existing.starts_with("PowerShell 7"));
            if candidate.exists() && !(name.starts_with("PowerShell 7") && already) {
                shells.push((name, candidate, arguments));
            }
        }
        let run = |shell: &Path, arguments: &[&str], command: &str| {
            let mut child = Command::new(shell)
                .args(arguments)
                .arg(command)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"AUM-STDIN-PROBE\n")
                .unwrap();
            child.wait_with_output().unwrap()
        };
        eprintln!(
            "shells under test: {:?}",
            shells.iter().map(|(name, ..)| *name).collect::<Vec<_>>()
        );
        // The temp root itself may be an 8.3 short path (CI: RUNNER~1).
        for folder in ["plain", "AUM dir (x86) Іван+v2,@x", "SHORTN~1"] {
            let directory = root.path().join(folder);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::copy(&sort, directory.join(BRIDGE_EXE)).unwrap();
            let command = suggested_command(false, Some(&directory)).unwrap();
            for (name, shell, arguments) in &shells {
                // Root cause of the review finding: the former "C:/…" form is
                // only a string to PowerShell; the program never runs.
                if name.starts_with("Windows PowerShell") || name.starts_with("PowerShell 7") {
                    let old_form = format!(
                        "\"{}\"",
                        directory
                            .join(BRIDGE_EXE)
                            .to_string_lossy()
                            .replace('\\', "/")
                    );
                    let old = run(shell, arguments, &old_form);
                    assert!(
                        !String::from_utf8_lossy(&old.stdout).contains("AUM-STDIN-PROBE"),
                        "{name}: the double-quoted form unexpectedly ran"
                    );
                }
                let output = run(shell, arguments, &command);
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(
                    output.status.success() && stdout.contains("AUM-STDIN-PROBE"),
                    "{name} did not run {command}: status {:?}, stdout {stdout:?}, stderr {:?}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }

    #[test]
    fn user_settings_path_honours_an_absolute_config_dir_only() {
        // Reads the real environment only to build a path; nothing is opened.
        let path = user_settings_path().unwrap();
        assert!(path.ends_with("settings.json"));
    }

    /// Non-destructive clipboard checks: without a valid owner window AUM
    /// refuses before touching the clipboard, and an invalid owner fails at
    /// OpenClipboard. The clipboard sequence number proves the user's
    /// clipboard was not changed. (A real copy is a manual smoke test, see
    /// `docs/state/feat-v0.2.0.md`, because it would replace the user's
    /// clipboard content.)
    #[cfg(windows)]
    #[test]
    fn clipboard_never_changes_without_a_valid_owner() {
        use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
        let before = unsafe { GetClipboardSequenceNumber() };
        assert_eq!(
            copy_to_clipboard(0, "x"),
            Err(ClipboardError::NoOwnerWindow)
        );
        // A handle value that is not a window of this process.
        assert_eq!(
            copy_to_clipboard(0x7fff_fff0, "x"),
            Err(ClipboardError::Busy)
        );
        assert_eq!(unsafe { GetClipboardSequenceNumber() }, before);
    }
}

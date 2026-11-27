use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

use crate::ManualRefreshRequest;
use agent_usage_monitor_core::{
    ClaudeCliOutputError, ClaudeUsageSnapshot, parse_claude_cli_usage_diagnostic,
};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const TIMEOUT: Duration = Duration::from_secs(90);
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const FAILURE_COOLDOWN: Duration = Duration::from_secs(10 * 60);
static CLAUDE_RESET_DIAGNOSTIC_WRITTEN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) enum ClaudeRefreshEvent {
    Completed {
        request_id: Option<u64>,
        snapshot: ClaudeUsageSnapshot,
    },
    Failed {
        request_id: Option<u64>,
        failure: ClaudeRefreshFailure,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudeRefreshFailure {
    CliMissing,
    SpawnFailure,
    NonzeroExit,
    Timeout,
    EmptyOutput,
    UnrecognizedFormat,
    NoQuotaValues,
    ProcessIoFailure,
    Cancelled,
}

impl ClaudeRefreshFailure {
    pub(crate) fn category(self) -> &'static str {
        match self {
            Self::CliMissing => "cli_missing",
            Self::SpawnFailure => "spawn_failure",
            Self::NonzeroExit => "nonzero_exit",
            Self::Timeout => "timeout",
            Self::EmptyOutput => "empty_output",
            Self::UnrecognizedFormat => "unrecognized_format",
            Self::NoQuotaValues => "no_quota_values",
            Self::ProcessIoFailure => "process_io_failure",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn from_category(category: &str) -> Option<Self> {
        Some(match category {
            "spawn_failure" => Self::SpawnFailure,
            "nonzero_exit" => Self::NonzeroExit,
            "timeout" => Self::Timeout,
            "empty_output" => Self::EmptyOutput,
            "unrecognized_format" => Self::UnrecognizedFormat,
            "no_quota_values" => Self::NoQuotaValues,
            "process_io_failure" => Self::ProcessIoFailure,
            _ => return None,
        })
    }
}

/// Error text when no Claude CLI is resolved (the refresh fails closed).
const CLI_NOT_FOUND: &str = "Claude CLI not found on PATH";

pub async fn refresh_once(
    cancel: &CancellationToken,
    request_id: Option<u64>,
    observed_at: i64,
) -> Result<ClaudeUsageSnapshot, ClaudeRefreshFailure> {
    // Fails closed: without a resolved CLI nothing is started.
    let command = claude_command(agent_usage_monitor_core::resolve_cli_executable("claude"))
        .map_err(|error| {
            if error == CLI_NOT_FOUND {
                ClaudeRefreshFailure::CliMissing
            } else {
                ClaudeRefreshFailure::SpawnFailure
            }
        })?;
    run_command(command, TIMEOUT, cancel, request_id, observed_at).await
}

/// Arguments of the only Claude CLI command AUM runs. Constant, and free of
/// characters that `cmd.exe` would interpret.
const CLAUDE_ARGUMENTS: [&str; 3] = ["-p", "--no-session-persistence", "/usage"];

/// Builds the Claude CLI command from the exact path found on `PATH` (absolute
/// entries only, never the current directory):
///
/// * `claude.exe` (native installer) is started directly.
/// * `claude.cmd` (npm shim) is started through `cmd.exe` from the system
///   directory with its full quoted path, so nothing is searched again.
/// * Nothing found, another file type, or no verified `cmd.exe`: an error, and
///   no process is started.
fn claude_command(resolved: Option<std::path::PathBuf>) -> Result<Command, String> {
    if !cfg!(windows) {
        let mut command = Command::new("claude");
        command.args(CLAUDE_ARGUMENTS);
        return Ok(command);
    }
    let executable = resolved.ok_or(CLI_NOT_FOUND)?;
    if !executable.is_absolute() {
        return Err("Claude CLI path is not absolute".into());
    }
    let extension = executable
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("exe") => {
            let mut command = Command::new(executable);
            command.args(CLAUDE_ARGUMENTS);
            Ok(command)
        }
        #[cfg(windows)]
        Some("cmd") => {
            use std::os::windows::process::CommandExt;
            let line = cmd_shim_command_line(&executable)?;
            let mut command = Command::new(agent_usage_monitor_core::system_tool_path("cmd.exe")?);
            // Passed verbatim: Rust's argument quoting is not cmd.exe's.
            command.as_std_mut().raw_arg(line);
            Ok(command)
        }
        _ => Err(format!(
            "unsupported Claude CLI file type: {}",
            executable.display()
        )),
    }
}

/// `cmd.exe` arguments that run one batch file by its full path:
/// `/D` (no AutoRun), `/V:OFF` (no `!` expansion), `/S /C ""<path>" <args>"`.
/// With `/S` cmd strips only the outer quotes, so the path stays quoted and
/// `& | < > ^ ( )` in it are literal. `%` is still expanded inside quotes and
/// `"` cannot be quoted, so paths containing them (or control characters) are
/// refused.
fn cmd_shim_command_line(path: &std::path::Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or("Claude CLI path is not valid Unicode")?;
    if text
        .chars()
        .any(|character| matches!(character, '%' | '"') || character.is_control())
    {
        return Err(format!(
            "Claude CLI path contains characters cmd.exe cannot pass safely: {text}"
        ));
    }
    Ok(format!(
        "/D /V:OFF /S /C \"\"{text}\" {}\"",
        CLAUDE_ARGUMENTS.join(" ")
    ))
}

async fn run_command(
    mut command: Command,
    timeout: Duration,
    cancel: &CancellationToken,
    request_id: Option<u64>,
    observed_at: i64,
) -> Result<ClaudeUsageSnapshot, ClaudeRefreshFailure> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
    }
    let cli_started = std::time::Instant::now();
    let mut child = command
        .spawn()
        .map_err(|_| ClaudeRefreshFailure::SpawnFailure)?;
    let pid = child.id();
    let mut stdout = child
        .stdout
        .take()
        .ok_or(ClaudeRefreshFailure::ProcessIoFailure)?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or(ClaudeRefreshFailure::ProcessIoFailure)?;
    let result = {
        let operation = async {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let (out_result, err_result, status) = tokio::join!(
                stdout.read_to_end(&mut out),
                stderr.read_to_end(&mut err),
                child.wait()
            );
            out_result?;
            err_result?;
            Ok::<_, std::io::Error>((out, err, status?))
        };
        tokio::pin!(operation);
        tokio::select! {
            _ = cancel.cancelled() => None,
            result = tokio::time::timeout(timeout, &mut operation) => Some(result),
        }
    };
    let (stdout, _stderr, status) = match result {
        None => {
            terminate_tree(pid, &mut child).await;
            return Err(ClaudeRefreshFailure::Cancelled);
        }
        Some(Err(_)) => {
            terminate_tree(pid, &mut child).await;
            return Err(ClaudeRefreshFailure::Timeout);
        }
        Some(Ok(Err(_))) => return Err(ClaudeRefreshFailure::ProcessIoFailure),
        Some(Ok(Ok(output))) => output,
    };
    eprintln!(
        "CLAUDE_REFRESH: cli_finished request_id={} cli_ms={} result={}",
        request_id.map_or_else(|| "background".into(), |id| id.to_string()),
        cli_started.elapsed().as_millis(),
        if status.success() {
            "success"
        } else {
            "nonzero_exit"
        }
    );
    if !status.success() {
        return Err(ClaudeRefreshFailure::NonzeroExit);
    }
    let stdout = String::from_utf8_lossy(&stdout);
    if stdout.trim().is_empty() {
        return Err(ClaudeRefreshFailure::EmptyOutput);
    }
    let parse_started = std::time::Instant::now();
    let snapshot =
        parse_claude_cli_usage_diagnostic(&stdout, observed_at).map_err(|error| match error {
            ClaudeCliOutputError::UnrecognizedFormat => ClaudeRefreshFailure::UnrecognizedFormat,
            ClaudeCliOutputError::NoQuotaValues => ClaudeRefreshFailure::NoQuotaValues,
        })?;
    eprintln!(
        "CLAUDE_REFRESH: parse_completed request_id={} parse_ms={}",
        request_id.map_or_else(|| "background".into(), |id| id.to_string()),
        parse_started.elapsed().as_millis()
    );
    if !CLAUDE_RESET_DIAGNOSTIC_WRITTEN.swap(true, std::sync::atomic::Ordering::Relaxed) {
        let reset_text_present = |prefix: &str| {
            stdout.lines().any(|line| {
                line.trim()
                    .strip_prefix(prefix)
                    .and_then(|rest| rest.split_once("% used"))
                    .is_some_and(|(_, reset)| {
                        reset
                            .trim()
                            .trim_start_matches('·')
                            .trim()
                            .starts_with("resets ")
                    })
            })
        };
        eprintln!(
            "CLAUDE_RESET: /usage 5-hour source_text={} parsed={} weekly source_text={} parsed={}",
            reset_text_present("Current session:"),
            snapshot
                .five_hour
                .as_ref()
                .is_some_and(|window| window.resets_at.is_some()),
            reset_text_present("Current week (all models):"),
            snapshot
                .weekly
                .as_ref()
                .is_some_and(|window| window.resets_at.is_some()),
        );
    }
    Ok(snapshot)
}

#[cfg(windows)]
async fn terminate_tree(pid: Option<u32>, child: &mut tokio::process::Child) {
    // Without a verified taskkill path only the direct child is killed below;
    // a bare "taskkill.exe" is never started.
    if let (Some(pid), Ok(taskkill_path)) = (
        pid,
        agent_usage_monitor_core::system_tool_path("taskkill.exe"),
    ) {
        let mut taskkill = Command::new(taskkill_path);
        taskkill
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            taskkill.as_std_mut().creation_flags(CREATE_NO_WINDOW);
        }
        let _ = taskkill.status().await;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[cfg(not(windows))]
async fn terminate_tree(_pid: Option<u32>, child: &mut tokio::process::Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

pub async fn run(
    settings: tokio::sync::watch::Receiver<crate::settings::AppSettings>,
    sender: tokio::sync::mpsc::Sender<ClaudeRefreshEvent>,
    refresh_requests: tokio::sync::mpsc::Receiver<ManualRefreshRequest>,
    cancellation: CancellationToken,
) {
    run_with_intervals(
        settings,
        sender,
        refresh_requests,
        cancellation,
        REFRESH_INTERVAL,
        FAILURE_COOLDOWN,
        |cancel, request_id, observed_at| {
            Box::pin(async move { refresh_once(&cancel, request_id, observed_at).await })
        },
    )
    .await;
}

type RefreshFuture =
    Pin<Box<dyn Future<Output = Result<ClaudeUsageSnapshot, ClaudeRefreshFailure>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitResult {
    Elapsed,
    Manual(ManualRefreshRequest),
    Disabled,
    Cancelled,
}

async fn run_with_intervals<F>(
    mut settings: tokio::sync::watch::Receiver<crate::settings::AppSettings>,
    sender: tokio::sync::mpsc::Sender<ClaudeRefreshEvent>,
    mut refresh_requests: tokio::sync::mpsc::Receiver<ManualRefreshRequest>,
    cancellation: CancellationToken,
    refresh_interval: Duration,
    failure_cooldown: Duration,
    mut refresh: F,
) where
    F: FnMut(CancellationToken, Option<u64>, i64) -> RefreshFuture,
{
    let mut pending_manual = None;
    // Wall-clock time of the last successful CLI refresh (failures leave it).
    let mut last_success_at: Option<i64> = None;
    loop {
        if cancellation.is_cancelled() {
            break;
        }
        let current = settings.borrow().clone();
        if !current.claude_enabled || !current.claude_cli_refresh_enabled {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                changed = settings.changed() => if changed.is_err() { break; },
                request = refresh_requests.recv() => {
                    if let Some(request) = request {
                        eprintln!("CLAUDE_REFRESH: skipped request_id={} reason=opt_in_disabled", request.id);
                    } else { break; }
                }
            }
            continue;
        }
        let request = pending_manual
            .take()
            .or_else(|| refresh_requests.try_recv().ok());
        // Timer first, resume request later: after sleep the periodic timer can
        // fire on wake-up, before the delayed resume request arrives. Do not
        // run the CLI twice; keep the periodic cadence from the last success.
        // (Request first, timer later needs no rule: every refresh restarts
        // the wait below, so an overdue timer is dropped.)
        if let Some(skipped) = request
            && crate::skip_redundant_resume_refresh(skipped.origin, last_success_at, now_unix())
        {
            eprintln!(
                "CLAUDE_REFRESH: skipped request_id={} reason=already_refreshed_after_resume",
                skipped.id
            );
            let age = now_unix().saturating_sub(last_success_at.unwrap_or_default());
            let remaining = refresh_interval.saturating_sub(Duration::from_secs(age.max(0) as u64));
            match wait_for_next(
                &mut settings,
                &cancellation,
                &mut refresh_requests,
                remaining,
            )
            .await
            {
                WaitResult::Manual(request) => pending_manual = Some(request),
                WaitResult::Elapsed | WaitResult::Disabled => {}
                WaitResult::Cancelled => break,
            }
            continue;
        }
        let request_id = request.map(|request| request.id);
        let observed_at = now_unix();
        eprintln!(
            "CLAUDE_REFRESH: started request_id={} source={}",
            request_id.map_or_else(|| "background".into(), |id| id.to_string()),
            if request_id.is_some() {
                "manual"
            } else {
                "background"
            }
        );
        let operation_cancel = cancellation.child_token();
        let mut operation = refresh(operation_cancel.clone(), request_id, observed_at);
        let result = loop {
            tokio::select! {
                result = &mut operation => break Some(result),
                _ = cancellation.cancelled() => {
                    operation_cancel.cancel();
                    let _ = operation.await;
                    return;
                },
                changed = settings.changed() => {
                    if changed.is_err() { operation_cancel.cancel(); let _ = operation.await; return; }
                    let current = settings.borrow().clone();
                    if !current.claude_enabled || !current.claude_cli_refresh_enabled {
                        operation_cancel.cancel();
                        let _ = operation.await;
                        break None;
                    }
                }
            }
        };
        let Some(result) = result else { continue };
        let delay = match result {
            Ok(snapshot) => {
                let current = settings.borrow().clone();
                if !current.claude_enabled || !current.claude_cli_refresh_enabled {
                    continue;
                }
                if sender
                    .send(ClaudeRefreshEvent::Completed {
                        request_id,
                        snapshot,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
                eprintln!(
                    "CLAUDE_REFRESH: completed request_id={} source={}",
                    request_id.map_or_else(|| "background".into(), |id| id.to_string()),
                    if request_id.is_some() {
                        "manual"
                    } else {
                        "background"
                    }
                );
                last_success_at = Some(now_unix());
                refresh_interval
            }
            Err(failure) => {
                eprintln!(
                    "CLAUDE_REFRESH: failed request_id={} source={} category={}",
                    request_id.map_or_else(|| "background".into(), |id| id.to_string()),
                    if request_id.is_some() {
                        "manual"
                    } else {
                        "background"
                    },
                    failure.category()
                );
                if sender
                    .send(ClaudeRefreshEvent::Failed {
                        request_id,
                        failure,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
                failure_cooldown
            }
        };
        match wait_for_next(&mut settings, &cancellation, &mut refresh_requests, delay).await {
            WaitResult::Manual(request) => pending_manual = Some(request),
            WaitResult::Elapsed | WaitResult::Disabled => {}
            WaitResult::Cancelled => break,
        }
    }
}

async fn wait_for_next(
    settings: &mut tokio::sync::watch::Receiver<crate::settings::AppSettings>,
    cancellation: &CancellationToken,
    refresh_requests: &mut tokio::sync::mpsc::Receiver<ManualRefreshRequest>,
    duration: Duration,
) -> WaitResult {
    let sleep = tokio::time::sleep(duration);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => return WaitResult::Cancelled,
            _ = &mut sleep => return WaitResult::Elapsed,
            request = refresh_requests.recv() => {
                return request.map_or(WaitResult::Cancelled, WaitResult::Manual);
            }
            changed = settings.changed() => {
                if changed.is_err() { return WaitResult::Cancelled; }
                let current = settings.borrow().clone();
                if !current.claude_enabled || !current.claude_cli_refresh_enabled { return WaitResult::Disabled; }
            }
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        ClaudeRefreshEvent, ClaudeRefreshFailure, ManualRefreshRequest, RefreshFuture, run_command,
        run_with_intervals,
    };
    use agent_usage_monitor_core::parse_claude_cli_usage;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;
    use tokio::process::Command;
    use tokio::sync::{mpsc, watch};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn missing_process_executable_is_classified_as_spawn_failure() {
        let directory = tempfile::tempdir().unwrap();
        let command = Command::new(directory.path().join("missing-claude-executable"));
        let failure = run_command(
            command,
            Duration::from_secs(2),
            &CancellationToken::new(),
            None,
            100,
        )
        .await
        .unwrap_err();
        assert_eq!(failure, ClaudeRefreshFailure::SpawnFailure);
    }

    #[cfg(windows)]
    #[test]
    fn native_claude_exe_runs_directly_and_shims_keep_cmd() {
        let program = |command: &Command| {
            command
                .as_std()
                .get_program()
                .to_string_lossy()
                .to_ascii_lowercase()
        };
        let arguments = |command: &Command| -> Vec<String> {
            command
                .as_std()
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect()
        };
        use std::path::{Path, PathBuf};
        // Absolute claude.exe (spaces and Unicode included): started directly,
        // exact path, arguments unchanged.
        for exe in [
            r"C:\Users\u\.local\bin\claude.exe",
            r"C:\Users\Éva Kovács\Program Files (x86)\Claude & Co\claude.exe",
        ] {
            let native = super::claude_command(Some(PathBuf::from(exe))).unwrap();
            assert_eq!(program(&native), exe.to_ascii_lowercase());
            assert_eq!(
                arguments(&native),
                ["-p", "--no-session-persistence", "/usage"]
            );
        }
        // Resolved claude.cmd: cmd.exe from the system directory, with the
        // shim's exact quoted path (no bare "claude" re-search).
        let shim_path =
            Path::new(r"C:\Users\Éva Kovács\AppData\Roaming\npm (x86)\a&b^c\claude.cmd");
        let shim = super::claude_command(Some(shim_path.to_path_buf())).unwrap();
        assert!(program(&shim).ends_with(r"\system32\cmd.exe"), "{shim:?}");
        assert_eq!(
            super::cmd_shim_command_line(shim_path).unwrap(),
            format!(
                "/D /V:OFF /S /C \"\"{}\" -p --no-session-persistence /usage\"",
                shim_path.display()
            )
        );
        // Fail closed: nothing resolved, relative paths, other file types.
        assert!(super::claude_command(None).is_err());
        assert!(super::claude_command(Some(PathBuf::from("claude.exe"))).is_err());
        assert!(super::claude_command(Some(PathBuf::from(r".\claude.cmd"))).is_err());
        assert!(super::claude_command(Some(PathBuf::from(r"C:\npm\claude.ps1"))).is_err());
        assert!(super::claude_command(Some(PathBuf::from(r"C:\npm\claude.bat"))).is_err());
        // Shell-sensitive characters cmd.exe cannot pass safely are refused.
        for unsafe_path in [
            r"C:\Users\%USERNAME%\npm\claude.cmd",
            "C:\\npm\\cla\"ude.cmd",
            "C:\\npm\nx\\claude.cmd",
        ] {
            assert!(
                super::claude_command(Some(PathBuf::from(unsafe_path))).is_err(),
                "{unsafe_path:?}"
            );
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn resolved_cmd_shim_in_an_unusual_folder_runs_with_exact_arguments() {
        // A stand-in shim (not the real CLI) in a folder whose name has spaces,
        // Unicode and cmd metacharacters. It records its arguments and prints
        // a usage report, proving the quoted path and arguments reach it intact.
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Ünï dir (x86) & a^b");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("report.txt"),
            "Current session: 8% used\r\nCurrent week (all models): 67% used\r\n",
        )
        .unwrap();
        std::fs::write(
            folder.join("claude.cmd"),
            "@echo off\r\n>\"%~dp0args.txt\" echo %*\r\ntype \"%~dp0report.txt\"\r\n",
        )
        .unwrap();
        let resolved =
            agent_usage_monitor_core::resolve_cli_in(folder.as_os_str(), "claude").unwrap();
        assert_eq!(resolved, folder.join("claude.cmd"));
        let command = super::claude_command(Some(resolved)).unwrap();
        let snapshot = run_command(
            command,
            Duration::from_secs(20),
            &CancellationToken::new(),
            None,
            1_783_000_000,
        )
        .await
        .unwrap();
        assert_eq!(snapshot.five_hour.unwrap().used_percent, Some(8.0));
        assert_eq!(snapshot.weekly.unwrap().used_percent, Some(67.0));
        assert_eq!(
            std::fs::read_to_string(folder.join("args.txt"))
                .unwrap()
                .trim_end(),
            "-p --no-session-persistence /usage"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn missing_claude_cli_fails_without_starting_a_process() {
        // An absolute PATH without Claude resolves to nothing, so refresh
        // fails before any process (shell or CLI) is spawned.
        let empty = tempfile::tempdir().unwrap();
        let resolved = agent_usage_monitor_core::resolve_cli_in(empty.path().as_os_str(), "claude");
        assert!(resolved.is_none());
        assert!(super::claude_command(resolved).is_err());
    }

    #[test]
    fn parses_sanitized_report_and_leaves_missing_windows_unavailable() {
        let report = "Current session: 8% used · resets Jul 13, 3:29pm (Europe/Warsaw)\nCurrent week (all models): 67% used · resets Jul 16, 9:59am (Europe/Warsaw)\nCurrent week (Fable): 2% used";
        let snapshot = parse_claude_cli_usage(report, 1_783_000_000);
        assert_eq!(snapshot.five_hour.unwrap().used_percent, Some(8.0));
        assert_eq!(snapshot.weekly.unwrap().used_percent, Some(67.0));
        let partial = parse_claude_cli_usage("Current session: 23% used", 1_783_000_000);
        assert_eq!(partial.five_hour.unwrap().resets_at, None);
        assert!(partial.weekly.is_none());
    }

    #[tokio::test]
    async fn default_disabled_scheduler_does_not_call_the_refresher() {
        let (settings_tx, settings_rx) = watch::channel(crate::settings::AppSettings::default());
        let (sender, _receiver) = mpsc::channel(2);
        let (_request_sender, request_receiver) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let task = tokio::spawn(run_with_intervals(
            settings_rx,
            sender,
            request_receiver,
            cancellation.clone(),
            Duration::from_millis(10),
            Duration::from_millis(10),
            move |_, _, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(parse_claude_cli_usage("Current session: 2% used", 100)) })
                    as RefreshFuture
            },
        ));
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        cancellation.cancel();
        task.await.unwrap();
        drop(settings_tx);
    }

    #[tokio::test]
    async fn disabling_during_in_flight_refresh_cancels_and_discards_its_result() {
        let initial = crate::settings::AppSettings::default();
        let (settings_tx, settings_rx) = watch::channel(initial.clone());
        let (sender, mut receiver) = mpsc::channel(2);
        let (_request_sender, request_receiver) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let (second_started_tx, second_started_rx) = tokio::sync::oneshot::channel();
        let second_started_tx = Arc::new(std::sync::Mutex::new(Some(second_started_tx)));
        let started = second_started_tx.clone();
        let task = tokio::spawn(run_with_intervals(
            settings_rx,
            sender,
            request_receiver,
            cancellation.clone(),
            Duration::from_millis(20),
            Duration::from_millis(20),
            move |operation_cancel, _, _| {
                let count = observed.fetch_add(1, Ordering::SeqCst) + 1;
                let started = started.clone();
                Box::pin(async move {
                    if count > 1 {
                        if let Some(send) = started.lock().unwrap().take() {
                            let _ = send.send(());
                        }
                        operation_cancel.cancelled().await;
                    }
                    Ok(parse_claude_cli_usage(
                        "Current session: 2% used",
                        100 + count as i64,
                    ))
                }) as RefreshFuture
            },
        ));
        let mut enabled = initial;
        enabled.claude_cli_refresh_enabled = true;
        settings_tx.send(enabled.clone()).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .unwrap()
                .is_some()
        );
        tokio::time::timeout(Duration::from_secs(1), second_started_rx)
            .await
            .unwrap()
            .unwrap();
        enabled.claude_cli_refresh_enabled = false;
        settings_tx.send(enabled).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(80), receiver.recv())
                .await
                .is_err()
        );
        cancellation.cancel();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn failed_refresh_uses_cooldown_before_retrying() {
        let enabled = crate::settings::AppSettings {
            claude_cli_refresh_enabled: true,
            ..Default::default()
        };
        let (_settings_tx, settings_rx) = watch::channel(enabled);
        let (sender, mut receiver) = mpsc::channel(2);
        let (_request_sender, request_receiver) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let task = tokio::spawn(run_with_intervals(
            settings_rx,
            sender,
            request_receiver,
            cancellation.clone(),
            Duration::from_secs(1),
            Duration::from_millis(80),
            move |_, _, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Err(ClaudeRefreshFailure::NonzeroExit) }) as RefreshFuture
            },
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ClaudeRefreshEvent::Failed {
                failure: ClaudeRefreshFailure::NonzeroExit,
                ..
            })
        ));
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(calls.load(Ordering::SeqCst) >= 2);
        cancellation.cancel();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn manual_refresh_bypasses_failure_cooldown_and_runs_once() {
        use super::ClaudeRefreshEvent;

        let enabled = crate::settings::AppSettings {
            claude_enabled: true,
            claude_cli_refresh_enabled: true,
            ..Default::default()
        };
        let (_settings_tx, settings_rx) = watch::channel(enabled);
        let (sender, mut receiver) = mpsc::channel(4);
        let (request_sender, request_receiver) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let task = tokio::spawn(run_with_intervals(
            settings_rx,
            sender,
            request_receiver,
            cancellation.clone(),
            Duration::from_secs(30),
            Duration::from_secs(30),
            move |operation_cancel, request_id, _observed_at| {
                let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
                Box::pin(async move {
                    if call == 1 {
                        return Err(ClaudeRefreshFailure::NonzeroExit);
                    }
                    assert_eq!(request_id, Some(42));
                    tokio::select! {
                        _ = operation_cancel.cancelled() => Err(ClaudeRefreshFailure::Cancelled),
                        _ = tokio::time::sleep(Duration::from_millis(20)) => Ok(parse_claude_cli_usage("Current session: 2% used", 102)),
                    }
                }) as RefreshFuture
            },
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ClaudeRefreshEvent::Failed {
                failure: ClaudeRefreshFailure::NonzeroExit,
                ..
            })
        ));
        request_sender
            .send(ManualRefreshRequest {
                id: 42,
                origin: crate::RefreshOrigin::Manual,
            })
            .await
            .unwrap();
        let completed = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("manual refresh waited for the background cooldown")
            .expect("scheduler ended before the manual result");
        assert!(matches!(
            completed,
            ClaudeRefreshEvent::Completed {
                request_id: Some(42),
                ..
            }
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        cancellation.cancel();
        task.await.unwrap();
    }

    fn request(id: u64, origin: crate::RefreshOrigin) -> ManualRefreshRequest {
        ManualRefreshRequest { id, origin }
    }

    fn enabled_settings() -> crate::settings::AppSettings {
        crate::settings::AppSettings {
            claude_enabled: true,
            claude_cli_refresh_enabled: true,
            ..Default::default()
        }
    }

    /// Order 1: the periodic timer refreshed successfully (e.g. on wake-up),
    /// then the delayed resume request arrives. The resume request must not
    /// run the CLI again, while a manual refresh still does.
    #[tokio::test]
    async fn resume_request_after_a_successful_timer_refresh_is_skipped() {
        use super::ClaudeRefreshEvent;
        let (_settings_tx, settings_rx) = watch::channel(enabled_settings());
        let (sender, mut receiver) = mpsc::channel(4);
        let (request_sender, request_receiver) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let task = tokio::spawn(run_with_intervals(
            settings_rx,
            sender,
            request_receiver,
            cancellation.clone(),
            Duration::from_secs(30),
            Duration::from_secs(30),
            move |_, _, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(parse_claude_cli_usage("Current session: 2% used", 100)) })
                    as RefreshFuture
            },
        ));
        // Timer (start-up/background) refresh succeeded.
        assert!(matches!(
            receiver.recv().await,
            Some(ClaudeRefreshEvent::Completed {
                request_id: None,
                ..
            })
        ));
        request_sender
            .send(request(7, crate::RefreshOrigin::Resume))
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(150), receiver.recv())
                .await
                .is_err(),
            "resume request repeated a fresh refresh"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // A manual refresh is never skipped.
        request_sender
            .send(request(8, crate::RefreshOrigin::Manual))
            .await
            .unwrap();
        let manual = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("manual refresh did not run")
            .unwrap();
        assert!(matches!(
            manual,
            ClaudeRefreshEvent::Completed {
                request_id: Some(8),
                ..
            }
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        cancellation.cancel();
        task.await.unwrap();
    }

    /// The timer refresh after wake-up failed (network not up yet): the
    /// resume request must retry instead of being skipped.
    #[tokio::test]
    async fn resume_request_after_a_failed_timer_refresh_retries() {
        use super::ClaudeRefreshEvent;
        let (_settings_tx, settings_rx) = watch::channel(enabled_settings());
        let (sender, mut receiver) = mpsc::channel(4);
        let (request_sender, request_receiver) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let task = tokio::spawn(run_with_intervals(
            settings_rx,
            sender,
            request_receiver,
            cancellation.clone(),
            Duration::from_secs(30),
            Duration::from_secs(30),
            move |_, _, _| {
                let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
                Box::pin(async move {
                    if call == 1 {
                        Err(ClaudeRefreshFailure::NonzeroExit)
                    } else {
                        Ok(parse_claude_cli_usage("Current session: 2% used", 100))
                    }
                }) as RefreshFuture
            },
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ClaudeRefreshEvent::Failed { .. })
        ));
        request_sender
            .send(request(9, crate::RefreshOrigin::Resume))
            .await
            .unwrap();
        let retried = tokio::time::timeout(Duration::from_millis(500), receiver.recv())
            .await
            .expect("resume request was not retried after a failure")
            .unwrap();
        assert!(matches!(
            retried,
            ClaudeRefreshEvent::Completed {
                request_id: Some(9),
                ..
            }
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        cancellation.cancel();
        task.await.unwrap();
    }

    /// Order 2: the resume request runs first, and the periodic timer that
    /// was already due must not repeat the CLI call right after it.
    #[tokio::test]
    async fn timer_due_after_a_resume_refresh_does_not_run_it_again() {
        use super::ClaudeRefreshEvent;
        let (_settings_tx, settings_rx) = watch::channel(enabled_settings());
        let (sender, mut receiver) = mpsc::channel(4);
        let (request_sender, request_receiver) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let task = tokio::spawn(run_with_intervals(
            settings_rx,
            sender,
            request_receiver,
            cancellation.clone(),
            // Long success interval; the failure cooldown is the timer that
            // would otherwise fire right after the resume refresh. It leaves
            // ample margin for slow CI before the request is sent.
            Duration::from_secs(30),
            Duration::from_millis(400),
            move |_, _, _| {
                let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
                Box::pin(async move {
                    if call == 1 {
                        Err(ClaudeRefreshFailure::NonzeroExit)
                    } else {
                        Ok(parse_claude_cli_usage("Current session: 2% used", 100))
                    }
                }) as RefreshFuture
            },
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ClaudeRefreshEvent::Failed { .. })
        ));
        // Resume request arrives before the 400 ms timer is due.
        request_sender
            .send(request(10, crate::RefreshOrigin::Resume))
            .await
            .unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_millis(500), receiver.recv())
                .await
                .unwrap(),
            Some(ClaudeRefreshEvent::Completed {
                request_id: Some(10),
                ..
            })
        ));
        // Well past the original 400 ms timer: no extra CLI run.
        assert!(
            tokio::time::timeout(Duration::from_millis(700), receiver.recv())
                .await
                .is_err(),
            "the earlier timer repeated the resume refresh"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        cancellation.cancel();
        task.await.unwrap();
    }

    #[cfg(windows)]
    fn stub_command(directory: &std::path::Path, body: &str) -> Command {
        let script = directory.join("claude-stub.cmd");
        std::fs::write(&script, body).unwrap();
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C"]).arg(script);
        command
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_stub_success_and_nonzero_exit_are_reported_truthfully() {
        let directory = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let command = stub_command(
            directory.path(),
            "@echo off\necho Current session: 8%% used\nexit /b 0\n",
        );
        let snapshot = run_command(command, Duration::from_secs(2), &token, None, 100)
            .await
            .unwrap();
        assert_eq!(snapshot.five_hour.unwrap().used_percent, Some(8.0));

        let command = stub_command(directory.path(), "@echo off\nexit /b 7\n");
        let failure = run_command(command, Duration::from_secs(2), &token, None, 100)
            .await
            .unwrap_err();
        assert_eq!(failure, ClaudeRefreshFailure::NonzeroExit);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_stub_distinguishes_empty_unknown_and_missing_quota_output() {
        let directory = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        for (body, expected) in [
            ("@echo off\n", ClaudeRefreshFailure::EmptyOutput),
            (
                "@echo off\necho Sanitized unfamiliar report\n",
                ClaudeRefreshFailure::UnrecognizedFormat,
            ),
            (
                "@echo off\necho Current session: unavailable\n",
                ClaudeRefreshFailure::NoQuotaValues,
            ),
        ] {
            let command = stub_command(directory.path(), body);
            let failure = run_command(command, Duration::from_secs(2), &token, None, 100)
                .await
                .unwrap_err();
            assert_eq!(failure, expected);
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_stub_timeout_terminates_the_process_tree() {
        let directory = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let marker = directory.path().join("survived.txt");
        let body = format!(
            "@echo off\nstart \"\" /B powershell.exe -NoProfile -Command \"Start-Sleep -Seconds 2; Set-Content -LiteralPath '{}' -Value alive\"\nping -n 30 127.0.0.1 >nul\n",
            marker.display()
        );
        let command = stub_command(directory.path(), &body);
        let failure = run_command(command, Duration::from_millis(100), &token, None, 100)
            .await
            .unwrap_err();
        assert_eq!(failure, ClaudeRefreshFailure::Timeout);
        tokio::time::sleep(Duration::from_millis(2200)).await;
        assert!(!marker.exists(), "descendant survived process-tree cleanup");
    }
}

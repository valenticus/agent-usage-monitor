use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

use crate::ManualRefreshRequest;
use agent_usage_monitor_core::{ClaudeUsageSnapshot, parse_claude_cli_usage};
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
        timed_out: bool,
    },
}

pub async fn refresh_once(
    cancel: &CancellationToken,
    request_id: Option<u64>,
    observed_at: i64,
) -> Result<ClaudeUsageSnapshot, String> {
    let command = if cfg!(windows) {
        let mut command = Command::new("cmd.exe");
        command.args([
            "/D",
            "/S",
            "/C",
            "claude -p --no-session-persistence /usage",
        ]);
        command
    } else {
        let mut command = Command::new("claude");
        command.args(["-p", "--no-session-persistence", "/usage"]);
        command
    };
    run_command(command, TIMEOUT, cancel, request_id, observed_at).await
}

async fn run_command(
    mut command: Command,
    timeout: Duration,
    cancel: &CancellationToken,
    request_id: Option<u64>,
    observed_at: i64,
) -> Result<ClaudeUsageSnapshot, String> {
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
        .map_err(|_| "could not start Claude CLI".to_string())?;
    let pid = child.id();
    let mut stdout = child.stdout.take().ok_or("Claude CLI stdout unavailable")?;
    let mut stderr = child.stderr.take().ok_or("Claude CLI stderr unavailable")?;
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
            return Err("Claude CLI refresh cancelled".into());
        }
        Some(Err(_)) => {
            terminate_tree(pid, &mut child).await;
            eprintln!(
                "CLAUDE_REFRESH: cli_timed_out request_id={} timeout_ms={}",
                request_id.map_or_else(|| "background".into(), |id| id.to_string()),
                timeout.as_millis()
            );
            return Err(format!(
                "Claude CLI refresh timed out after {} seconds",
                timeout.as_secs()
            ));
        }
        Some(Ok(Err(_))) => return Err("Claude CLI process failed".into()),
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
        return Err(format!("Claude CLI exited with status {status}"));
    }
    let stdout = String::from_utf8_lossy(&stdout);
    let parse_started = std::time::Instant::now();
    let snapshot = parse_claude_cli_usage(&stdout, observed_at);
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
    if snapshot.five_hour.is_none() && snapshot.weekly.is_none() {
        eprintln!("CLAUDE_SOURCE: /usage contained no recognized quota lines");
        return Err("Claude CLI returned no recognized quota values".into());
    }
    Ok(snapshot)
}

#[cfg(windows)]
async fn terminate_tree(pid: Option<u32>, child: &mut tokio::process::Child) {
    if let Some(pid) = pid {
        let mut taskkill = Command::new("taskkill.exe");
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

type RefreshFuture = Pin<Box<dyn Future<Output = Result<ClaudeUsageSnapshot, String>> + Send>>;

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
                refresh_interval
            }
            Err(error) => {
                let timed_out = error.contains("timed out");
                let kind = if timed_out { "timed_out" } else { "failed" };
                eprintln!(
                    "CLAUDE_REFRESH: {kind} request_id={} source={}",
                    request_id.map_or_else(|| "background".into(), |id| id.to_string()),
                    if request_id.is_some() {
                        "manual"
                    } else {
                        "background"
                    }
                );
                if sender
                    .send(ClaudeRefreshEvent::Failed {
                        request_id,
                        timed_out,
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
    use super::{ManualRefreshRequest, RefreshFuture, run_command, run_with_intervals};
    use agent_usage_monitor_core::parse_claude_cli_usage;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;
    use tokio::process::Command;
    use tokio::sync::{mpsc, watch};
    use tokio_util::sync::CancellationToken;

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
            Duration::from_secs(1),
            Duration::from_millis(80),
            move |_, _, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Err("stub failure".into()) }) as RefreshFuture
            },
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
                        return Err("stub failure".into());
                    }
                    assert_eq!(request_id, Some(42));
                    tokio::select! {
                        _ = operation_cancel.cancelled() => Err("cancelled".into()),
                        _ = tokio::time::sleep(Duration::from_millis(20)) => Ok(parse_claude_cli_usage("Current session: 2% used", 102)),
                    }
                }) as RefreshFuture
            },
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ClaudeRefreshEvent::Failed { .. })
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
        assert!(failure.contains("exited with status"));
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
        assert!(failure.contains("timed out"));
        tokio::time::sleep(Duration::from_millis(2200)).await;
        assert!(!marker.exists(), "descendant survived process-tree cleanup");
    }
}

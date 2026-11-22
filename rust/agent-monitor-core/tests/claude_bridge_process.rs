#![cfg(windows)]

use std::process::Stdio;
use std::time::Duration;

use agent_usage_monitor_core::{
    CLAUDE_PIPE_NAME, CLAUDE_PIPE_OVERRIDE_ENV, CLAUDE_TEST_PIPE_PREFIX, resolve_client_pipe_name,
    run_claude_status_pipe_named,
};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

/// A pipe name owned by this test run only. The bridge child receives it via
/// the test override, so neither the server nor the bridge ever touches the
/// production pipe of a running AUM.
fn isolated_pipe_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .subsec_nanos();
    let name = format!("{CLAUDE_TEST_PIPE_PREFIX}{}-{nanos}", std::process::id());
    assert_ne!(name, CLAUDE_PIPE_NAME);
    assert_eq!(
        resolve_client_pipe_name(Some(&name)).as_deref(),
        Ok(name.as_str()),
        "the bridge must accept the isolated name, never fall back"
    );
    name
}

#[tokio::test]
async fn status_line_executable_forwards_allowlisted_data_and_exits_cleanly() {
    let pipe_name = isolated_pipe_name();
    let cancellation = CancellationToken::new();
    let (sender, mut receiver) = mpsc::channel(1);
    let server_token = cancellation.clone();
    let server_pipe = pipe_name.clone();
    let server = tokio::spawn(async move {
        run_claude_status_pipe_named(&server_pipe, sender, server_token).await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-usage-monitor-claude-bridge"))
        .env(CLAUDE_PIPE_OVERRIDE_ENV, &pipe_name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("bridge process should start");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_secs();
    let input = format!(
        r#"{{"transcript_path":"not-opened","session_id":"ignored","rate_limits":{{"five_hour":{{"used_percentage":25.0,"resets_at":{}}},"seven_day":{{}}}}}}"#,
        now + 3_600
    );
    child
        .stdin
        .take()
        .expect("bridge stdin should be piped")
        .write_all(input.as_bytes())
        .await
        .expect("status-line input should be written");

    let output = timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .expect("bridge process should finish promptly")
        .expect("bridge process should exit");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "5 год 75% · тиждень —"
    );
    let received = timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("pipe snapshot should arrive")
        .expect("pipe server should remain connected");
    assert_eq!(received.five_hour_remaining_percent, Some(75.0));
    assert_eq!(received.weekly_remaining_percent, None);
    assert_eq!(received.five_hour_reset_at, Some(now as i64 + 3_600));

    cancellation.cancel();
    timeout(Duration::from_secs(1), server)
        .await
        .expect("pipe server should observe cancellation")
        .expect("pipe task should not panic")
        .expect("pipe server should stop cleanly");
}

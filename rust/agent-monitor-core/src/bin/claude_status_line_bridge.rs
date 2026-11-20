use std::io::{self, Read};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use agent_usage_monitor_core::{ClaudeStatusLineSnapshot, send_claude_status_snapshot};

const EXIT_SUCCESS: u8 = 0;
const EXIT_INPUT_ERROR: u8 = 2;
const EXIT_MONITOR_UNAVAILABLE: u8 = 3;
const EXIT_PIPE_CONNECTION_FAILED: u8 = 4;
const EXIT_DELIVERY_FAILED: u8 = 5;
const EXIT_INTERNAL_ERROR: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeError {
    Input,
    MonitorUnavailable,
    PipeConnectionFailure,
    Delivery,
    Internal,
}

const MAX_STATUS_LINE_INPUT_BYTES: usize = 64 * 1024;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::from(EXIT_SUCCESS),
        Err(error) => {
            let (code, message) = match error {
                BridgeError::Input => (EXIT_INPUT_ERROR, "invalid status-line input"),
                BridgeError::MonitorUnavailable => {
                    (EXIT_MONITOR_UNAVAILABLE, "AUM monitor is unavailable")
                }
                BridgeError::PipeConnectionFailure => {
                    (EXIT_PIPE_CONNECTION_FAILED, "AUM pipe connection failed")
                }
                BridgeError::Delivery => (EXIT_DELIVERY_FAILED, "AUM pipe delivery failed"),
                BridgeError::Internal => (EXIT_INTERNAL_ERROR, "bridge internal error"),
            };
            eprintln!("aum-claude-bridge: {message}");
            ExitCode::from(code)
        }
    }
}

fn run() -> Result<(), BridgeError> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take((MAX_STATUS_LINE_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| BridgeError::Input)?;
    if bytes.is_empty() || bytes.len() > MAX_STATUS_LINE_INPUT_BYTES {
        return Err(BridgeError::Input);
    }
    let input = std::str::from_utf8(&bytes).map_err(|_| BridgeError::Input)?;
    let snapshot = ClaudeStatusLineSnapshot::from_status_line_json(input, now_unix_seconds())
        .map_err(|_| BridgeError::Input)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| BridgeError::Internal)?;
    runtime
        .block_on(send_claude_status_snapshot(&snapshot))
        .map_err(|error| classify_send_error(error.kind()))?;
    println!(
        "5 год {} · тиждень {}",
        display_percent(snapshot.five_hour_remaining_percent),
        display_percent(snapshot.weekly_remaining_percent),
    );
    Ok(())
}

fn display_percent(value: Option<f64>) -> String {
    value
        .map(|percent| format!("{percent:.0}%"))
        .unwrap_or_else(|| "—".into())
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn classify_send_error(kind: io::ErrorKind) -> BridgeError {
    match kind {
        io::ErrorKind::NotFound | io::ErrorKind::TimedOut => BridgeError::MonitorUnavailable,
        io::ErrorKind::ConnectionRefused | io::ErrorKind::PermissionDenied => {
            BridgeError::PipeConnectionFailure
        }
        _ => BridgeError::Delivery,
    }
}

#[cfg(test)]
mod tests {
    use super::{BridgeError, classify_send_error, display_percent, now_unix_seconds};
    use agent_usage_monitor_core::ClaudeStatusLineSnapshot;
    use std::io;

    #[test]
    fn status_line_keeps_missing_percentages_unavailable() {
        assert_eq!(display_percent(Some(75.4)), "75%");
        assert_eq!(display_percent(None), "—");
    }

    #[test]
    fn unavailable_listener_and_failed_delivery_have_distinct_categories() {
        assert_eq!(
            classify_send_error(io::ErrorKind::NotFound),
            BridgeError::MonitorUnavailable
        );
        assert_eq!(
            classify_send_error(io::ErrorKind::TimedOut),
            BridgeError::MonitorUnavailable
        );
        assert_eq!(
            classify_send_error(io::ErrorKind::InvalidData),
            BridgeError::Delivery
        );
        assert_eq!(
            classify_send_error(io::ErrorKind::ConnectionRefused),
            BridgeError::PipeConnectionFailure
        );
        assert_eq!(
            classify_send_error(io::ErrorKind::BrokenPipe),
            BridgeError::Delivery
        );
    }

    #[test]
    fn a_parsed_snapshot_is_not_reported_as_delivered_when_send_fails() {
        let snapshot = ClaudeStatusLineSnapshot::from_status_line_json(
            r#"{"rate_limits":{"five_hour":{"used_percentage":25}}}"#,
            now_unix_seconds(),
        )
        .unwrap();
        let send_result: io::Result<()> = Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pipe rejected snapshot",
        ));
        let reported = send_result.map_err(|error| classify_send_error(error.kind()));

        assert_eq!(snapshot.five_hour_remaining_percent, Some(75.0));
        assert_eq!(reported, Err(BridgeError::Delivery));
    }
}

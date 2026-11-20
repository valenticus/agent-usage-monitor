use std::io::{self, Read};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use agent_usage_monitor_core::{ClaudeStatusLineSnapshot, send_claude_status_snapshot};

const MAX_STATUS_LINE_INPUT_BYTES: usize = 64 * 1024;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

fn run() -> Result<(), ()> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take((MAX_STATUS_LINE_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.is_empty() || bytes.len() > MAX_STATUS_LINE_INPUT_BYTES {
        return Err(());
    }
    let input = std::str::from_utf8(&bytes).map_err(|_| ())?;
    let snapshot = ClaudeStatusLineSnapshot::from_status_line_json(input, now_unix_seconds())
        .map_err(|_| ())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ())?;
    let _ = runtime.block_on(send_claude_status_snapshot(&snapshot));
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

#[cfg(test)]
mod tests {
    use super::display_percent;

    #[test]
    fn status_line_keeps_missing_percentages_unavailable() {
        assert_eq!(display_percent(Some(75.4)), "75%");
        assert_eq!(display_percent(None), "—");
    }
}

//! Provider-neutral rate-limit models and safe provider clients for Agent Usage Monitor.

mod alerts;
mod claude_pipe;
mod claude_status_line;
mod claude_usage;
mod codex;
mod models;
mod provider;

pub use alerts::{
    AlertEngine, AlertEngineState, AlertEvent, AlertKey, AlertSettings, AlertSeverity,
    AlertStateStore, AlertStateStoreError, AlertWindow, AlertWindowState,
    minimum_five_hour_remaining,
};
pub use claude_pipe::{
    CLAUDE_PIPE_NAME, CLAUDE_PIPE_OVERRIDE_ENV, CLAUDE_TEST_PIPE_PREFIX, resolve_client_pipe_name,
    run_claude_status_pipe, run_claude_status_pipe_named, send_claude_status_snapshot,
};
pub use claude_status_line::ClaudeStatusLineSnapshot;
pub use claude_usage::{
    ClaudeUsageSnapshot, ClaudeUsageSource, ClaudeUsageWindow, parse_claude_cli_usage,
};
pub use codex::{CodexAppServerClient, CodexClientConfig, CodexRateLimitProvider};
pub use models::{
    ProviderError, ProviderSnapshot, ProviderStatus, RateLimitSnapshot, RateLimitWindow,
};
pub use provider::{ProviderState, RateLimitProvider};

/// Format a non-negative countdown as `2d 3h`, `4h 5m`, `6m 7s`, or `0s`.
pub fn format_countdown(seconds: i64) -> String {
    let seconds = seconds.max(0) as u64;
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    let seconds = seconds % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::format_countdown;

    #[test]
    fn formats_countdown_boundaries_and_past_resets() {
        assert_eq!(format_countdown(-5), "0s");
        assert_eq!(format_countdown(59), "59s");
        assert_eq!(format_countdown(60), "1m 0s");
        assert_eq!(format_countdown(3600), "1h 0m");
        assert_eq!(format_countdown(86_400), "1d 0h");
    }
}

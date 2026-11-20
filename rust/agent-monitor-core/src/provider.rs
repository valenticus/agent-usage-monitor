use std::future::Future;

use tokio_util::sync::CancellationToken;

use crate::models::{ProviderError, ProviderSnapshot, ProviderStatus, RateLimitSnapshot};

/// A provider reads its own rate limits and reports a provider-local snapshot.
pub trait RateLimitProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn read_rate_limits(
        &self,
        cancellation: CancellationToken,
    ) -> impl Future<Output = Result<Vec<RateLimitSnapshot>, ProviderError>> + Send;
}

/// Keeps the last good data for one provider without sharing state with others.
#[derive(Debug, Clone)]
pub struct ProviderState {
    provider_id: String,
    latest: ProviderSnapshot,
    last_known_good: Option<Vec<RateLimitSnapshot>>,
}

impl ProviderState {
    pub fn new(provider_id: impl Into<String>) -> Self {
        let provider_id = provider_id.into();
        Self {
            latest: ProviderSnapshot {
                provider_id: provider_id.clone(),
                status: ProviderStatus::Unavailable,
                version: None,
                limits: Vec::new(),
                last_success_unix_seconds: None,
                error: None,
            },
            provider_id,
            last_known_good: None,
        }
    }

    pub fn snapshot(&self) -> &ProviderSnapshot {
        &self.latest
    }

    pub fn last_known_good(&self) -> Option<&[RateLimitSnapshot]> {
        self.last_known_good.as_deref()
    }

    pub fn record_success(
        &mut self,
        limits: Vec<RateLimitSnapshot>,
        version: Option<String>,
        unix_seconds: i64,
    ) {
        self.last_known_good = Some(limits.clone());
        self.latest = ProviderSnapshot {
            provider_id: self.provider_id.clone(),
            status: ProviderStatus::Available,
            version,
            limits,
            last_success_unix_seconds: Some(unix_seconds),
            error: None,
        };
    }

    pub fn record_failure(&mut self, status: ProviderStatus, error: ProviderError) {
        debug_assert_ne!(status, ProviderStatus::Available);
        self.latest = ProviderSnapshot {
            provider_id: self.provider_id.clone(),
            status,
            version: self.latest.version.clone(),
            limits: self.last_known_good.clone().unwrap_or_default(),
            last_success_unix_seconds: self.latest.last_success_unix_seconds,
            error: Some(error),
        };
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{ProviderError, ProviderStatus, RateLimitSnapshot, RateLimitWindow};
    use crate::provider::ProviderState;

    fn limit(used_percent: f64) -> RateLimitSnapshot {
        RateLimitSnapshot {
            limit_id: Some("codex".into()),
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent,
                window_duration_mins: Some(300),
                resets_at: None,
            }),
            secondary: None,
            plan_type: None,
            rate_limit_reached_type: None,
        }
    }

    #[test]
    fn failures_retain_only_this_providers_last_known_good_values() {
        let mut codex = ProviderState::new("codex");
        let claude = ProviderState::new("claude");
        codex.record_success(vec![limit(21.0)], Some("1.0".into()), 42);
        codex.record_failure(
            ProviderStatus::Error,
            ProviderError {
                code: "timeout".into(),
                message: "timed out".into(),
            },
        );
        assert_eq!(codex.snapshot().limits, vec![limit(21.0)]);
        assert_eq!(codex.snapshot().last_success_unix_seconds, Some(42));
        assert_eq!(claude.snapshot().status, ProviderStatus::Unavailable);
        assert!(claude.last_known_good().is_none());
    }
}

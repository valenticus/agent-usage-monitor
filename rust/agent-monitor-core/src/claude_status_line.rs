use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Serialize, de::value::MapAccessDeserializer};

use crate::models::{
    ProviderError, ProviderSnapshot, ProviderStatus, RateLimitSnapshot, RateLimitWindow,
};

/// Normalized values copied from Claude Code's documented local status-line JSON.
/// It intentionally contains no session, workspace, transcript, or conversation data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeStatusLineSnapshot {
    pub five_hour_remaining_percent: Option<f64>,
    pub five_hour_reset_at: Option<i64>,
    pub weekly_remaining_percent: Option<f64>,
    pub weekly_reset_at: Option<i64>,
    pub snapshot_at: i64,
}

impl ClaudeStatusLineSnapshot {
    /// Parses only documented rate-limit members. Unknown JSON fields are discarded by
    /// Serde's `IgnoredAny` path; this code never follows a transcript path or retains
    /// unrelated status-line fields.
    pub fn from_status_line_json(input: &str, snapshot_at: i64) -> Result<Self, ProviderError> {
        let parsed: ObjectOnly<StatusLineInput> =
            serde_json::from_str(input).map_err(|_| invalid_input())?;
        let rate_limits = parsed.0.rate_limits.ok_or_else(unavailable)?.0;
        let five = normalize_window(rate_limits.five_hour)?;
        let weekly = normalize_window(rate_limits.seven_day)?;
        let snapshot = Self {
            five_hour_remaining_percent: five.0,
            five_hour_reset_at: five.1,
            weekly_remaining_percent: weekly.0,
            weekly_reset_at: weekly.1,
            snapshot_at,
        };
        snapshot.validate(snapshot_at)?;
        Ok(snapshot)
    }

    pub fn validate(&self, now_unix_seconds: i64) -> Result<(), ProviderError> {
        if self.snapshot_at < now_unix_seconds.saturating_sub(MAX_MESSAGE_AGE_SECONDS)
            || self.snapshot_at > now_unix_seconds.saturating_add(MAX_FUTURE_SKEW_SECONDS)
            || !valid_percent(self.five_hour_remaining_percent)
            || !valid_percent(self.weekly_remaining_percent)
            || !valid_reset(self.five_hour_reset_at, self.snapshot_at)
            || !valid_reset(self.weekly_reset_at, self.snapshot_at)
        {
            return Err(invalid_input());
        }
        Ok(())
    }

    /// Converts normalized remaining percentages into the Core's used-percent model.
    /// Missing percentages remain missing; no fallback zero is introduced.
    pub fn to_provider_snapshot(&self) -> ProviderSnapshot {
        let primary = self
            .five_hour_remaining_percent
            .map(|remaining| RateLimitWindow {
                used_percent: 100.0 - remaining,
                window_duration_mins: Some(300),
                resets_at: self.five_hour_reset_at,
            });
        let secondary = self
            .weekly_remaining_percent
            .map(|remaining| RateLimitWindow {
                used_percent: 100.0 - remaining,
                window_duration_mins: Some(10_080),
                resets_at: self.weekly_reset_at,
            });
        ProviderSnapshot {
            provider_id: "claude".into(),
            status: ProviderStatus::Available,
            version: None,
            limits: vec![RateLimitSnapshot {
                limit_id: Some("claude".into()),
                limit_name: Some("Claude".into()),
                primary,
                secondary,
                plan_type: None,
                rate_limit_reached_type: None,
            }],
            last_success_unix_seconds: Some(self.snapshot_at),
            error: None,
        }
    }
}

const MAX_MESSAGE_AGE_SECONDS: i64 = 120;
const MAX_FUTURE_SKEW_SECONDS: i64 = 30;
const MAX_RESET_HORIZON_SECONDS: i64 = 31 * 86_400;

#[derive(Deserialize)]
struct StatusLineInput {
    #[serde(default)]
    rate_limits: Option<ObjectOnly<StatusLineRateLimits>>,
}

#[derive(Deserialize)]
struct StatusLineRateLimits {
    #[serde(default)]
    five_hour: Option<ObjectOnly<StatusLineWindow>>,
    #[serde(default)]
    seven_day: Option<ObjectOnly<StatusLineWindow>>,
}

#[derive(Deserialize)]
struct StatusLineWindow {
    #[serde(default)]
    used_percentage: Option<f64>,
    #[serde(default)]
    resets_at: Option<i64>,
}

fn normalize_window(
    window: Option<ObjectOnly<StatusLineWindow>>,
) -> Result<(Option<f64>, Option<i64>), ProviderError> {
    let Some(ObjectOnly(window)) = window else {
        return Ok((None, None));
    };
    if let Some(used) = window.used_percentage {
        if !used.is_finite() || !(0.0..=100.0).contains(&used) {
            return Err(invalid_input());
        }
    }
    Ok((
        window.used_percentage.map(|used| 100.0 - used),
        window.resets_at,
    ))
}

struct ObjectOnly<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ObjectOnly<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ObjectVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
            type Value = ObjectOnly<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                T::deserialize(MapAccessDeserializer::new(map)).map(ObjectOnly)
            }
        }

        deserializer.deserialize_map(ObjectVisitor(std::marker::PhantomData))
    }
}

fn valid_percent(value: Option<f64>) -> bool {
    value.is_none_or(|percent| percent.is_finite() && (0.0..=100.0).contains(&percent))
}

fn valid_reset(value: Option<i64>, snapshot_at: i64) -> bool {
    value.is_none_or(|reset| {
        reset > 0 && reset <= snapshot_at.saturating_add(MAX_RESET_HORIZON_SECONDS)
    })
}

fn invalid_input() -> ProviderError {
    ProviderError {
        code: "invalid_claude_status_line".into(),
        message: "Claude status-line rate-limit data was invalid".into(),
    }
}

fn unavailable() -> ProviderError {
    ProviderError {
        code: "claude_rate_limits_unavailable".into(),
        message: "Claude status-line input has no documented rate-limit fields".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::ClaudeStatusLineSnapshot;

    #[test]
    fn normalizes_only_documented_usage_and_reset_fields() {
        let input = r#"{"transcript_path":"not-opened","session_id":"ignored","context_window":{"used_percentage":88},"rate_limits":{"five_hour":{"used_percentage":23.5,"resets_at":1700000100},"seven_day":{"used_percentage":41.2,"resets_at":1700000200},"spend_limit":{"used_percentage":99}}}"#;
        let normalized =
            ClaudeStatusLineSnapshot::from_status_line_json(input, 1_700_000_000).unwrap();
        assert_eq!(normalized.five_hour_remaining_percent, Some(76.5));
        assert_eq!(normalized.five_hour_reset_at, Some(1_700_000_100));
        assert_eq!(normalized.weekly_remaining_percent, Some(58.8));
        assert_eq!(normalized.weekly_reset_at, Some(1_700_000_200));
        let value = serde_json::to_value(normalized).unwrap();
        let keys: Vec<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "five_hour_remaining_percent",
                "five_hour_reset_at",
                "snapshot_at",
                "weekly_remaining_percent",
                "weekly_reset_at",
            ]
        );
    }

    #[test]
    fn missing_values_stay_missing_and_preserve_independent_reset() {
        let normalized = ClaudeStatusLineSnapshot::from_status_line_json(
            r#"{"rate_limits":{"five_hour":{"resets_at":1700000100},"seven_day":{}}}"#,
            1_700_000_000,
        )
        .unwrap();
        assert_eq!(normalized.five_hour_remaining_percent, None);
        assert_eq!(normalized.five_hour_reset_at, Some(1_700_000_100));
        assert_eq!(normalized.weekly_remaining_percent, None);
        assert_eq!(normalized.weekly_reset_at, None);
        let core = normalized.to_provider_snapshot();
        assert!(core.limits[0].primary.is_none());
        assert!(core.limits[0].secondary.is_none());
    }

    #[test]
    fn rejects_invalid_ranges_bad_json_unknown_ipc_fields_and_stale_messages() {
        for raw in [
            r#"{"rate_limits":{"five_hour":{"used_percentage":101}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":-1}}}"#,
            r#"{"rate_limits":[]}"#,
            "not-json",
        ] {
            assert!(
                ClaudeStatusLineSnapshot::from_status_line_json(raw, 1_700_000_000).is_err(),
                "accepted invalid status-line input: {raw}"
            );
        }
        let mut stale = ClaudeStatusLineSnapshot::from_status_line_json(
            r#"{"rate_limits":{"five_hour":{"used_percentage":50}}}"#,
            1_700_000_000,
        )
        .unwrap();
        stale.snapshot_at = 1_699_999_000;
        assert!(stale.validate(1_700_000_000).is_err());

        let unknown: Result<ClaudeStatusLineSnapshot, _> = serde_json::from_str(
            r#"{"five_hour_remaining_percent":50,"five_hour_reset_at":null,"weekly_remaining_percent":null,"weekly_reset_at":null,"snapshot_at":1700000000,"transcript":"forbidden"}"#,
        );
        assert!(unknown.is_err());
    }
}

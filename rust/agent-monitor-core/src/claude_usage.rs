use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::{
    ClaudeStatusLineSnapshot, ProviderSnapshot, ProviderStatus, RateLimitSnapshot, RateLimitWindow,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeUsageSource {
    StatusLine,
    BackgroundCli,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeUsageWindow {
    pub used_percent: Option<f64>,
    pub resets_at: Option<i64>,
    pub observed_at: i64,
    pub source: ClaudeUsageSource,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaudeUsageSnapshot {
    pub five_hour: Option<ClaudeUsageWindow>,
    pub weekly: Option<ClaudeUsageWindow>,
}

impl ClaudeUsageSnapshot {
    pub fn merge(&mut self, incoming: Self) {
        fn newer(old: &Option<ClaudeUsageWindow>, new: &Option<ClaudeUsageWindow>) -> bool {
            match (old, new) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(old), Some(new)) => {
                    new.observed_at > old.observed_at
                        || (new.observed_at == old.observed_at
                            && new.source == ClaudeUsageSource::StatusLine
                            && old.source != ClaudeUsageSource::StatusLine)
                }
            }
        }
        if newer(&self.five_hour, &incoming.five_hour) {
            self.five_hour = incoming.five_hour;
        }
        if newer(&self.weekly, &incoming.weekly) {
            self.weekly = incoming.weekly;
        }
    }

    pub fn to_provider_snapshot(&self) -> Option<ProviderSnapshot> {
        if self
            .five_hour
            .as_ref()
            .is_none_or(|window| window.used_percent.is_none())
            && self
                .weekly
                .as_ref()
                .is_none_or(|window| window.used_percent.is_none())
        {
            return None;
        }
        let to_rate_window = |window: &Option<ClaudeUsageWindow>, duration| {
            window.as_ref().and_then(|window| {
                Some(RateLimitWindow {
                    used_percent: window.used_percent?,
                    window_duration_mins: Some(duration),
                    resets_at: window.resets_at,
                })
            })
        };
        let oldest = [self.five_hour.as_ref(), self.weekly.as_ref()]
            .into_iter()
            .flatten()
            .map(|window| window.observed_at)
            .min();
        Some(ProviderSnapshot {
            provider_id: "claude".into(),
            status: ProviderStatus::Available,
            version: None,
            limits: vec![RateLimitSnapshot {
                limit_id: Some("claude".into()),
                limit_name: Some("Claude".into()),
                primary: to_rate_window(&self.five_hour, 300),
                secondary: to_rate_window(&self.weekly, 10_080),
                plan_type: None,
                rate_limit_reached_type: None,
            }],
            last_success_unix_seconds: oldest,
            error: None,
        })
    }
}

impl ClaudeStatusLineSnapshot {
    pub fn to_usage_snapshot(&self) -> ClaudeUsageSnapshot {
        ClaudeUsageSnapshot {
            five_hour: (self.five_hour_remaining_percent.is_some()
                || self.five_hour_reset_at.is_some())
            .then(|| ClaudeUsageWindow {
                used_percent: self
                    .five_hour_remaining_percent
                    .map(|remaining| 100.0 - remaining),
                resets_at: self.five_hour_reset_at,
                observed_at: self.snapshot_at,
                source: ClaudeUsageSource::StatusLine,
            }),
            weekly: (self.weekly_remaining_percent.is_some() || self.weekly_reset_at.is_some())
                .then(|| ClaudeUsageWindow {
                    used_percent: self
                        .weekly_remaining_percent
                        .map(|remaining| 100.0 - remaining),
                    resets_at: self.weekly_reset_at,
                    observed_at: self.snapshot_at,
                    source: ClaudeUsageSource::StatusLine,
                }),
        }
    }
}

/// Parses Claude Code's human-readable `/usage` output. This format is not a
/// documented stable interface; unsupported lines are simply left unavailable.
pub fn parse_claude_cli_usage(input: &str, observed_at: i64) -> ClaudeUsageSnapshot {
    let mut result = ClaudeUsageSnapshot::default();
    for line in input.lines() {
        let line = line.trim();
        let target = if line.starts_with("Current session:") {
            Some((&mut result.five_hour, "Current session:"))
        } else if line.starts_with("Current week (all models):") {
            Some((&mut result.weekly, "Current week (all models):"))
        } else {
            None
        };
        let Some((slot, prefix)) = target else {
            continue;
        };
        let rest = line.trim_start_matches(prefix).trim();
        let Some((percent, reset_text)) = rest.split_once("% used") else {
            continue;
        };
        let Ok(used_percent) = percent.trim().parse::<f64>() else {
            continue;
        };
        if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
            continue;
        }
        let reset_text = reset_text.trim().trim_start_matches('·').trim();
        let resets_at = parse_reset(reset_text, observed_at);
        *slot = Some(ClaudeUsageWindow {
            used_percent: Some(used_percent),
            resets_at,
            observed_at,
            source: ClaudeUsageSource::BackgroundCli,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn merges_independent_windows_by_timestamp_without_erasing_missing_values() {
        let mut current = ClaudeUsageSnapshot {
            five_hour: Some(ClaudeUsageWindow {
                used_percent: Some(20.0),
                resets_at: Some(200),
                observed_at: 100,
                source: ClaudeUsageSource::BackgroundCli,
            }),
            weekly: Some(ClaudeUsageWindow {
                used_percent: Some(50.0),
                resets_at: None,
                observed_at: 90,
                source: ClaudeUsageSource::StatusLine,
            }),
        };
        current.merge(ClaudeUsageSnapshot {
            five_hour: Some(ClaudeUsageWindow {
                used_percent: Some(30.0),
                resets_at: None,
                observed_at: 99,
                source: ClaudeUsageSource::StatusLine,
            }),
            weekly: None,
        });
        assert_eq!(current.five_hour.unwrap().used_percent, Some(20.0));
        assert_eq!(current.weekly.unwrap().used_percent, Some(50.0));
    }

    #[test]
    fn accepts_newer_window_and_prefers_status_line_on_equal_timestamp() {
        let mut snapshot = ClaudeUsageSnapshot::default();
        snapshot.merge(ClaudeUsageSnapshot {
            five_hour: Some(ClaudeUsageWindow {
                used_percent: Some(10.0),
                resets_at: None,
                observed_at: 10,
                source: ClaudeUsageSource::BackgroundCli,
            }),
            weekly: None,
        });
        snapshot.merge(ClaudeUsageSnapshot {
            five_hour: Some(ClaudeUsageWindow {
                used_percent: Some(20.0),
                resets_at: None,
                observed_at: 10,
                source: ClaudeUsageSource::StatusLine,
            }),
            weekly: None,
        });
        assert_eq!(snapshot.five_hour.unwrap().used_percent, Some(20.0));
    }

    #[test]
    fn parses_only_session_and_all_model_weekly_lines_with_partial_values() {
        let input = "Current session: 8% used · resets Jul 13, 3:29pm (Europe/Warsaw)\nCurrent week (all models): 67% used\nCurrent week (Sonnet): 99% used";
        let result = parse_claude_cli_usage(input, 1_783_000_000);
        assert_eq!(result.five_hour.unwrap().used_percent, Some(8.0));
        assert_eq!(result.weekly.unwrap().used_percent, Some(67.0));
        assert!(
            parse_claude_cli_usage("unrecognized output", 100)
                .weekly
                .is_none()
        );
    }

    #[test]
    fn parses_timezone_aware_reset_and_leaves_ambiguous_or_unknown_zone_unavailable() {
        let zone: Tz = "Europe/Warsaw".parse().unwrap();
        let observed = zone
            .with_ymd_and_hms(2026, 7, 10, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        let parsed = parse_claude_cli_usage(
            "Current session: 8% used · resets Jul 13, 3:29pm (Europe/Warsaw)",
            observed,
        );
        let reset = parsed.five_hour.unwrap().resets_at.unwrap();
        assert_eq!(
            Utc.timestamp_opt(reset, 0)
                .single()
                .unwrap()
                .with_timezone(&zone)
                .hour(),
            15
        );
        assert_eq!(
            Utc.timestamp_opt(reset, 0)
                .single()
                .unwrap()
                .with_timezone(&zone)
                .day(),
            13
        );

        let ambiguous = parse_claude_cli_usage(
            "Current session: 8% used · resets Oct 25, 2:30am (Europe/Warsaw)",
            zone.with_ymd_and_hms(2026, 10, 24, 10, 0, 0)
                .single()
                .unwrap()
                .timestamp(),
        );
        assert_eq!(ambiguous.five_hour.unwrap().resets_at, None);
        let unknown = parse_claude_cli_usage(
            "Current session: 8% used · resets Jul 13, 3:29pm (Unknown/Zone)",
            observed,
        );
        assert_eq!(unknown.five_hour.unwrap().resets_at, None);
    }
}

fn parse_reset(text: &str, observed_at: i64) -> Option<i64> {
    let (date_and_time, zone) = text.rsplit_once('(')?;
    let zone = zone.trim_end_matches(')').trim().parse::<Tz>().ok()?;
    let date_and_time = date_and_time.trim().strip_prefix("resets")?.trim();
    let base = Utc
        .timestamp_opt(observed_at, 0)
        .single()?
        .with_timezone(&zone);
    let mut candidate = None;
    let (month_day, time_text) = date_and_time.split_once(", ")?;
    for year in [base.year(), base.year().saturating_add(1)] {
        let date_text = format!("{month_day} {year}");
        let date = NaiveDate::parse_from_str(&date_text, "%b %d %Y")
            .or_else(|_| NaiveDate::parse_from_str(&date_text, "%b %e %Y"))
            .ok()?;
        let time = NaiveTime::parse_from_str(time_text, "%I:%M%P")
            .or_else(|_| NaiveTime::parse_from_str(time_text, "%I:%M %P"))
            .ok()?;
        let naive = date.and_time(time);
        if let chrono::LocalResult::Single(value) = zone.from_local_datetime(&naive) {
            let timestamp = value.timestamp();
            if timestamp >= observed_at.saturating_sub(86_400)
                && timestamp <= observed_at.saturating_add(8 * 86_400)
            {
                candidate = Some(timestamp);
                break;
            }
        }
    }
    candidate
}

use chrono::Datelike;

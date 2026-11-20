use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::models::{ProviderSnapshot, ProviderStatus};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertSettings {
    pub enabled: bool,
    pub warning_threshold: u8,
    pub critical_threshold: u8,
    pub exhausted_threshold: u8,
    #[serde(default = "default_true")]
    pub exhausted_alerts_enabled: bool,
    pub include_weekly: bool,
    pub popup_duration_seconds: u16,
    pub stale_after_seconds: i64,
}

impl Default for AlertSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            warning_threshold: 25,
            critical_threshold: 10,
            exhausted_threshold: 0,
            exhausted_alerts_enabled: true,
            include_weekly: true,
            popup_duration_seconds: 8,
            stale_after_seconds: 900,
        }
    }
}

fn default_true() -> bool {
    true
}

impl AlertSettings {
    fn valid(&self) -> bool {
        self.exhausted_threshold <= self.critical_threshold
            && self.critical_threshold <= self.warning_threshold
            && self.warning_threshold <= 100
            && self.popup_duration_seconds > 0
            && self.stale_after_seconds >= 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertWindow {
    FiveHour,
    Weekly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Warning,
    Critical,
    Exhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AlertKey {
    pub provider_id: String,
    pub window: AlertWindow,
}

impl Serialize for AlertKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}:{}", self.window.as_str(), self.provider_id))
    }
}

impl<'de> Deserialize<'de> for AlertKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        let (window, provider_id) = encoded
            .split_once(':')
            .ok_or_else(|| serde::de::Error::custom("invalid alert key"))?;
        let window = match window {
            "five_hour" => AlertWindow::FiveHour,
            "weekly" => AlertWindow::Weekly,
            _ => return Err(serde::de::Error::custom("invalid alert window")),
        };
        Ok(Self {
            provider_id: provider_id.into(),
            window,
        })
    }
}

impl AlertWindow {
    fn as_str(self) -> &'static str {
        match self {
            Self::FiveHour => "five_hour",
            Self::Weekly => "weekly",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertEvent {
    pub key: AlertKey,
    pub severity: AlertSeverity,
    pub remaining_percent: u8,
    pub reset_at: Option<i64>,
    pub popup_duration_seconds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AlertWindowState {
    pub reset_at: Option<i64>,
    pub highest_severity: Option<AlertSeverity>,
}

/// Serializable state; persist by atomically replacing the state file after a refresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AlertEngineState {
    pub windows: BTreeMap<AlertKey, AlertWindowState>,
}

#[derive(Debug, thiserror::Error)]
pub enum AlertStateStoreError {
    #[error("failed to access alert state")]
    Io(#[from] std::io::Error),
    #[error("alert state data is invalid")]
    Invalid(#[from] serde_json::Error),
}

/// Loads and atomically saves the serialized alert state at an app-owned path.
pub struct AlertStateStore;

impl AlertStateStore {
    pub fn load(path: &Path) -> Result<AlertEngineState, AlertStateStoreError> {
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AlertEngineState::default());
            }
            Err(error) => return Err(error.into()),
        };
        let mut serialized = String::new();
        file.read_to_string(&mut serialized)?;
        Ok(serde_json::from_str(&serialized)?)
    }

    pub fn save(path: &Path, state: &AlertEngineState) -> Result<(), AlertStateStoreError> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut file, state)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(path)
            .map_err(|error| AlertStateStoreError::Io(error.error))?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AlertEngine {
    settings: AlertSettings,
    state: AlertEngineState,
}

impl AlertEngine {
    pub fn new(settings: AlertSettings, state: AlertEngineState) -> Result<Self, &'static str> {
        if !settings.valid() {
            return Err("invalid alert thresholds or duration");
        }
        Ok(Self { settings, state })
    }

    pub fn state(&self) -> &AlertEngineState {
        &self.state
    }

    pub fn set_settings(&mut self, settings: AlertSettings) -> Result<(), &'static str> {
        if !settings.valid() {
            return Err("invalid alert thresholds or duration");
        }
        self.settings = settings;
        Ok(())
    }

    /// Returns every new severity transition across providers/windows in one refresh.
    pub fn refresh(
        &mut self,
        providers: &[ProviderSnapshot],
        now_unix_seconds: i64,
    ) -> Vec<AlertEvent> {
        if !self.settings.enabled {
            return Vec::new();
        }

        let mut events = Vec::new();
        for provider in providers {
            if provider.status != ProviderStatus::Available
                || provider.last_success_unix_seconds.is_none_or(|last| {
                    now_unix_seconds.saturating_sub(last) > self.settings.stale_after_seconds
                        || last > now_unix_seconds
                })
            {
                continue;
            }
            for limit in &provider.limits {
                for (window_kind, window) in [
                    (AlertWindow::FiveHour, limit.primary.as_ref()),
                    (AlertWindow::Weekly, limit.secondary.as_ref()),
                ] {
                    if window_kind == AlertWindow::Weekly && !self.settings.include_weekly {
                        continue;
                    }
                    let Some(window) = window else { continue };
                    if !window.used_percent.is_finite()
                        || !(0.0..=100.0).contains(&window.used_percent)
                    {
                        continue;
                    }
                    let remaining_exact = (100.0 - window.used_percent).clamp(0.0, 100.0);
                    let remaining = remaining_exact.round() as u8;
                    let Some(severity) = self.severity(remaining_exact) else {
                        continue;
                    };
                    let key = AlertKey {
                        provider_id: provider.provider_id.clone(),
                        window: window_kind,
                    };
                    let cycle = self.state.windows.entry(key.clone()).or_default();
                    if let Some(reset) = window.resets_at {
                        if cycle.reset_at.is_some_and(|previous| reset > previous) {
                            cycle.highest_severity = None;
                        }
                        if cycle.reset_at.is_none_or(|previous| reset > previous) {
                            cycle.reset_at = Some(reset);
                        }
                    }
                    if cycle.highest_severity.is_some_and(|sent| severity <= sent) {
                        continue;
                    }
                    cycle.highest_severity = Some(severity);
                    events.push(AlertEvent {
                        key,
                        severity,
                        remaining_percent: remaining,
                        reset_at: window.resets_at,
                        popup_duration_seconds: self.settings.popup_duration_seconds,
                    });
                }
            }
        }
        events
    }

    fn severity(&self, remaining: f64) -> Option<AlertSeverity> {
        if self.settings.exhausted_alerts_enabled
            && remaining <= f64::from(self.settings.exhausted_threshold)
        {
            Some(AlertSeverity::Exhausted)
        } else if remaining <= f64::from(self.settings.critical_threshold) {
            Some(AlertSeverity::Critical)
        } else if remaining <= f64::from(self.settings.warning_threshold) {
            Some(AlertSeverity::Warning)
        } else {
            None
        }
    }
}

/// Tray status is intentionally based on provider 5-hour windows only.
pub fn minimum_five_hour_remaining(
    providers: &[ProviderSnapshot],
    now: i64,
    stale_after: i64,
) -> Option<u8> {
    providers
        .iter()
        .filter(|provider| provider.status == ProviderStatus::Available)
        .filter(|provider| {
            provider
                .last_success_unix_seconds
                .is_some_and(|time| time <= now && now.saturating_sub(time) <= stale_after)
        })
        .flat_map(|provider| {
            provider
                .limits
                .iter()
                .filter_map(|limit| limit.primary.as_ref())
        })
        .filter(|window| {
            window.used_percent.is_finite() && (0.0..=100.0).contains(&window.used_percent)
        })
        .map(|window| (100.0 - window.used_percent).clamp(0.0, 100.0).round() as u8)
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{RateLimitSnapshot, RateLimitWindow};

    fn provider(
        id: &str,
        five: Option<(f64, Option<i64>)>,
        weekly: Option<(f64, Option<i64>)>,
        time: Option<i64>,
    ) -> ProviderSnapshot {
        let window = |value: Option<(f64, Option<i64>)>| {
            value.map(|(used_percent, resets_at)| RateLimitWindow {
                used_percent,
                window_duration_mins: None,
                resets_at,
            })
        };
        ProviderSnapshot {
            provider_id: id.into(),
            status: ProviderStatus::Available,
            version: None,
            limits: vec![RateLimitSnapshot {
                limit_id: None,
                limit_name: None,
                primary: window(five),
                secondary: window(weekly),
                plan_type: None,
                rate_limit_reached_type: None,
            }],
            last_success_unix_seconds: time,
            error: None,
        }
    }

    fn engine() -> AlertEngine {
        AlertEngine::new(AlertSettings::default(), AlertEngineState::default()).unwrap()
    }

    #[test]
    fn alerts_multiple_providers_and_windows_in_one_refresh() {
        let mut engine = engine();
        let events = engine.refresh(
            &[
                provider(
                    "codex",
                    Some((80.0, Some(500))),
                    Some((95.0, Some(900))),
                    Some(100),
                ),
                provider(
                    "claude",
                    Some((91.0, Some(500))),
                    Some((100.0, Some(900))),
                    Some(100),
                ),
            ],
            101,
        );
        assert_eq!(events.len(), 4);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.severity == AlertSeverity::Warning)
                .count(),
            1,
            "events: {events:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.severity == AlertSeverity::Critical)
                .count(),
            2
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.severity == AlertSeverity::Exhausted)
                .count(),
            1
        );
    }

    #[test]
    fn escalates_once_deduplicates_and_resets_only_on_new_reset_cycle() {
        let mut engine = engine();
        let warning = provider("codex", Some((80.0, Some(200))), None, Some(100));
        assert_eq!(
            engine.refresh(std::slice::from_ref(&warning), 100)[0].severity,
            AlertSeverity::Warning
        );
        assert!(engine.refresh(&[warning], 101).is_empty());
        assert_eq!(
            engine.refresh(
                &[provider("codex", Some((91.0, Some(200))), None, Some(101))],
                101
            )[0]
            .severity,
            AlertSeverity::Critical
        );
        assert_eq!(
            engine.refresh(
                &[provider("codex", Some((100.0, Some(150))), None, Some(101))],
                101
            )[0]
            .severity,
            AlertSeverity::Exhausted
        );
        assert!(
            engine
                .refresh(
                    &[provider("codex", Some((100.0, Some(150))), None, Some(101))],
                    101
                )
                .is_empty()
        );
        assert_eq!(
            engine.refresh(
                &[provider("codex", Some((100.0, Some(300))), None, Some(102))],
                102
            )[0]
            .severity,
            AlertSeverity::Exhausted
        );
    }

    #[test]
    fn ignores_missing_stale_invalid_and_future_data_without_zero_alerts() {
        let mut engine = engine();
        assert!(
            engine
                .refresh(&[provider("codex", None, None, Some(100))], 100)
                .is_empty()
        );
        assert!(
            engine
                .refresh(
                    &[provider("codex", Some((100.0, Some(200))), None, Some(0))],
                    1000
                )
                .is_empty()
        );
        assert!(
            engine
                .refresh(
                    &[provider("codex", Some((100.0, Some(200))), None, Some(101))],
                    100
                )
                .is_empty()
        );
        assert!(
            engine
                .refresh(
                    &[provider("codex", Some((101.0, Some(200))), None, Some(100))],
                    100
                )
                .is_empty()
        );
        assert!(
            engine
                .refresh(
                    &[ProviderSnapshot {
                        last_success_unix_seconds: None,
                        ..provider("codex", Some((100.0, Some(200))), None, Some(100))
                    }],
                    100
                )
                .is_empty()
        );
    }

    #[test]
    fn persists_deduplication_state_and_limits_tray_value_to_five_hour_windows() {
        let mut engine = engine();
        let data = [provider(
            "codex",
            Some((80.0, Some(200))),
            Some((100.0, Some(300))),
            Some(100),
        )];
        let first = engine.refresh(&data, 100);
        let serialized = serde_json::to_string(engine.state()).unwrap();
        let restored: AlertEngineState = serde_json::from_str(&serialized).unwrap();
        let mut restarted = AlertEngine::new(AlertSettings::default(), restored).unwrap();
        assert!(restarted.refresh(&data, 101).is_empty());
        assert_eq!(minimum_five_hour_remaining(&data, 100, 900), Some(20));
        assert_eq!(first[0].popup_duration_seconds, 8);
        assert_eq!(
            minimum_five_hour_remaining(
                &[provider(
                    "claude",
                    None,
                    Some((100.0, Some(300))),
                    Some(100)
                )],
                100,
                900
            ),
            None
        );
    }

    #[test]
    fn settings_validate_threshold_order() {
        let settings = AlertSettings {
            critical_threshold: 30,
            ..AlertSettings::default()
        };
        assert!(AlertEngine::new(settings, AlertEngineState::default()).is_err());
    }

    #[test]
    fn defaults_match_approved_values_and_weekly_can_be_disabled() {
        let defaults = AlertSettings::default();
        assert!(defaults.enabled);
        assert_eq!(defaults.warning_threshold, 25);
        assert_eq!(defaults.critical_threshold, 10);
        assert_eq!(defaults.exhausted_threshold, 0);
        assert!(defaults.exhausted_alerts_enabled);
        assert!(defaults.include_weekly);
        assert_eq!(defaults.popup_duration_seconds, 8);

        let settings = AlertSettings {
            include_weekly: false,
            ..defaults
        };
        let mut engine = AlertEngine::new(settings, AlertEngineState::default()).unwrap();
        let events = engine.refresh(
            &[provider(
                "codex",
                Some((80.0, Some(200))),
                Some((100.0, Some(300))),
                Some(100),
            )],
            100,
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].key.window, AlertWindow::FiveHour);
    }

    #[test]
    fn exhausted_setting_suppresses_only_the_exhausted_severity_and_settings_update_live() {
        let mut engine = AlertEngine::new(
            AlertSettings {
                exhausted_alerts_enabled: false,
                ..AlertSettings::default()
            },
            AlertEngineState::default(),
        )
        .unwrap();
        let events = engine.refresh(
            &[provider("codex", Some((100.0, Some(200))), None, Some(100))],
            100,
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].severity, AlertSeverity::Critical);
        engine
            .set_settings(AlertSettings {
                warning_threshold: 50,
                ..AlertSettings::default()
            })
            .unwrap();
        let events = engine.refresh(
            &[provider("claude", Some((60.0, Some(300))), None, Some(100))],
            100,
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].severity, AlertSeverity::Warning);
    }

    #[test]
    fn fractional_remaining_is_not_misclassified_as_exhausted() {
        let mut engine = engine();
        let events = engine.refresh(
            &[provider("codex", Some((99.6, Some(200))), None, Some(100))],
            100,
        );
        assert_eq!(events[0].severity, AlertSeverity::Critical);
        assert_eq!(events[0].remaining_percent, 0);
    }

    #[test]
    fn state_store_round_trips_and_missing_file_starts_empty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        assert_eq!(
            AlertStateStore::load(&path).unwrap(),
            AlertEngineState::default()
        );
        let mut engine = engine();
        engine.refresh(
            &[provider("codex", Some((80.0, Some(200))), None, Some(100))],
            100,
        );
        AlertStateStore::save(&path, engine.state()).unwrap();
        assert_eq!(AlertStateStore::load(&path).unwrap(), *engine.state());
    }
}

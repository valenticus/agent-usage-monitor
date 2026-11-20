use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use agent_usage_monitor_core::AlertEngineState;
use serde::{Deserialize, Serialize};

const SETTINGS_MIGRATION_LIMIT: u64 = 64 * 1024;
const ALERT_STATE_MIGRATION_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Graphite,
    #[serde(alias = "dark")]
    Navy,
    Light,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    System,
    Ukrainian,
    #[default]
    English,
    Polish,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub schema_version: u32,
    pub theme_mode: ThemeMode,
    pub language: Language,
    pub codex_enabled: bool,
    pub claude_enabled: bool,
    pub claude_cli_refresh_enabled: bool,
    pub alerts_enabled: bool,
    pub warning_threshold: u8,
    pub critical_threshold: u8,
    pub exhausted_alerts_enabled: bool,
    pub include_weekly: bool,
    pub popup_duration_seconds: u16,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            theme_mode: ThemeMode::System,
            language: Language::System,
            codex_enabled: true,
            claude_enabled: true,
            claude_cli_refresh_enabled: false,
            alerts_enabled: true,
            warning_threshold: 25,
            critical_threshold: 10,
            exhausted_alerts_enabled: true,
            include_weekly: true,
            popup_duration_seconds: 8,
        }
    }
}

impl AppSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported settings version");
        }
        if self.critical_threshold > self.warning_threshold || self.warning_threshold > 100 {
            return Err("thresholds must satisfy 0 <= critical <= warning <= 100");
        }
        if !(1..=120).contains(&self.popup_duration_seconds) {
            return Err("popup duration must be between 1 and 120 seconds");
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings file access failed")]
    Io(#[from] std::io::Error),
    #[error("settings data is invalid")]
    Json(#[from] serde_json::Error),
    #[error("settings values are invalid")]
    Invalid,
}

pub fn load(path: &Path) -> Result<AppSettings, SettingsError> {
    let data = match read_bounded(path, SETTINGS_MIGRATION_LIMIT) {
        Ok(data) => data,
        Err(SettingsError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppSettings::default());
        }
        Err(error) => return Err(error),
    };
    let settings: AppSettings = serde_json::from_slice(&data)?;
    settings.validate().map_err(|_| SettingsError::Invalid)?;
    Ok(settings)
}

pub fn save(path: &Path, settings: &AppSettings) -> Result<(), SettingsError> {
    settings.validate().map_err(|_| SettingsError::Invalid)?;
    let parent = path.parent().ok_or(SettingsError::Invalid)?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temporary, settings)?;
    use std::io::Write;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| SettingsError::Io(error.error))?;
    Ok(())
}

pub fn settings_path() -> PathBuf {
    settings_directory().join("settings.json")
}

pub fn settings_directory() -> PathBuf {
    data_root().join("Agent Usage Monitor")
}

pub fn legacy_settings_directory() -> PathBuf {
    data_root().join("Agent Monitor")
}

fn data_root() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    Copied,
    AlreadyPresent,
    Missing,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationReport {
    pub settings: MigrationOutcome,
    pub alert_state: MigrationOutcome,
}

/// Copies only recognized, validated app-owned JSON files. Existing target files
/// always win; source files and directories are never removed.
pub fn migrate_legacy_data(
    old_directory: &Path,
    new_directory: &Path,
) -> Result<MigrationReport, SettingsError> {
    let settings = migrate_file::<AppSettings>(
        &old_directory.join("settings.json"),
        &new_directory.join("settings.json"),
        SETTINGS_MIGRATION_LIMIT,
        |settings| settings.validate().is_ok(),
    )?;
    let alert_state = migrate_file::<AlertEngineState>(
        &old_directory.join("alert-state.json"),
        &new_directory.join("alert-state.json"),
        ALERT_STATE_MIGRATION_LIMIT,
        |_| true,
    )?;
    Ok(MigrationReport {
        settings,
        alert_state,
    })
}

fn migrate_file<T: for<'de> Deserialize<'de>>(
    source: &Path,
    destination: &Path,
    maximum_bytes: u64,
    validate: impl FnOnce(&T) -> bool,
) -> Result<MigrationOutcome, SettingsError> {
    if destination.exists() {
        return Ok(MigrationOutcome::AlreadyPresent);
    }
    let data = match read_bounded(source, maximum_bytes) {
        Ok(data) => data,
        Err(SettingsError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MigrationOutcome::Missing);
        }
        Err(SettingsError::Io(error)) if error.kind() == std::io::ErrorKind::InvalidData => {
            return Ok(MigrationOutcome::Invalid);
        }
        Err(error) => return Err(error),
    };
    let parsed: T = match serde_json::from_slice(&data) {
        Ok(value) => value,
        Err(_) => return Ok(MigrationOutcome::Invalid),
    };
    if !validate(&parsed) {
        return Ok(MigrationOutcome::Invalid);
    }
    let parent = destination.parent().ok_or(SettingsError::Invalid)?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&data)?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(destination) {
        Ok(_) => Ok(MigrationOutcome::Copied),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(MigrationOutcome::AlreadyPresent)
        }
        Err(error) => Err(error.error.into()),
    }
}

fn read_bounded(path: &Path, maximum_bytes: u64) -> Result<Vec<u8>, SettingsError> {
    let file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file exceeds size limit",
        )
        .into());
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(maximum_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file exceeds size limit",
        )
        .into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_usage_monitor_core::{
        AlertKey, AlertSeverity, AlertStateStore, AlertWindow, AlertWindowState,
    };

    #[test]
    fn defaults_match_approved_provider_and_alert_settings() {
        let settings = AppSettings::default();
        assert!(settings.codex_enabled && settings.claude_enabled && settings.alerts_enabled);
        assert!(!settings.claude_cli_refresh_enabled);
        assert_eq!(
            (settings.warning_threshold, settings.critical_threshold),
            (25, 10)
        );
        assert!(settings.exhausted_alerts_enabled && settings.include_weekly);
        assert_eq!(settings.popup_duration_seconds, 8);
        assert_eq!(settings.theme_mode, ThemeMode::System);
    }

    #[test]
    fn legacy_dark_theme_setting_migrates_to_navy() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"schema_version":1,"theme_mode":"dark"}"#).unwrap();
        assert_eq!(settings.theme_mode, ThemeMode::Navy);
    }

    #[test]
    fn settings_round_trip_atomically_and_older_missing_fields_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = AppSettings {
            codex_enabled: false,
            claude_cli_refresh_enabled: true,
            language: Language::Polish,
            ..Default::default()
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        std::fs::write(&path, r#"{"schema_version":1,"codex_enabled":false}"#).unwrap();
        let loaded = load(&path).unwrap();
        assert!(!loaded.codex_enabled);
        assert!(loaded.claude_enabled);
        assert!(!loaded.claude_cli_refresh_enabled);
        assert_eq!(loaded.warning_threshold, 25);
    }

    #[test]
    fn invalid_or_malformed_settings_are_rejected_without_silent_clamping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let invalid = AppSettings {
            critical_threshold: 30,
            warning_threshold: 25,
            ..Default::default()
        };
        assert!(save(&path, &invalid).is_err());
        std::fs::write(&path, b"{").unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn migration_copies_only_valid_allowlisted_files_and_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("Agent Monitor");
        let new = directory.path().join("Agent Usage Monitor");
        std::fs::create_dir_all(&old).unwrap();
        let settings = AppSettings {
            theme_mode: ThemeMode::Light,
            language: Language::Ukrainian,
            codex_enabled: false,
            ..Default::default()
        };
        let settings_bytes = serde_json::to_vec(&settings).unwrap();
        std::fs::write(old.join("settings.json"), &settings_bytes).unwrap();
        let mut alert_state = AlertEngineState::default();
        alert_state.windows.insert(
            AlertKey {
                provider_id: "claude".into(),
                window: AlertWindow::Weekly,
            },
            AlertWindowState {
                reset_at: Some(1_800_000_000),
                highest_severity: Some(AlertSeverity::Critical),
            },
        );
        AlertStateStore::save(&old.join("alert-state.json"), &alert_state).unwrap();
        std::fs::write(old.join("credentials.json"), "must not be copied").unwrap();

        let first = migrate_legacy_data(&old, &new).unwrap();
        assert_eq!(first.settings, MigrationOutcome::Copied);
        assert_eq!(first.alert_state, MigrationOutcome::Copied);
        assert_eq!(
            std::fs::read(new.join("settings.json")).unwrap(),
            settings_bytes
        );
        assert_eq!(
            AlertStateStore::load(&new.join("alert-state.json")).unwrap(),
            alert_state
        );
        assert!(!new.join("credentials.json").exists());
        assert!(old.join("settings.json").exists());

        let repeated = migrate_legacy_data(&old, &new).unwrap();
        assert_eq!(repeated.settings, MigrationOutcome::AlreadyPresent);
        assert_eq!(repeated.alert_state, MigrationOutcome::AlreadyPresent);
        assert_eq!(load(&new.join("settings.json")).unwrap(), settings);
    }

    #[test]
    fn migration_preserves_existing_target_and_skips_invalid_or_oversized_sources() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old");
        let new = directory.path().join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("settings.json"), b"{").unwrap();
        std::fs::write(new.join("settings.json"), b"target-owned").unwrap();
        std::fs::write(old.join("alert-state.json"), vec![b' '; 1024 * 1024 + 1]).unwrap();

        let report = migrate_legacy_data(&old, &new).unwrap();
        assert_eq!(report.settings, MigrationOutcome::AlreadyPresent);
        assert_eq!(report.alert_state, MigrationOutcome::Invalid);
        assert_eq!(
            std::fs::read(new.join("settings.json")).unwrap(),
            b"target-owned"
        );
        assert_eq!(std::fs::read(old.join("settings.json")).unwrap(), b"{");

        let invalid_old = directory.path().join("invalid-old");
        let invalid_new = directory.path().join("invalid-new");
        std::fs::create_dir_all(&invalid_old).unwrap();
        std::fs::write(invalid_old.join("settings.json"), b"{").unwrap();
        let invalid = migrate_legacy_data(&invalid_old, &invalid_new).unwrap();
        assert_eq!(invalid.settings, MigrationOutcome::Invalid);
        assert!(!invalid_new.join("settings.json").exists());
    }
}

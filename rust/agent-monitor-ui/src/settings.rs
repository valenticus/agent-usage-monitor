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
    #[serde(alias = "system")]
    Graphite,
    #[serde(alias = "dark", alias = "navy")]
    BluishDark,
    Light,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    #[serde(alias = "system")]
    Ukrainian,
    English,
    Polish,
    Russian,
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
    /// MSIX / Store build only: listen for the Claude Code status-line bridge.
    /// Packaged apps cannot receive `--enable-claude-bridge`, so the opt-in is
    /// this explicit setting instead. Off by default; ignored when unpackaged.
    pub claude_bridge_listener_enabled: bool,
    /// Opt-in for public GitHub release checks; off by default for privacy.
    pub automatic_update_checks_enabled: bool,
    /// Unix time of the last successful release metadata check.
    pub last_update_check_success_unix: Option<i64>,
    /// Last automatic attempt; failures also wait a day before retrying.
    pub last_automatic_update_check_attempt_unix: Option<i64>,
    pub launch_at_startup: bool,
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
            theme_mode: ThemeMode::Graphite,
            language: Language::Ukrainian,
            codex_enabled: true,
            claude_enabled: true,
            claude_cli_refresh_enabled: false,
            claude_bridge_listener_enabled: false,
            automatic_update_checks_enabled: false,
            last_update_check_success_unix: None,
            last_automatic_update_check_attempt_unix: None,
            launch_at_startup: false,
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
    #[error("persistent settings storage is unavailable")]
    StorageUnavailable,
}

/// Loads settings. An empty path means "no persistent storage" (see
/// [`settings_directory`]): the defaults are used and nothing is touched.
pub fn load(path: &Path) -> Result<AppSettings, SettingsError> {
    if path.as_os_str().is_empty() {
        return Ok(AppSettings::default());
    }
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
    if path.as_os_str().is_empty() {
        // No persistent storage: never report a save that did not happen.
        return Err(SettingsError::StorageUnavailable);
    }
    if !write_allowed_for(
        crate::packaged::state(),
        path,
        &unpackaged_settings_directory(),
    ) {
        eprintln!("SETTINGS: refused to write the EXE build's settings from this process");
        return Err(SettingsError::Invalid);
    }
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

/// Data folder of the EXE builds (Inno, portable) under `%APPDATA%`.
const UNPACKAGED_FOLDER: &str = "Agent Usage Monitor";
const SETTINGS_FILE: &str = "settings.json";
const PROVIDER_ICONS_FOLDER: &str = "provider-icons";

/// `settings.json` in [`settings_directory`], or `None` without persistent storage.
pub fn settings_path() -> Option<PathBuf> {
    settings_directory().map(|directory| directory.join(SETTINGS_FILE))
}

/// AUM's data folder for this process (settings, alert state, user provider
/// icons, notification icon):
///
/// * EXE builds: `%APPDATA%\Agent Usage Monitor`, unchanged.
/// * Store build: the package's own `LocalState` folder
///   (`ApplicationData.Current.LocalFolder`). It is private to the package,
///   is not subject to AppData virtualization and is removed on uninstall, so
///   the Store build can never overwrite the EXE build's settings.
/// * `None` (no persistent storage): the Store data folder cannot be
///   resolved, or the package identity is unknown. AUM then runs with default
///   settings in memory, refuses every save (`StorageUnavailable`), writes no
///   alert state and shows the problem in Settings. There is no fallback to
///   `%TEMP%` or to the EXE folder.
pub fn settings_directory() -> Option<PathBuf> {
    static DIRECTORY: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIRECTORY
        .get_or_init(|| {
            let directory = resolve_data_directory(
                crate::packaged::state(),
                &unpackaged_settings_directory(),
                package_local_folder,
            );
            if directory.is_none() {
                eprintln!(
                    "SETTINGS: no persistent storage; settings stay in memory and are not saved"
                );
            }
            directory
        })
        .clone()
}

/// Whether settings can be persisted in this process.
pub fn storage_available() -> bool {
    settings_directory().is_some()
}

/// The EXE builds' data folder. The Store build only ever reads it, and only
/// for the explicit one-time import ([`import_unpackaged_settings`]).
pub fn unpackaged_settings_directory() -> PathBuf {
    data_root().join(UNPACKAGED_FOLDER)
}

pub fn legacy_settings_directory() -> PathBuf {
    data_root().join("Agent Monitor")
}

fn data_root() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn resolve_data_directory(
    state: crate::packaged::PackageState,
    unpackaged: &Path,
    package_folder: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    use crate::packaged::PackageState;
    match state {
        PackageState::Unpackaged => Some(unpackaged.to_path_buf()),
        PackageState::Packaged => match package_folder() {
            Some(folder) if folder.is_absolute() && !same_or_inside(&folder, unpackaged) => {
                Some(folder)
            }
            _ => None,
        },
        PackageState::Unknown => None,
    }
}

/// `ApplicationData.Current.LocalFolder.Path`, read on a dedicated thread
/// inside its own explicitly initialized multithreaded apartment, so the
/// main/UI thread's apartment is never initialized or changed here.
#[cfg(windows)]
fn package_local_folder() -> Option<PathBuf> {
    let result = crate::winrt::run_in_multithreaded_apartment(|| {
        use windows::Storage::ApplicationData;
        let path = ApplicationData::Current()
            .and_then(|data| data.LocalFolder())
            .and_then(|folder| folder.Path())
            .map_err(|error| format!("ApplicationData.LocalFolder: {error}"))?;
        Ok(PathBuf::from(path.to_os_string()))
    });
    match result {
        Ok(path) if !path.as_os_str().is_empty() => Some(path),
        Ok(_) => None,
        Err(error) => {
            eprintln!("SETTINGS: package data folder unavailable: {error}");
            None
        }
    }
}

#[cfg(not(windows))]
fn package_local_folder() -> Option<PathBuf> {
    None
}

/// Case-insensitive (Windows paths) check that `path` is `folder` or inside it.
fn same_or_inside(path: &Path, folder: &Path) -> bool {
    let normalize = |value: &Path| PathBuf::from(value.to_string_lossy().to_lowercase());
    normalize(path).starts_with(normalize(folder))
}

/// Only a confirmed unpackaged process may write into the EXE builds' folder.
fn write_allowed_for(state: crate::packaged::PackageState, path: &Path, unpackaged: &Path) -> bool {
    state == crate::packaged::PackageState::Unpackaged || !same_or_inside(path, unpackaged)
}

/// Result of the Store build's optional import from the EXE build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    Imported,
    /// The Store build already has its own settings; nothing was changed.
    StoreSettingsExist,
    /// No EXE-build settings file was found.
    Missing,
    /// The EXE-build file is unreadable, too large, malformed (for example
    /// partially written) or has invalid values; nothing was imported.
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
    pub settings: ImportOutcome,
    pub provider_icons: usize,
}

/// The preferences the Store build takes over from the EXE build. Permissions
/// granted to the other app are never carried over: launch at sign-in, the
/// status-line bridge listener and background Claude CLI refresh stay off
/// until the user turns them on in the Store build.
pub fn sanitize_import(source: &AppSettings) -> AppSettings {
    AppSettings {
        schema_version: 1,
        theme_mode: source.theme_mode,
        language: source.language,
        codex_enabled: source.codex_enabled,
        claude_enabled: source.claude_enabled,
        claude_cli_refresh_enabled: false,
        claude_bridge_listener_enabled: false,
        automatic_update_checks_enabled: false,
        last_update_check_success_unix: None,
        last_automatic_update_check_attempt_unix: None,
        launch_at_startup: false,
        alerts_enabled: source.alerts_enabled,
        warning_threshold: source.warning_threshold,
        critical_threshold: source.critical_threshold,
        exhausted_alerts_enabled: source.exhausted_alerts_enabled,
        include_weekly: source.include_weekly,
        popup_duration_seconds: source.popup_duration_seconds,
    }
}

/// Reads and validates the EXE build's settings without writing anything.
pub fn read_import_source(source_directory: &Path) -> Result<AppSettings, ImportOutcome> {
    let data = match read_bounded(
        &source_directory.join(SETTINGS_FILE),
        SETTINGS_MIGRATION_LIMIT,
    ) {
        Ok(data) => data,
        Err(SettingsError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ImportOutcome::Missing);
        }
        Err(_) => return Err(ImportOutcome::Invalid),
    };
    let settings: AppSettings =
        serde_json::from_slice(&data).map_err(|_| ImportOutcome::Invalid)?;
    settings.validate().map_err(|_| ImportOutcome::Invalid)?;
    Ok(settings)
}

/// Whether the import can be offered: the Store build has no settings yet and
/// the EXE build's settings are valid.
pub fn import_available(source_directory: &Path, destination_directory: &Path) -> bool {
    !destination_directory.join(SETTINGS_FILE).exists()
        && !same_or_inside(destination_directory, source_directory)
        && read_import_source(source_directory).is_ok()
}

/// Explicit, one-time import from the EXE build (Store build only, started by
/// the user). The source folder is only read. Existing Store settings are never
/// overwritten: once `settings.json` exists in the destination, the import is
/// no longer possible. User-supplied provider icons with known file names are
/// copied when the destination has none of that name (used from the next
/// start). Alert history, credentials and anything else are not copied.
pub fn import_unpackaged_settings(
    source_directory: &Path,
    destination_directory: &Path,
    icon_names: &[&str],
    icon_limit: u64,
) -> Result<ImportReport, SettingsError> {
    let report = |settings, provider_icons| ImportReport {
        settings,
        provider_icons,
    };
    if same_or_inside(destination_directory, source_directory)
        || same_or_inside(source_directory, destination_directory)
    {
        return Err(SettingsError::Invalid);
    }
    let destination = destination_directory.join(SETTINGS_FILE);
    if destination.exists() {
        return Ok(report(ImportOutcome::StoreSettingsExist, 0));
    }
    let source = match read_import_source(source_directory) {
        Ok(source) => source,
        Err(outcome) => return Ok(report(outcome, 0)),
    };
    let imported = sanitize_import(&source);
    imported.validate().map_err(|_| SettingsError::Invalid)?;
    std::fs::create_dir_all(destination_directory)?;

    let mut icons = 0;
    for name in icon_names {
        if copy_file_if_absent(
            &source_directory.join(PROVIDER_ICONS_FOLDER).join(name),
            &destination_directory.join(PROVIDER_ICONS_FOLDER).join(name),
            icon_limit,
        )? {
            icons += 1;
        }
    }

    // Settings last: an interrupted import leaves no Store settings and can
    // simply be started again.
    let mut temporary = tempfile::NamedTempFile::new_in(destination_directory)?;
    serde_json::to_writer_pretty(&mut temporary, &imported)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(&destination) {
        Ok(_) => Ok(report(ImportOutcome::Imported, icons)),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(report(ImportOutcome::StoreSettingsExist, icons))
        }
        Err(error) => Err(error.error.into()),
    }
}

/// Copies one regular file (not a link) of at most `maximum_bytes`, never
/// replacing an existing destination. Returns whether it was copied.
fn copy_file_if_absent(
    source: &Path,
    destination: &Path,
    maximum_bytes: u64,
) -> Result<bool, SettingsError> {
    match std::fs::symlink_metadata(source) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= maximum_bytes => {}
        _ => return Ok(false),
    }
    if destination.exists() {
        return Ok(false);
    }
    let data = match read_bounded(source, maximum_bytes) {
        Ok(data) => data,
        Err(_) => return Ok(false),
    };
    let parent = destination.parent().ok_or(SettingsError::Invalid)?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&data)?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(destination) {
        Ok(_) => Ok(true),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.error.into()),
    }
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
        assert!(!settings.claude_bridge_listener_enabled);
        assert!(!settings.launch_at_startup);
        assert_eq!(
            (settings.warning_threshold, settings.critical_threshold),
            (25, 10)
        );
        assert!(settings.exhausted_alerts_enabled && settings.include_weekly);
        assert_eq!(settings.popup_duration_seconds, 8);
        assert_eq!(settings.theme_mode, ThemeMode::Graphite);
        assert_eq!(settings.language, Language::Ukrainian);
    }

    #[test]
    fn legacy_theme_settings_map_to_the_three_supported_themes() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"schema_version":1,"theme_mode":"dark"}"#).unwrap();
        assert_eq!(settings.theme_mode, ThemeMode::BluishDark);
        let settings: AppSettings =
            serde_json::from_str(r#"{"schema_version":1,"theme_mode":"system"}"#).unwrap();
        assert_eq!(settings.theme_mode, ThemeMode::Graphite);
    }

    #[test]
    fn settings_round_trip_atomically_and_older_missing_fields_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = AppSettings {
            codex_enabled: false,
            claude_cli_refresh_enabled: true,
            launch_at_startup: true,
            language: Language::Polish,
            automatic_update_checks_enabled: true,
            last_update_check_success_unix: Some(1_700_000_000),
            last_automatic_update_check_attempt_unix: Some(1_700_000_000),
            ..Default::default()
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        std::fs::write(&path, r#"{"schema_version":1,"codex_enabled":false}"#).unwrap();
        let loaded = load(&path).unwrap();
        assert!(!loaded.codex_enabled);
        assert!(loaded.claude_enabled);
        assert!(!loaded.claude_cli_refresh_enabled);
        assert!(!loaded.claude_bridge_listener_enabled);
        assert!(!loaded.automatic_update_checks_enabled);
        assert_eq!(loaded.last_update_check_success_unix, None);
        assert_eq!(loaded.last_automatic_update_check_attempt_unix, None);
        assert!(!loaded.launch_at_startup);
        assert_eq!(loaded.warning_threshold, 25);
        // The Store-only bridge opt-in survives a save/load round trip.
        let opted_in = AppSettings {
            claude_bridge_listener_enabled: true,
            ..Default::default()
        };
        save(&path, &opted_in).unwrap();
        assert!(load(&path).unwrap().claude_bridge_listener_enabled);
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

    use crate::packaged::PackageState;

    #[test]
    fn store_and_exe_builds_use_separate_data_folders() {
        let unpackaged = Path::new(r"C:\Users\u\AppData\Roaming\Agent Usage Monitor");
        let local_state =
            PathBuf::from(r"C:\Users\u\AppData\Local\Packages\Pfn_sfbqfhxtgjc58\LocalState");
        // EXE builds: the existing path, exactly.
        assert_eq!(
            resolve_data_directory(PackageState::Unpackaged, unpackaged, || None),
            Some(unpackaged.to_path_buf())
        );
        // Store build: the package's LocalState.
        assert_eq!(
            resolve_data_directory(PackageState::Packaged, unpackaged, || Some(
                local_state.clone()
            )),
            Some(local_state)
        );
        // Store build without a usable package folder: no persistent storage at
        // all (no %TEMP% fallback, never the EXE folder, never a relative path).
        for folder in [
            None,
            Some(unpackaged.to_path_buf()),
            Some(unpackaged.join("nested")),
            Some(PathBuf::from("LocalState")),
        ] {
            assert_eq!(
                resolve_data_directory(PackageState::Packaged, unpackaged, || folder),
                None
            );
        }
        // Unknown identity: no storage, and the package folder is not consulted.
        assert_eq!(
            resolve_data_directory(PackageState::Unknown, unpackaged, || panic!(
                "not consulted"
            )),
            None
        );
    }

    #[test]
    fn without_storage_nothing_is_read_or_saved() {
        // The empty path stands for "no persistent storage".
        let none = Path::new("");
        assert_eq!(load(none).unwrap(), AppSettings::default());
        assert!(matches!(
            save(none, &AppSettings::default()),
            Err(SettingsError::StorageUnavailable)
        ));
        // Validation still comes first.
        let invalid = AppSettings {
            critical_threshold: 50,
            ..Default::default()
        };
        assert!(matches!(save(none, &invalid), Err(SettingsError::Invalid)));
    }

    #[test]
    fn only_confirmed_unpackaged_processes_write_the_exe_settings() {
        let unpackaged = Path::new(r"C:\Roaming\Agent Usage Monitor");
        let exe_file = unpackaged.join("settings.json");
        let exe_file_other_case = Path::new(r"c:\roaming\agent usage monitor\SETTINGS.json");
        let store_file = Path::new(r"C:\Local\Packages\Pfn\LocalState\settings.json");
        assert!(write_allowed_for(
            PackageState::Unpackaged,
            &exe_file,
            unpackaged
        ));
        for state in [PackageState::Packaged, PackageState::Unknown] {
            assert!(!write_allowed_for(state, &exe_file, unpackaged));
            assert!(!write_allowed_for(state, exe_file_other_case, unpackaged));
            assert!(write_allowed_for(state, store_file, unpackaged));
        }
        // A sibling folder with a longer name is not "inside".
        assert!(write_allowed_for(
            PackageState::Packaged,
            Path::new(r"C:\Roaming\Agent Usage Monitor 2\settings.json"),
            unpackaged
        ));
    }

    fn exe_settings() -> AppSettings {
        AppSettings {
            theme_mode: ThemeMode::Light,
            language: Language::Polish,
            codex_enabled: false,
            claude_enabled: true,
            claude_cli_refresh_enabled: true,
            claude_bridge_listener_enabled: true,
            launch_at_startup: true,
            alerts_enabled: false,
            warning_threshold: 40,
            critical_threshold: 15,
            exhausted_alerts_enabled: false,
            include_weekly: false,
            popup_duration_seconds: 20,
            ..Default::default()
        }
    }

    #[test]
    fn import_copies_preferences_but_never_permissions() {
        let imported = sanitize_import(&exe_settings());
        assert_eq!(imported.theme_mode, ThemeMode::Light);
        assert_eq!(imported.language, Language::Polish);
        assert!(!imported.codex_enabled && imported.claude_enabled);
        assert!(!imported.alerts_enabled);
        assert_eq!(
            (imported.warning_threshold, imported.critical_threshold),
            (40, 15)
        );
        assert!(!imported.exhausted_alerts_enabled && !imported.include_weekly);
        assert_eq!(imported.popup_duration_seconds, 20);
        // Autostart is never enabled implicitly; the bridge and the background
        // CLI stay off until the user opts in again in the Store build.
        assert!(!imported.launch_at_startup);
        assert!(!imported.claude_bridge_listener_enabled);
        assert!(!imported.claude_cli_refresh_enabled);
        assert!(imported.validate().is_ok());
    }

    #[test]
    fn import_is_read_only_toward_the_source_and_one_time() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("Roaming").join("Agent Usage Monitor");
        let destination = root.path().join("LocalState");
        std::fs::create_dir_all(source.join("provider-icons")).unwrap();
        let source_bytes = serde_json::to_vec_pretty(&exe_settings()).unwrap();
        std::fs::write(source.join("settings.json"), &source_bytes).unwrap();
        std::fs::write(source.join("alert-state.json"), b"{}").unwrap();
        std::fs::write(source.join("credentials.json"), b"never copied").unwrap();
        std::fs::write(source.join("provider-icons").join("codex.svg"), b"<svg/>").unwrap();
        std::fs::write(source.join("provider-icons").join("other.svg"), b"<svg/>").unwrap();
        std::fs::write(
            source.join("provider-icons").join("claude.png"),
            vec![0u8; 33],
        )
        .unwrap();

        assert!(import_available(&source, &destination));
        let report =
            import_unpackaged_settings(&source, &destination, &["codex.svg", "claude.png"], 32)
                .unwrap();
        assert_eq!(report.settings, ImportOutcome::Imported);
        // Only known names within the size limit.
        assert_eq!(report.provider_icons, 1);
        assert!(
            destination
                .join("provider-icons")
                .join("codex.svg")
                .exists()
        );
        assert!(
            !destination
                .join("provider-icons")
                .join("claude.png")
                .exists()
        );
        assert!(
            !destination
                .join("provider-icons")
                .join("other.svg")
                .exists()
        );
        assert!(!destination.join("credentials.json").exists());
        assert!(!destination.join("alert-state.json").exists());
        let loaded = load(&destination.join("settings.json")).unwrap();
        assert_eq!(loaded, sanitize_import(&exe_settings()));
        // The source is untouched.
        assert_eq!(
            std::fs::read(source.join("settings.json")).unwrap(),
            source_bytes
        );

        // One time: Store settings now exist and are never overwritten.
        assert!(!import_available(&source, &destination));
        let store_bytes = std::fs::read(destination.join("settings.json")).unwrap();
        let mut changed = exe_settings();
        changed.language = Language::Russian;
        std::fs::write(
            source.join("settings.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        let repeated =
            import_unpackaged_settings(&source, &destination, &["codex.svg"], 32).unwrap();
        assert_eq!(repeated.settings, ImportOutcome::StoreSettingsExist);
        assert_eq!(
            std::fs::read(destination.join("settings.json")).unwrap(),
            store_bytes
        );
    }

    #[test]
    fn existing_store_settings_block_the_import() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("exe");
        let destination = root.path().join("store");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(
            source.join("settings.json"),
            serde_json::to_vec(&exe_settings()).unwrap(),
        )
        .unwrap();
        std::fs::write(destination.join("settings.json"), b"store-owned").unwrap();
        assert!(!import_available(&source, &destination));
        let report = import_unpackaged_settings(&source, &destination, &[], 32).unwrap();
        assert_eq!(report.settings, ImportOutcome::StoreSettingsExist);
        assert_eq!(
            std::fs::read(destination.join("settings.json")).unwrap(),
            b"store-owned"
        );
    }

    #[test]
    fn missing_invalid_and_partial_sources_are_rejected_without_writes() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("store");
        let missing = root.path().join("missing");
        assert_eq!(
            import_unpackaged_settings(&missing, &destination, &[], 32)
                .unwrap()
                .settings,
            ImportOutcome::Missing
        );
        let full = serde_json::to_vec(&exe_settings()).unwrap();
        let cases: [(&str, Vec<u8>); 5] = [
            ("partial", full[..full.len() / 2].to_vec()),
            ("empty", Vec::new()),
            (
                "invalid-values",
                br#"{"schema_version":1,"warning_threshold":10,"critical_threshold":50}"#.to_vec(),
            ),
            ("future-schema", br#"{"schema_version":2}"#.to_vec()),
            (
                "oversized",
                vec![b' '; SETTINGS_MIGRATION_LIMIT as usize + 1],
            ),
        ];
        for (name, bytes) in cases {
            let source = root.path().join(name);
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(source.join("settings.json"), &bytes).unwrap();
            assert!(!import_available(&source, &destination), "{name}");
            let report = import_unpackaged_settings(&source, &destination, &[], 32).unwrap();
            assert_eq!(report.settings, ImportOutcome::Invalid, "{name}");
            assert!(!destination.join("settings.json").exists(), "{name}");
            assert_eq!(
                std::fs::read(source.join("settings.json")).unwrap(),
                bytes,
                "{name}"
            );
        }
    }

    #[test]
    fn import_refuses_overlapping_folders() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("Agent Usage Monitor");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("settings.json"),
            serde_json::to_vec(&exe_settings()).unwrap(),
        )
        .unwrap();
        assert!(import_unpackaged_settings(&source, &source, &[], 32).is_err());
        assert!(import_unpackaged_settings(&source, &source.join("store"), &[], 32).is_err());
        assert!(!import_available(&source, &source.join("store")));
    }
}

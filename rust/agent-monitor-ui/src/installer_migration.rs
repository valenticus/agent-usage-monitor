//! One-time, per-install migration of the v0.2.0 Inno Claude Bridge choice.
//! Only the matching unpackaged installation may consume the marker. Portable
//! and packaged instances deliberately ignore it.

use crate::packaged::PackageState;
use crate::settings::AppSettings;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marker {
    Pending { executable: PathBuf },
    Done { executable: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Ignore,
    PersistOptIn,
    MarkDone,
}

fn action_for(
    marker: &Marker,
    current_executable: &Path,
    package_state: PackageState,
    installed_exe: bool,
    current_choice: Option<bool>,
) -> Action {
    if package_state != PackageState::Unpackaged || !installed_exe {
        return Action::Ignore;
    }
    match marker {
        Marker::Pending { executable } if same_path(executable, current_executable) => {
            match current_choice {
                Some(_) => Action::MarkDone,
                None => Action::PersistOptIn,
            }
        }
        _ => Action::Ignore,
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

/// Consume an installer-created pending marker. Returns `true` when the marker
/// was completed. If settings persistence fails it remains Pending for retry.
pub fn consume_pending(settings: &mut AppSettings, settings_path: &Path) -> Result<bool, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let installed_exe = is_installer_managed_executable(&executable);
    if !installed_exe {
        // A Portable copy has independent legacy Run evidence. Never consult
        // the install-wide marker: an installed copy may own a terminal Done.
        return migrate_legacy_portable_run(settings, settings_path, &executable);
    }
    let Some(marker) = read_marker()? else {
        return Ok(false);
    };
    match action_for(
        &marker,
        &executable,
        crate::packaged::state(),
        installed_exe,
        settings.exe_claude_bridge_enabled,
    ) {
        Action::Ignore => Ok(false),
        Action::PersistOptIn => {
            persist_choice(settings, true, |updated| {
                crate::settings::save(settings_path, updated)
                    .map_err(|error| format!("could not persist migrated choice: {error}"))
            })?;
            mark_done()?;
            Ok(true)
        }
        Action::MarkDone => {
            mark_done()?;
            Ok(true)
        }
    }
}

fn migrate_legacy_portable_run(
    settings: &mut AppSettings,
    settings_path: &Path,
    executable: &Path,
) -> Result<bool, String> {
    if crate::packaged::state() != PackageState::Unpackaged
        || settings.exe_claude_bridge_enabled.is_some()
        || !crate::autostart::has_legacy_bridge_opt_in(executable)?
    {
        return Ok(false);
    }
    migrate_legacy_run_choice(
        settings,
        true,
        |updated| {
            crate::settings::save(settings_path, updated)
                .map_err(|error| format!("could not persist Portable bridge choice: {error}"))
        },
        || crate::autostart::remove_legacy_bridge_flag(executable),
    )
}

fn migrate_legacy_run_choice(
    settings: &mut AppSettings,
    positive_evidence: bool,
    persist: impl FnOnce(&AppSettings) -> Result<(), String>,
    remove_legacy_flag: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    if !positive_evidence || settings.exe_claude_bridge_enabled.is_some() {
        return Ok(false);
    }
    persist_choice(settings, true, persist)?;
    remove_legacy_flag()
        .map_err(|error| format!("bridge choice saved; legacy flag remains: {error}"))?;
    Ok(true)
}

fn is_installer_managed_executable(executable: &Path) -> bool {
    executable
        .parent()
        .is_some_and(|directory| directory.join("unins000.exe").is_file())
}

fn persist_choice(
    settings: &mut AppSettings,
    enabled: bool,
    save: impl FnOnce(&AppSettings) -> Result<(), String>,
) -> Result<(), String> {
    let previous = settings.exe_claude_bridge_enabled;
    settings.exe_claude_bridge_enabled = Some(enabled);
    if let Err(error) = save(settings) {
        settings.exe_claude_bridge_enabled = previous;
        return Err(error);
    }
    Ok(())
}

#[cfg(not(windows))]
fn read_marker() -> Result<Option<Marker>, String> {
    Ok(None)
}

#[cfg(not(windows))]
fn mark_done() -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
const KEY: &str = r"Software\Valenticus\AgentUsageMonitor\InstallerMigration";

#[cfg(windows)]
fn read_marker() -> Result<Option<Marker>, String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_QUERY_VALUE, RegCloseKey, RegOpenKeyExW,
    };
    let mut key = std::ptr::null_mut();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(KEY).as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if result == 2 {
        return Ok(None);
    }
    if result != 0 {
        return Err(format!("could not open migration marker ({result})"));
    }
    let state = query_string(key, "State");
    let executable = query_string(key, "InstallExe");
    let bridge = query_string(key, "BridgeOptIn");
    unsafe { RegCloseKey(key) };
    let state_value = state?;
    let executable_value = executable?;
    let bridge_value = if state_value.as_deref() == Some("Pending") {
        bridge?
    } else {
        None
    };
    let (Some(state), Some(executable)) = (state_value, executable_value) else {
        return Ok(None);
    };
    let executable = PathBuf::from(executable);
    match state.as_str() {
        "Pending" => match bridge_value {
            Some(value) if value == "1" => Ok(Some(Marker::Pending { executable })),
            _ => Ok(None),
        },
        "Done" => Ok(Some(Marker::Done { executable })),
        _ => Ok(None),
    }
}

#[cfg(windows)]
fn mark_done() -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegSetValueExW,
    };
    let mut key = std::ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(KEY).as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if result != 0 {
        return Err(format!(
            "could not open migration marker for update ({result})"
        ));
    }
    let mut state: Vec<u16> = "Done".encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        RegSetValueExW(
            key,
            wide("State").as_ptr(),
            0,
            REG_SZ,
            state.as_mut_ptr().cast(),
            (state.len() * 2) as u32,
        )
    };
    unsafe { RegCloseKey(key) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("could not mark migration complete ({result})"))
    }
}

#[cfg(windows)]
fn query_string(
    key: windows_sys::Win32::System::Registry::HKEY,
    name: &str,
) -> Result<Option<String>, String> {
    use windows_sys::Win32::System::Registry::{REG_SZ, RegQueryValueExW};
    let mut kind = 0;
    let mut bytes = 0u32;
    let value_name = wide(name);
    let mut result = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if result == 2 {
        return Ok(None);
    }
    if result != 0 || kind != REG_SZ || bytes < 2 || bytes % 2 != 0 {
        return Err(format!("invalid migration marker value {name} ({result})"));
    }
    let mut value = vec![0u16; (bytes / 2) as usize];
    result = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            value.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if result != 0 || kind != REG_SZ {
        return Err(format!(
            "could not read migration marker value {name} ({result})"
        ));
    }
    let len = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    Ok(Some(String::from_utf16_lossy(&value[..len])))
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALLER_SCRIPT: &str = include_str!("../../../packaging/windows/AgentUsageMonitor.iss");

    fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start = source.find(start).expect("start marker");
        let tail = &source[start..];
        let end = tail.find(end).expect("end marker");
        &tail[..end]
    }

    #[test]
    fn migration_is_off_by_default_and_applies_only_matching_pending_evidence() {
        let exe = Path::new(
            r"C:\Users\u\AppData\Local\Programs\AgentUsageMonitor\agent-usage-monitor.exe",
        );
        let marker = Marker::Pending {
            executable: exe.to_path_buf(),
        };
        assert_eq!(
            action_for(&marker, exe, PackageState::Unpackaged, true, None),
            Action::PersistOptIn
        );
        assert_eq!(
            action_for(&marker, exe, PackageState::Unpackaged, true, Some(false)),
            Action::MarkDone
        );
        assert_eq!(
            action_for(
                &marker,
                Path::new(r"D:\Portable\agent-usage-monitor.exe"),
                PackageState::Unpackaged,
                true,
                None
            ),
            Action::Ignore
        );
        assert_eq!(
            action_for(&marker, exe, PackageState::Packaged, true, None),
            Action::Ignore
        );
        assert_eq!(
            action_for(&marker, exe, PackageState::Unpackaged, false, None),
            Action::Ignore
        );
    }

    #[test]
    fn done_marker_never_reapplies_old_evidence() {
        let exe = Path::new(r"C:\AUM\agent-usage-monitor.exe");
        let marker = Marker::Done {
            executable: exe.to_path_buf(),
        };
        assert_eq!(
            action_for(&marker, exe, PackageState::Unpackaged, true, None),
            Action::Ignore
        );
    }

    #[test]
    fn portable_copy_never_consumes_an_installers_marker_even_at_matching_path() {
        let exe = Path::new(r"C:\AUM\agent-usage-monitor.exe");
        let marker = Marker::Pending {
            executable: exe.to_path_buf(),
        };
        assert_eq!(
            action_for(&marker, exe, PackageState::Unpackaged, false, None),
            Action::Ignore
        );
    }

    #[test]
    fn failed_settings_write_keeps_choice_unset_for_retry() {
        let mut settings = AppSettings::default();
        let result = persist_choice(&mut settings, true, |_| Err("read-only".into()));
        assert_eq!(result, Err("read-only".into()));
        assert_eq!(settings.exe_claude_bridge_enabled, None);
    }

    #[test]
    fn portable_run_migration_saves_before_removing_flag() {
        let mut settings = AppSettings::default();
        let persisted = std::cell::Cell::new(false);
        let mut removed = false;
        let result = migrate_legacy_run_choice(
            &mut settings,
            true,
            |updated| {
                assert_eq!(updated.exe_claude_bridge_enabled, Some(true));
                persisted.set(true);
                Ok(())
            },
            || {
                assert!(persisted.get());
                removed = true;
                Ok(())
            },
        );
        assert_eq!(result, Ok(true));
        assert_eq!(settings.exe_claude_bridge_enabled, Some(true));
        assert!(removed);
    }

    #[test]
    fn portable_run_migration_failure_preserves_legacy_flag_and_choice() {
        let mut settings = AppSettings::default();
        let mut removed = false;
        let result = migrate_legacy_run_choice(
            &mut settings,
            true,
            |_| Err("settings write failed".into()),
            || {
                removed = true;
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(settings.exe_claude_bridge_enabled, None);
        assert!(!removed);
    }

    #[test]
    fn portable_run_migration_does_not_infer_off_or_override_explicit_choice() {
        let mut default_settings = AppSettings::default();
        assert_eq!(
            migrate_legacy_run_choice(&mut default_settings, false, |_| Ok(()), || Ok(())),
            Ok(false)
        );
        assert_eq!(default_settings.exe_claude_bridge_enabled, None);

        let mut opted_out = AppSettings {
            exe_claude_bridge_enabled: Some(false),
            ..Default::default()
        };
        let mut persisted = false;
        assert_eq!(
            migrate_legacy_run_choice(
                &mut opted_out,
                true,
                |_| {
                    persisted = true;
                    Ok(())
                },
                || Ok(())
            ),
            Ok(false)
        );
        assert!(!persisted);
        assert_eq!(opted_out.exe_claude_bridge_enabled, Some(false));
    }

    #[test]
    fn portable_migration_is_independent_of_an_installed_copies_done_marker() {
        let source = include_str!("installer_migration.rs");
        let consume = source_between(
            source,
            "pub fn consume_pending",
            "fn migrate_legacy_portable_run",
        );
        let portable_branch = consume.find("if !installed_exe").expect("Portable branch");
        let marker_read = consume.find("read_marker()").expect("marker lookup");
        assert!(portable_branch < marker_read);
        assert!(
            consume[portable_branch..marker_read]
                .contains("migrate_legacy_portable_run(settings, settings_path, &executable)")
        );

        let portable = source_between(
            source,
            "fn migrate_legacy_portable_run",
            "fn migrate_legacy_run_choice",
        );
        assert!(portable.contains("PackageState::Unpackaged"));
        assert!(portable.contains("settings.exe_claude_bridge_enabled.is_some()"));
        assert!(portable.contains("has_legacy_bridge_opt_in(executable)"));
        assert!(portable.contains("remove_legacy_bridge_flag(executable)"));
    }

    #[test]
    fn portable_run_flag_removal_failure_keeps_successfully_saved_opt_in() {
        let mut settings = AppSettings::default();
        let result = migrate_legacy_run_choice(
            &mut settings,
            true,
            |_| Ok(()),
            || Err("Run value could not be updated".into()),
        );
        assert!(result.is_err());
        assert_eq!(settings.exe_claude_bridge_enabled, Some(true));
    }

    #[test]
    fn installer_checks_selected_directory_only_after_initialize_setup() {
        let initialize = source_between(
            INSTALLER_SCRIPT,
            "function InitializeSetup()",
            "function IsPre030RunOptIn",
        );
        assert!(!initialize.contains("ExpandConstant('{app}"));
        let prepare = source_between(
            INSTALLER_SCRIPT,
            "function PrepareToInstall",
            "function InitializeUninstall",
        );
        assert!(prepare.contains("ExpandConstant('{app}\\{#AppExe}')"));
        assert!(prepare.contains("SamePath(PreviousInstallPath"));
        assert!(prepare.contains("FileExists(AppExePath)"));
        assert!(prepare.contains("WriteMigrationMarker"));
        assert!(prepare.contains("MigrationPersistenceFailed"));
    }

    #[test]
    fn empty_selected_tasks_do_not_suppress_positive_shortcut_or_run_evidence() {
        let detect = source_between(
            INSTALLER_SCRIPT,
            "procedure DetectLegacyBridgeChoice",
            "function InitializeSetup()",
        );
        let task_check = source_between(
            detect,
            "if RegQueryStringValue(HKCU, '{#UninstallKey}'",
            "if ShortcutEvidence",
        );
        assert!(task_check.contains("ContainsLegacyTask(Tasks)"));
        assert!(!task_check.contains("exit;"));
        assert!(detect.contains("ShortcutEvidence("));
        assert!(detect.contains("RunCommandEvidence("));
        assert!(detect.contains("LegacyEvidenceCaptured := True"));
    }

    #[test]
    fn installer_marker_write_preserves_existing_state_and_only_writes_pending_for_evidence() {
        let writer = source_between(
            INSTALLER_SCRIPT,
            "function WriteMigrationMarker",
            "procedure RefreshOwnedStartMenuShortcut",
        );
        assert!(writer.contains("if RegKeyExists(HKCU, '{#MigrationKey}') then begin"));
        assert!(writer.contains("{ Preserve every existing marker"));
        assert!(
            writer.contains("if not (LegacyEvidenceCaptured and LegacyBridgeEnabled) then begin")
        );
        assert!(writer.contains("State := 'Pending';"));
        assert!(!writer.contains("State := 'Done';"));
        assert!(writer.contains("RegWriteStringValue(HKCU, '{#MigrationKey}', 'State', State)"));
    }

    #[test]
    fn installer_refresh_requires_owned_shortcut_and_replaces_helper_first() {
        let shortcut = source_between(
            INSTALLER_SCRIPT,
            "function ShortcutEvidence",
            "function RunCommandEvidence",
        );
        assert!(shortcut.contains("Link.TargetPath"));
        assert!(shortcut.contains("Link.WorkingDirectory"));
        assert!(shortcut.contains("System.AppUserModel.ID"));
        assert!(shortcut.contains("--enable-claude-bridge"));
        let files = source_between(INSTALLER_SCRIPT, "[Files]", "[Icons]");
        assert!(files.find("{#BridgeExe}").unwrap() < files.find("{#AppExe}").unwrap());
        assert!(!files.contains("restartreplace"));
    }

    #[test]
    fn portable_opt_in_requires_an_exact_run_target_and_same_install_directory() {
        let detect = source_between(
            INSTALLER_SCRIPT,
            "function IsPre030RunOptIn",
            "function ShouldCreateStartMenuShortcut",
        );
        assert!(detect.contains("CompareText(Args, '--enable-claude-bridge') <> 0"));
        assert!(detect.contains("FileExists(ExePath)"));
        let prepare = source_between(
            INSTALLER_SCRIPT,
            "function PrepareToInstall",
            "function InitializeUninstall",
        );
        assert!(prepare.contains("SamePath(LegacyPortablePath, AppExePath)"));
        assert!(prepare.contains("LegacyRunOwned := True"));
        let run_update = source_between(
            INSTALLER_SCRIPT,
            "procedure RemoveMigratedRunFlag",
            "function TryReserveAumMutex",
        );
        assert!(run_update.contains("LegacyRunOwned and MigrationMarkerReady"));
        assert!(run_update.contains("RegWriteStringValue(HKCU"));
    }

    #[test]
    fn silent_installer_fails_at_prepare_before_any_files_are_replaced() {
        let prepare = source_between(
            INSTALLER_SCRIPT,
            "function PrepareToInstall",
            "function InitializeUninstall",
        );
        assert!(prepare.contains("WaitForAumReservation(not WizardSilent"));
        assert!(INSTALLER_SCRIPT.contains("CloseApplications=no"));
        assert!(INSTALLER_SCRIPT.contains("SetupMutex={#InstallerMutexName}"));
        let post_install = source_between(
            INSTALLER_SCRIPT,
            "procedure CurStepChanged",
            "procedure CurUninstallStepChanged",
        );
        assert!(!post_install.contains("WriteMigrationMarker"));
    }

    #[test]
    fn no_evidence_done_marker_is_written_only_after_successful_setup() {
        let prepare = source_between(
            INSTALLER_SCRIPT,
            "function PrepareToInstall",
            "function InitializeUninstall",
        );
        assert!(!prepare.contains("State := 'Done'"));
        assert!(prepare.contains("WriteMigrationMarker"));
        let post_install = source_between(
            INSTALLER_SCRIPT,
            "procedure CurStepChanged",
            "procedure CurUninstallStepChanged",
        );
        assert!(post_install.contains("WriteCompletedMigrationMarker"));
        assert!(INSTALLER_SCRIPT.contains("function WriteCompletedMigrationMarker"));
    }

    #[test]
    fn installer_uses_checked_normal_wizard_transition_after_license_acceptance() {
        const PHASE_B_DOC: &str = include_str!("../../../docs/smart-installer-phase-b.md");
        let setup = source_between(INSTALLER_SCRIPT, "[Setup]", "[Languages]");
        assert!(setup.contains("DisableWelcomePage=yes"));
        assert!(setup.contains("DisableReadyPage=yes"));
        assert!(setup.contains("DisableFinishedPage=yes"));
        assert!(setup.contains("DisableDirPage=yes"));
        assert!(setup.contains("UsePreviousAppDir=yes"));
        assert!(INSTALLER_SCRIPT.contains("LicenseFile=..\\..\\LICENSE"));
        assert!(!INSTALLER_SCRIPT.contains("\n[Run]\n"));
        assert!(PHASE_B_DOC.contains("`/DIR`"));
        assert!(PHASE_B_DOC.contains("UsePreviousAppDir=yes"));
        assert!(PHASE_B_DOC.contains("after acceptance"));

        let transition = source_between(
            INSTALLER_SCRIPT,
            "procedure AutoAdvanceTimerProc",
            "procedure RetryPreparationClick",
        );
        assert!(transition.contains("WizardForm.NextButton.Visible := True"));
        assert!(transition.contains("WizardForm.NextButton.CanFocus"));
        assert!(transition.contains("SendMessageW(WizardForm.NextButton.Handle, BM_CLICK"));
        assert!(transition.contains("CurrentWizardPageID = PreviousPageID"));
        assert!(transition.contains("WizardForm.LicenseAcceptedRadio.Checked"));
        assert!(transition.contains("StopAutoAdvanceTimer"));
        assert!(!transition.contains("PostMessageW"));

        for language in ["ukrainian", "english", "polish", "russian"] {
            assert!(INSTALLER_SCRIPT.contains(&format!("{language}.RetryPreparation=")));
            assert!(INSTALLER_SCRIPT.contains(&format!("{language}.RetryBackFallback=")));
            assert!(INSTALLER_SCRIPT.contains(&format!("{language}.AutoStartUnavailable=")));
        }

        let scheduler = source_between(
            INSTALLER_SCRIPT,
            "procedure StartAutoAdvanceTimer",
            "procedure InitializeWizard",
        );
        assert!(scheduler.contains("SetTimer(0, 0, 150"));
    }

    #[test]
    fn prepare_failure_exposes_retry_and_reenters_normal_page_flow() {
        let retry = source_between(
            INSTALLER_SCRIPT,
            "procedure RetryPreparationClick",
            "procedure InitializeWizard",
        );
        assert!(retry.contains("if not PrepareFailurePending then exit"));
        assert!(retry.contains("WizardForm.BackButton.Visible := True"));
        assert!(retry.contains("WizardForm.BackButton.CanFocus"));
        assert!(retry.contains("SendMessageW(WizardForm.BackButton.Handle, BM_CLICK"));
        assert!(retry.contains("CurrentWizardPageID = PreviousPageID"));
        assert!(retry.contains("CustomMessage('RetryBackFallback')"));
        assert!(!retry.contains("PrepareToInstall("));

        let preparation = source_between(
            INSTALLER_SCRIPT,
            "function FailPreparation",
            "function InitializeUninstall",
        );
        assert!(preparation.contains("PrepareFailurePending := True"));
        assert!(!preparation.contains("RetryButton.Enabled := True"));
        assert!(!preparation.contains("RetryButton.Visible := True"));
        assert!(preparation.contains("FailPreparation(CustomMessage('InstallPathMismatch'))"));
        assert!(preparation.contains("FailPreparation(CustomMessage('InstallPathOccupied'))"));
        assert!(preparation.contains("FailPreparation(ErrorText)"));
        assert!(
            preparation.contains("FailPreparation(CustomMessage('MigrationPersistenceFailed'))")
        );
        assert!(preparation.contains("FileExists(AppExePath)"));
        assert!(preparation.contains("SamePath(PreviousInstallPath, ExpandConstant('{app}')"));
    }

    #[test]
    fn destination_errors_explain_cancel_and_dir_restart_in_every_language() {
        for language in ["ukrainian", "english", "polish", "russian"] {
            for key in ["InstallPathMismatch", "InstallPathOccupied"] {
                let line = INSTALLER_SCRIPT
                    .lines()
                    .find(|line| line.starts_with(&format!("{language}.{key}=")))
                    .expect("localized destination error");
                assert!(line.contains("/DIR"), "{language}.{key}: {line}");
                assert!(
                    line.to_lowercase().contains("setup")
                        || line.contains("Setup")
                        || line.contains("instal")
                        || line.contains("установ")
                        || line.contains("інстал"),
                    "{language}.{key}: {line}"
                );
            }
        }
    }

    #[test]
    fn readme_is_installed_without_auto_open_and_setup_requires_inno_673() {
        let readme = INSTALLER_SCRIPT
            .lines()
            .find(|line| line.contains("README.md"))
            .expect("README staging entry");
        assert!(readme.contains("DestDir: \"{app}\""));
        assert!(readme.contains("Flags: ignoreversion"));
        assert!(!readme.contains("isreadme"));
        assert!(INSTALLER_SCRIPT.contains("#if Ver < EncodeVer(6, 7, 3)"));
        assert!(
            INSTALLER_SCRIPT.contains("#error This installer requires Inno Setup 6.7.3 or newer.")
        );
    }

    #[test]
    fn install_launch_uses_original_user_context() {
        let completion = source_between(
            INSTALLER_SCRIPT,
            "procedure CurStepChanged",
            "procedure CurUninstallStepChanged",
        );
        assert!(completion.contains("ExecAsOriginalUser("));
        assert!(!completion.contains("if not Exec("));
    }

    #[test]
    fn installer_launches_once_only_after_successful_interactive_completion() {
        let completion = source_between(
            INSTALLER_SCRIPT,
            "procedure CurStepChanged",
            "procedure CurUninstallStepChanged",
        );
        assert!(completion.contains("CurStep <> ssDone"));
        assert!(completion.contains("WizardSilent"));
        assert!(completion.contains("InstallLaunchAttempted"));
        assert!(completion.contains("InstallCompleted"));
        assert!(completion.contains("InstallCompleted := True"));
        assert!(completion.contains("LaunchFailed"));
        assert!(!INSTALLER_SCRIPT.contains("\n[Run]\n"));
        assert!(completion.contains(
            "CurStep <> ssDone) or not InstallCompleted or WizardSilent or InstallLaunchAttempted"
        ));
    }

    #[test]
    fn restart_required_preflight_keeps_inno_controls_and_cannot_launch() {
        let page_change = source_between(
            INSTALLER_SCRIPT,
            "procedure CurPageChanged(CurPageID: Integer)",
            "procedure DeinitializeSetup",
        );
        let preparing_start = page_change
            .find("if CurPageID = wpPreparing then begin")
            .expect("Preparing page handled explicitly");
        let retry_reset = page_change
            .find("RetryButton.Visible := False;")
            .expect("generic Retry reset exists");
        assert!(preparing_start < retry_reset);
        let preparing = &page_change[preparing_start..retry_reset];
        assert!(preparing.contains("PrepareFailurePending and not WizardSilent"));
        assert!(preparing.contains("WizardForm.NextButton.Visible := False"));
        assert!(preparing.contains("RetryButton.Enabled := True"));
        assert!(preparing.contains("RetryButton.Visible := True"));
        assert!(!preparing.contains("CancelButton.Visible := False"));
        assert!(page_change.contains("if CurPageID = wpFinished then\n    exit"));

        let failure = source_between(
            INSTALLER_SCRIPT,
            "function FailPreparation",
            "function PrepareToInstall",
        );
        assert!(failure.contains("PrepareFailurePending := True"));
        assert!(!failure.contains("RetryButton.Visible := True"));
        let completion = source_between(
            INSTALLER_SCRIPT,
            "procedure CurStepChanged",
            "procedure CurUninstallStepChanged",
        );
        let post_install = completion.find("InstallCompleted := True").unwrap();
        let launch_guard = completion
            .find("not InstallCompleted")
            .expect("launch is gated by completed install");
        assert!(post_install < launch_guard);
        assert!(completion.contains("if CurStep = ssPostInstall then begin"));
    }

    #[test]
    fn uninstall_wait_never_updates_setup_wizard_form() {
        let wait = source_between(
            INSTALLER_SCRIPT,
            "function WaitForAumReservation",
            "function PrepareToInstall",
        );
        assert!(wait.contains("if not UninstallContext then WizardForm.Update"));
        assert!(INSTALLER_SCRIPT.contains("WaitForAumReservation(not UninstallSilent, True"));
    }

    #[test]
    fn uninstall_uses_only_uninstall_specific_wait_and_failure_messages() {
        let wait = source_between(
            INSTALLER_SCRIPT,
            "function WaitForAumReservation",
            "function PrepareToInstall",
        );
        assert!(wait.contains("PromptText := CustomMessage('UninstallRunningWait')"));
        assert!(wait.contains("PromptText := CustomMessage('UninstallStillBlocked')"));
        let uninstall = source_between(
            INSTALLER_SCRIPT,
            "function InitializeUninstall()",
            "function WriteCompletedMigrationMarker",
        );
        assert!(uninstall.contains("CustomMessage('UninstallMutexFailure')"));
        assert!(uninstall.contains("CustomMessage('UninstallRunningBlocker')"));
        assert!(!uninstall.contains("CustomMessage('MutexFailure')"));
        assert!(!uninstall.contains("CustomMessage('RunningBlocker')"));
        assert!(!uninstall.contains("CustomMessage('RunningStillBlocked')"));
        for key in ["UninstallRunningWait", "UninstallStillBlocked"] {
            assert!(INSTALLER_SCRIPT.contains(&format!("english.{key}=")));
            assert!(INSTALLER_SCRIPT.contains(&format!("ukrainian.{key}=")));
            assert!(INSTALLER_SCRIPT.contains(&format!("polish.{key}=")));
            assert!(INSTALLER_SCRIPT.contains(&format!("russian.{key}=")));
        }
    }
}

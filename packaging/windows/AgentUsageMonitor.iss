; Inno Setup 6 script for AgentUsageMonitor (Windows x64, per-user).
;
; Build with packaging\windows\build-installer.ps1, which compiles the release
; binaries, stages the static AUM icon and passes the version from Cargo.toml.
; Manual build (from this folder):
;   ISCC.exe /DAppVersion=0.2.0 /DStageDir=..\..\rust\target\installer\stage AgentUsageMonitor.iss
;
; Policy for this release:
; - Per-user install under %LOCALAPPDATA%\Programs; no administrator rights.
; - Start menu shortcut; desktop shortcut only when chosen.
; - Optional per-user Windows sign-in launch, disabled by default; no automatic updates or telemetry.
; - Provider (Codex/Claude) marks are embedded inside agent-usage-monitor.exe
;   by build.rs from the tracked original SVGs in
;   rust/agent-monitor-ui/assets/providers/; no loose logo files are
;   installed. Provenance and distribution approval: docs/design/brand-assets.md.
;   The user's %APPDATA%\Agent Usage Monitor\provider-icons folder is never
;   touched.
; - Settings are kept on upgrade and uninstall unless the user explicitly
;   chooses to remove them during uninstall.
; - The Claude Code status-line bridge stays opt-in (unchecked task).

#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#ifndef StageDir
  #define StageDir "..\..\rust\target\installer\stage"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\rust\target\installer"
#endif

#define AppName "AgentUsageMonitor"
#define AppExe "agent-usage-monitor.exe"
#define BridgeExe "agent-usage-monitor-claude-bridge.exe"
#define AppUserModelID "Valenticus.AgentUsageMonitor"
#define AppMutexName "Local\AgentUsageMonitor.Singleton"
#define RepoUrl "https://github.com/valenticus/agent-usage-monitor"

[Setup]
; Never change AppId: it links upgrades and the uninstall entry.
AppId={{11666F0A-E4E2-413E-B875-2472A4EFDB84}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Valentyn Fetisov
AppPublisherURL={#RepoUrl}
AppSupportURL={#RepoUrl}/issues
AppUpdatesURL={#RepoUrl}/releases
AppCopyright=Copyright (c) 2026 Valentyn Fetisov
VersionInfoVersion={#AppVersion}.0
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} Setup
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 1809 or later (toast AppUserModelID registration, DWM APIs).
MinVersion=10.0.17763
LicenseFile=..\..\LICENSE
SetupIconFile={#StageDir}\aum.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
; Ask a running AUM to close (Restart Manager) and detect it by its mutex.
AppMutex={#AppMutexName}
CloseApplications=yes
RestartApplications=no
OutputDir={#OutputDir}
OutputBaseFilename=AgentUsageMonitor-{#AppVersion}-win-x64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "ukrainian"; MessagesFile: "compiler:Languages\Ukrainian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "polish"; MessagesFile: "compiler:Languages\Polish.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[CustomMessages]
ukrainian.BridgeTask=Увімкнути міст статус-рядка Claude Code (за бажанням)
english.BridgeTask=Enable the Claude Code status-line bridge (opt-in)
polish.BridgeTask=Włącz mostek paska stanu Claude Code (opcjonalnie)
russian.BridgeTask=Включить мост строки состояния Claude Code (по желанию)
ukrainian.IntegrationGroup=Інтеграції:
english.IntegrationGroup=Integrations:
polish.IntegrationGroup=Integracje:
russian.IntegrationGroup=Интеграции:
ukrainian.RemoveSettings=Видалити також збережені дані AgentUsageMonitor?
english.RemoveSettings=Also delete saved AgentUsageMonitor data?
polish.RemoveSettings=Czy usunąć również zapisane dane AgentUsageMonitor?
russian.RemoveSettings=Удалить также сохранённые данные AgentUsageMonitor?

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "claudebridge"; Description: "{cm:BridgeTask}"; GroupDescription: "{cm:IntegrationGroup}"; Flags: unchecked

[Files]
Source: "{#StageDir}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\{#BridgeExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
; Same AppUserModelID as the running app, so toasts and the taskbar group
; under the shortcut's name and icon.
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; AppUserModelID: "{#AppUserModelID}"; Tasks: not claudebridge
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Parameters: "--enable-claude-bridge"; WorkingDir: "{app}"; AppUserModelID: "{#AppUserModelID}"; Tasks: claudebridge
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; AppUserModelID: "{#AppUserModelID}"; Tasks: desktopicon and not claudebridge
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Parameters: "--enable-claude-bridge"; WorkingDir: "{app}"; AppUserModelID: "{#AppUserModelID}"; Tasks: desktopicon and claudebridge

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent; Tasks: not claudebridge
Filename: "{app}\{#AppExe}"; Parameters: "--enable-claude-bridge"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent; Tasks: claudebridge

[Registry]
; AUM registers this per-user notification identity at runtime
; (DisplayName, IconUri); remove it with the application. Not created here.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\{#AppUserModelID}"; Flags: uninsdeletekey dontcreatekey
; Remove only AUM's own Run value; preserve every unrelated startup entry.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "AgentUsageMonitor"; Flags: uninsdeletevalue dontcreatekey

[UninstallDelete]
; Generated by the app for the notification identity; not user data.
Type: files; Name: "{userappdata}\Agent Usage Monitor\notification-icon.png"

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep <> usPostUninstall then
    exit;
  DataDir := ExpandConstant('{userappdata}\Agent Usage Monitor');
  if not DirExists(DataDir) then
    exit;
  { Ask at the end of uninstall; silent uninstalls preserve user data. }
  if UninstallSilent then
    exit;
  if MsgBox(CustomMessage('RemoveSettings'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
  begin
    DeleteFile(DataDir + '\settings.json');
    DeleteFile(DataDir + '\alert-state.json');
    { Remove only the AUM directory if empty; user files keep it present. }
    RemoveDir(DataDir);
  end;
end;

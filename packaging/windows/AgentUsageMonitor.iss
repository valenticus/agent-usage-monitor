; Inno Setup 6 script for AgentUsageMonitor (Windows x64, per-user).
;
; Build with packaging\windows\build-installer.ps1, which compiles the release
; binaries, stages the static AUM icon and passes the version from Cargo.toml.
; Manual build (from this folder):
;   ISCC.exe /DAppVersion=0.3.0 /DStageDir=..\..\rust\target\installer\stage AgentUsageMonitor.iss
;
; Policy for this release:
; - Per-user install under %LOCALAPPDATA%\Programs; no administrator rights.
; - Start menu shortcut; desktop shortcut only when chosen.
; - Optional per-user Windows sign-in launch, disabled by default; no automatic update installation or telemetry.
; - Provider (Codex/Claude) marks are embedded inside agent-usage-monitor.exe
;   by build.rs from the tracked original SVGs in
;   rust/agent-monitor-ui/assets/providers/; no loose logo files are
;   installed. Provenance and distribution approval: docs/design/brand-assets.md.
;   The user's %APPDATA%\Agent Usage Monitor\provider-icons folder is never
;   touched.
; - Settings are kept on upgrade and uninstall unless the user explicitly
;   chooses to remove them during uninstall.
; - The Claude Code status-line bridge stays opt-in and is managed in AUM Settings.
; - Installer version parsing supports strict major.minor.patch versions only;
;   suffixes are rejected safely.

#ifndef AppVersion
  #define AppVersion "0.3.0"
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
#define InstallerMutexName "Local\AgentUsageMonitor.Setup"
#define RepoUrl "https://github.com/valenticus/agent-usage-monitor"
#define UninstallKey "Software\Microsoft\Windows\CurrentVersion\Uninstall\{11666F0A-E4E2-413E-B875-2472A4EFDB84}_is1"
#define MigrationKey "Software\Valenticus\AgentUsageMonitor\InstallerMigration"
#define LegacyMigrationCutoff "0.3.0"

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
; The custom guard waits for a manual tray Exit. Setup never sends window
; messages or asks Restart Manager to close an AUM process.
SetupMutex={#InstallerMutexName}
CloseApplications=no
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
ukrainian.RemoveSettings=Видалити також збережені дані AgentUsageMonitor?
english.RemoveSettings=Also delete saved AgentUsageMonitor data?
polish.RemoveSettings=Czy usunąć również zapisane dane AgentUsageMonitor?
russian.RemoveSettings=Удалить также сохранённые данные AgentUsageMonitor?
ukrainian.VersionCheckFailed=Не вдалося безпечно перевірити встановлену версію. Файли не змінено.
english.VersionCheckFailed=The installed version could not be safely verified. No files were changed.
polish.VersionCheckFailed=Nie można bezpiecznie zweryfikować zainstalowanej wersji. Pliki nie zostały zmienione.
russian.VersionCheckFailed=Не удалось безопасно проверить установленную версию. Файлы не изменены.
ukrainian.InstallPathMismatch=Вибрана папка не відповідає наявному встановленню AgentUsageMonitor. Файли не змінено.
english.InstallPathMismatch=The selected folder does not match the existing AgentUsageMonitor installation. No files were changed.
polish.InstallPathMismatch=Wybrany folder nie odpowiada istniejącej instalacji AgentUsageMonitor. Pliki nie zostały zmienione.
russian.InstallPathMismatch=Выбранная папка не соответствует существующей установке AgentUsageMonitor. Файлы не изменены.
ukrainian.InstallPathOccupied=У вибраній папці є виконуваний файл AUM, але його версію не можна безпечно перевірити. Виберіть іншу папку або використайте сумісний інсталятор.
english.InstallPathOccupied=The selected folder contains an AUM executable whose version cannot be safely verified. Choose another folder or use a compatible installer.
polish.InstallPathOccupied=Wybrany folder zawiera plik AUM, którego wersji nie można bezpiecznie zweryfikować. Wybierz inny folder lub użyj zgodnego instalatora.
russian.InstallPathOccupied=В выбранной папке находится исполняемый файл AUM, версию которого нельзя безопасно проверить. Выберите другую папку или используйте совместимый установщик.
ukrainian.DowngradeBlocked=Встановлена версія %1 новіша за цю версію %2. Пониження версії заблоковано.
english.DowngradeBlocked=Installed version %1 is newer than this version %2. Downgrades are blocked.
polish.DowngradeBlocked=Zainstalowana wersja %1 jest nowsza niż wersja %2. Obniżenie wersji zablokowano.
russian.DowngradeBlocked=Установленная версия %1 новее этой версии %2. Понижение версии запрещено.
ukrainian.RunningBlocker=Запущено Agent Usage Monitor. Цей інсталятор не закриватиме програму. Вийдіть через меню піктограми в треї; встановлення продовжиться автоматично. Якщо запущено Store або Portable версію, скасуйте встановлення, якщо не хочете закривати її.
english.RunningBlocker=An Agent Usage Monitor instance is running. Setup will not close it. Exit from its tray menu; installation will continue automatically. If this is the Store or Portable copy and you do not want to close it, cancel setup.
polish.RunningBlocker=Agent Usage Monitor jest uruchomiony. Instalator go nie zamknie. Zakończ działanie z menu ikony w zasobniku; instalacja będzie kontynuowana automatycznie. Jeśli działa wersja Store lub Portable i nie chcesz jej zamykać, anuluj instalację.
russian.RunningBlocker=Запущен Agent Usage Monitor. Установщик не будет закрывать программу. Выйдите через меню значка в трее; установка продолжится автоматически. Если запущена Store- или Portable-версия и вы не хотите её закрывать, отмените установку.
ukrainian.RunningStillBlocked=Agent Usage Monitor усе ще працює. Вийдіть через меню піктограми в треї, щоб продовжити, або скасуйте встановлення.
english.RunningStillBlocked=Agent Usage Monitor is still running. Exit from its tray menu to continue, or cancel setup.
polish.RunningStillBlocked=Agent Usage Monitor nadal działa. Zakończ działanie z menu ikony w zasobniku, aby kontynuować, lub anuluj instalację.
russian.RunningStillBlocked=Agent Usage Monitor всё ещё работает. Выйдите через меню значка в трее, чтобы продолжить, или отмените установку.
ukrainian.UninstallRunningBlocker=Agent Usage Monitor усе ще працює. Вийдіть через меню піктограми в треї та повторіть видалення. Програма не буде закрита автоматично.
english.UninstallRunningBlocker=Agent Usage Monitor is still running. Exit from its tray menu, then run uninstall again. The app will not be closed automatically.
polish.UninstallRunningBlocker=Agent Usage Monitor nadal działa. Zakończ działanie z menu ikony w zasobniku i ponownie uruchom deinstalator. Aplikacja nie zostanie zamknięta automatycznie.
russian.UninstallRunningBlocker=Agent Usage Monitor всё ещё работает. Выйдите через меню значка в трее и запустите удаление снова. Программа не будет закрыта автоматически.
ukrainian.UninstallRunningWait=Запущено Agent Usage Monitor. Вийдіть через меню піктограми в треї; видалення продовжиться автоматично. Натисніть «Скасувати», щоб перервати видалення.
english.UninstallRunningWait=Agent Usage Monitor is running. Exit from its tray menu; uninstall will continue automatically. Select Cancel to stop uninstalling.
polish.UninstallRunningWait=Agent Usage Monitor jest uruchomiony. Zakończ działanie z menu ikony w zasobniku; deinstalacja będzie kontynuowana automatycznie. Wybierz Anuluj, aby przerwać.
russian.UninstallRunningWait=Запущен Agent Usage Monitor. Выйдите через меню значка в трее; удаление продолжится автоматически. Нажмите «Отмена», чтобы прервать удаление.
ukrainian.UninstallStillBlocked=Agent Usage Monitor усе ще працює. Вийдіть через меню піктограми в треї, щоб продовжити видалення, або скасуйте його.
english.UninstallStillBlocked=Agent Usage Monitor is still running. Exit from its tray menu to continue uninstalling, or cancel uninstall.
polish.UninstallStillBlocked=Agent Usage Monitor nadal działa. Zakończ działanie z menu ikony w zasobniku, aby kontynuować deinstalację, lub ją anuluj.
russian.UninstallStillBlocked=Agent Usage Monitor всё ещё работает. Выйдите через меню значка в трее, чтобы продолжить удаление, или отмените его.
ukrainian.UninstallMutexFailure=Не вдалося перевірити або зарезервувати екземпляр AUM. Видалення скасовано без змін.
english.UninstallMutexFailure=The AUM instance could not be checked or reserved safely. Uninstall was canceled without changes.
polish.UninstallMutexFailure=Nie można było bezpiecznie sprawdzić ani zarezerwować instancji AUM. Deinstalację anulowano bez zmian.
russian.UninstallMutexFailure=Не удалось безопасно проверить или зарезервировать экземпляр AUM. Удаление отменено без изменений.
ukrainian.MutexFailure=Не вдалося безпечно зарезервувати екземпляр AUM (помилка Win32 %1). Файли не змінено.
english.MutexFailure=Setup could not safely reserve the AUM instance (Win32 error %1). No files were changed.
polish.MutexFailure=Instalator nie mógł bezpiecznie zarezerwować instancji AUM (błąd Win32 %1). Pliki nie zostały zmienione.
russian.MutexFailure=Не удалось безопасно зарезервировать экземпляр AUM (ошибка Win32 %1). Файлы не изменены.
ukrainian.MigrationPersistenceFailed=Не вдалося зберегти стан оновлення AUM. Файли не змінено; повторіть встановлення після усунення проблеми з доступом до реєстру.
english.MigrationPersistenceFailed=The AUM migration state could not be saved. No files were changed; resolve the registry access issue and retry setup.
polish.MigrationPersistenceFailed=Nie można zapisać stanu migracji AUM. Pliki nie zostały zmienione; usuń problem z dostępem do rejestru i ponów instalację.
russian.MigrationPersistenceFailed=Не удалось сохранить состояние миграции AUM. Файлы не изменены; устраните проблему доступа к реестру и повторите установку.

[Files]
; Replace the independently invoked helper first. If Claude locks it, setup
; follows Inno's normal file error/retry flow; close the helper and retry.
; Setup never force-closes it or promises replacement at reboot/atomic rollback.
Source: "{#StageDir}\{#BridgeExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
; Same AppUserModelID as the running app, so toasts and the taskbar group
; under the shortcut's name and icon.
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; AppUserModelID: "{#AppUserModelID}"; Check: ShouldCreateStartMenuShortcut

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent; Check: ShouldOfferUpgradeLaunch

[Registry]
; AUM registers this per-user notification identity at runtime
; (DisplayName, IconUri); remove it with the application. Not created here.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\{#AppUserModelID}"; Flags: uninsdeletekey dontcreatekey
; Remove only AUM's own Run value; preserve every unrelated startup entry.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "AgentUsageMonitor"; Flags: uninsdeletevalue dontcreatekey
; Positive-evidence Pending is written before replacement; no-evidence Done
; is written only after a successful install. Existing markers stay terminal.
Root: HKCU; Subkey: "{#MigrationKey}"; Flags: uninsdeletekey dontcreatekey

[UninstallDelete]
; Generated by the app for the notification identity; not user data.
Type: files; Name: "{userappdata}\Agent Usage Monitor\notification-icon.png"

[Code]
var
  PreviousInstallFound: Boolean;
  PreviousInstallIsLegacy: Boolean;
  ExistingLegacyPortable: Boolean;
  MigrationMarkerExists: Boolean;
  LegacyEvidenceCaptured: Boolean;
  LegacyBridgeEnabled: Boolean;
  LegacyStartMenuOwned: Boolean;
  LegacyRunOwned: Boolean;
  MigrationMarkerReady: Boolean;
  PreviousInstallPath: String;
  LegacyPortablePath: String;
  AppMutexHandle: THandle;
  MutexWin32Error: LongWord;
  MutexCheckFailed: Boolean;

function CreateMutexW(Attributes: LongWord; InitialOwner: Boolean; Name: String): THandle;
  external 'CreateMutexW@kernel32.dll stdcall';
function GetLastError(): LongWord;
  external 'GetLastError@kernel32.dll stdcall';
procedure SetLastError(Error: LongWord);
  external 'SetLastError@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Boolean;
  external 'CloseHandle@kernel32.dll stdcall';

function ParseVersion(const S: String; var A, B, C: Integer): Boolean;
var
  I, Part, Value, Digits: Integer;
  Ch: Char;
begin
  Result := False;
  A := 0; B := 0; C := 0;
  Part := 0; Value := 0; Digits := 0;
  for I := 1 to Length(S) do begin
    Ch := S[I];
    if Ch = '.' then begin
      if (Digits = 0) or (Part >= 2) then exit;
      if Part = 0 then A := Value else B := Value;
      Part := Part + 1; Value := 0; Digits := 0;
    end else begin
      if (Ch < '0') or (Ch > '9') then exit;
      if (Value > 214748364) or
         ((Value = 214748364) and (Ord(Ch) - Ord('0') > 7)) then exit;
      Value := Value * 10 + Ord(Ch) - Ord('0');
      Digits := Digits + 1;
    end;
  end;
  if (Part <> 2) or (Digits = 0) then exit;
  C := Value;
  Result := True;
end;

function CompareVersions(const Left, Right: String; var Comparison: Integer): Boolean;
var
  LA, LB, LC, RA, RB, RC: Integer;
begin
  Result := False;
  if not ParseVersion(Left, LA, LB, LC) then exit;
  if not ParseVersion(Right, RA, RB, RC) then exit;
  Comparison := 0;
  if LA < RA then Comparison := -1
  else if LA > RA then Comparison := 1
  else if LB < RB then Comparison := -1
  else if LB > RB then Comparison := 1
  else if LC < RC then Comparison := -1
  else if LC > RC then Comparison := 1;
  Result := True;
end;

function ContainsLegacyTask(const Tasks: String): Boolean;
var
  I, StartAt: Integer;
  Token, Value: String;
begin
  Result := False;
  Value := Tasks + ',';
  StartAt := 1;
  for I := 1 to Length(Value) do
    if Value[I] = ',' then begin
      Token := Trim(Copy(Value, StartAt, I - StartAt));
      if CompareText(Token, 'claudebridge') = 0 then begin
        Result := True;
        exit;
      end;
      StartAt := I + 1;
    end;
end;

function IsPreV030Executable(const ExePath: String): Boolean;
var
  Major, Minor, Revision, Build: Word;
begin
  Result := False;
  if GetVersionComponents(ExePath, Major, Minor, Revision, Build) then
    Result := (Major = 0) and (Minor < 3);
end;

function ShortcutEvidence(const LinkPath, ExpectedExe: String; var HasBridgeFlag: Boolean): Boolean;
var
  Shell, ShellApp, Link, Folder, Item: Variant;
  Target, Args, WorkingDir, AppId: String;
begin
  Result := False;
  HasBridgeFlag := False;
  try
    Shell := CreateOleObject('WScript.Shell');
    Link := Shell.CreateShortcut(LinkPath);
    Target := ExpandFileName(Link.TargetPath);
    if CompareText(Target, ExpandFileName(ExpectedExe)) <> 0 then exit;
    WorkingDir := ExpandFileName(Link.WorkingDirectory);
    if CompareText(WorkingDir, ExtractFileDir(ExpandFileName(ExpectedExe))) <> 0 then exit;
    ShellApp := CreateOleObject('Shell.Application');
    Folder := ShellApp.NameSpace(ExtractFileDir(LinkPath));
    Item := Folder.ParseName(ExtractFileName(LinkPath));
    AppId := Item.ExtendedProperty('System.AppUserModel.ID');
    if CompareText(AppId, '{#AppUserModelID}') <> 0 then exit;
    Args := Link.Arguments;
    Args := Trim(Args);
    if Args <> '' then begin
      if CompareText(Args, '--enable-claude-bridge') <> 0 then exit;
      HasBridgeFlag := True;
    end;
    Result := True;
  except
    Result := False;
  end;
end;

function RunCommandEvidence(const Command, ExpectedExe: String): Boolean;
var
  QuoteEnd, I: Integer;
  Major, Minor, Revision, Build: Word;
  Target, Args: String;
begin
  Result := False;
  if (Length(Command) < 3) or (Command[1] <> '"') then exit;
  QuoteEnd := 0;
  for I := 2 to Length(Command) do
    if Command[I] = '"' then begin QuoteEnd := I; break; end;
  if QuoteEnd = 0 then exit;
  Target := Copy(Command, 2, QuoteEnd - 2);
  if CompareText(ExpandFileName(Target), ExpandFileName(ExpectedExe)) <> 0 then exit;
  if not FileExists(Target) then exit;
  { An available version resource must confirm pre-v0.3.0. Older Rust EXEs may
    lack one; their exact legacy bridge argument is itself explicit evidence. }
  if GetVersionComponents(Target, Major, Minor, Revision, Build) and
     not IsPreV030Executable(Target) then exit;
  Args := Trim(Copy(Command, QuoteEnd + 1, Length(Command)));
  Result := CompareText(Args, '--enable-claude-bridge') = 0;
end;

function SamePath(const Left, Right: String): Boolean;
begin
  Result := CompareText(ExpandFileName(Left), ExpandFileName(Right)) = 0;
end;

function HasMatchingOptInMarker(const ExePath: String; var MarkerState: String): Boolean;
var
  MarkerExe, Bridge: String;
begin
  Result := False;
  MarkerState := '';
  if not RegQueryStringValue(HKCU, '{#MigrationKey}', 'State', MarkerState) or
     ((CompareText(MarkerState, 'Pending') <> 0) and
      (CompareText(MarkerState, 'Done') <> 0)) then exit;
  Result :=
    RegQueryStringValue(HKCU, '{#MigrationKey}', 'InstallExe', MarkerExe) and
    SamePath(MarkerExe, ExePath) and
    RegQueryStringValue(HKCU, '{#MigrationKey}', 'BridgeOptIn', Bridge) and
    (Bridge = '1');
end;

procedure DetectLegacyBridgeChoice;
var
  Tasks, Command, MarkerState: String;
  HasBridgeFlag: Boolean;
begin
  LegacyEvidenceCaptured := False;
  LegacyBridgeEnabled := False;
  LegacyStartMenuOwned := False;
  LegacyRunOwned := False;
  if not PreviousInstallFound then exit;
  if MigrationMarkerExists then begin
    { Pending/Done records with positive prior evidence authorize shortcut
      cleanup only. They never make legacy evidence eligible for migration. }
    if not HasMatchingOptInMarker(PreviousInstallPath + '\{#AppExe}', MarkerState) then exit;
    if ShortcutEvidence(ExpandConstant('{autoprograms}\{#AppName}.lnk'),
        PreviousInstallPath + '\{#AppExe}', HasBridgeFlag) and HasBridgeFlag then
      LegacyStartMenuOwned := True;
    if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run',
        'AgentUsageMonitor', Command) and
        RunCommandEvidence(Command, PreviousInstallPath + '\{#AppExe}') then
      LegacyRunOwned := True;
    exit;
  end;
  if not PreviousInstallIsLegacy then exit;
  if RegQueryStringValue(HKCU, '{#UninstallKey}', 'Inno Setup: Selected Tasks', Tasks) then begin
    if ContainsLegacyTask(Tasks) then begin
      LegacyBridgeEnabled := True;
      LegacyEvidenceCaptured := True;
    end;
  end;
  if ShortcutEvidence(ExpandConstant('{autoprograms}\{#AppName}.lnk'),
      PreviousInstallPath + '\{#AppExe}', HasBridgeFlag) then begin
    LegacyStartMenuOwned := True;
    if HasBridgeFlag then begin
      LegacyBridgeEnabled := True;
      LegacyEvidenceCaptured := True;
    end;
  end;
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run',
      'AgentUsageMonitor', Command) and
      RunCommandEvidence(Command, PreviousInstallPath + '\{#AppExe}') then begin
    LegacyRunOwned := True;
    LegacyBridgeEnabled := True;
    LegacyEvidenceCaptured := True;
  end;
end;

function InitializeSetup(): Boolean;
var
  VersionText, StoredPath: String;
  Comparison: Integer;
begin
  Result := False;
  PreviousInstallFound := RegKeyExists(HKCU, '{#UninstallKey}');
  MigrationMarkerExists := RegKeyExists(HKCU, '{#MigrationKey}');
  PreviousInstallIsLegacy := False;
  ExistingLegacyPortable := False;
  if PreviousInstallFound then begin
    if not RegQueryStringValue(HKCU, '{#UninstallKey}', 'DisplayVersion', VersionText) or
       not CompareVersions(VersionText, '{#AppVersion}', Comparison) then begin
      MsgBox(CustomMessage('VersionCheckFailed'), mbError, MB_OK);
      exit;
    end;
    if Comparison > 0 then begin
      MsgBox(FmtMessage(CustomMessage('DowngradeBlocked'), [VersionText, '{#AppVersion}']), mbError, MB_OK);
      exit;
    end;
    if not RegQueryStringValue(HKCU, '{#UninstallKey}', 'Inno Setup: App Path', StoredPath) then begin
      MsgBox(CustomMessage('VersionCheckFailed'), mbError, MB_OK);
      exit;
    end;
    PreviousInstallPath := RemoveBackslashUnlessRoot(StoredPath);
    if not CompareVersions(VersionText, '{#LegacyMigrationCutoff}', Comparison) then begin
      MsgBox(CustomMessage('VersionCheckFailed'), mbError, MB_OK);
      exit;
    end;
    PreviousInstallIsLegacy := Comparison < 0;
    DetectLegacyBridgeChoice;
  end;
  Result := True;
end;

function IsPre030RunOptIn(const Command: String; var ExePath: String): Boolean;
var
  QuoteEnd, I: Integer;
  Major, Minor, Revision, Build: Word;
  Args: String;
begin
  Result := False;
  ExePath := '';
  if (Length(Command) < 3) or (Command[1] <> '"') then exit;
  QuoteEnd := 0;
  for I := 2 to Length(Command) do
    if Command[I] = '"' then begin QuoteEnd := I; break; end;
  if QuoteEnd = 0 then exit;
  ExePath := Copy(Command, 2, QuoteEnd - 2);
  if (CompareText(ExtractFileName(ExePath), '{#AppExe}') <> 0) or
     not FileExists(ExePath) then exit;
  Args := Trim(Copy(Command, QuoteEnd + 1, Length(Command)));
  if CompareText(Args, '--enable-claude-bridge') <> 0 then exit;
  if GetVersionComponents(ExePath, Major, Minor, Revision, Build) and
     not IsPreV030Executable(ExePath) then exit;
  Result := True;
end;

procedure DetectPortableRunOptIn;
var
  Command: String;
begin
  if PreviousInstallFound or MigrationMarkerExists then exit;
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run',
      'AgentUsageMonitor', Command) and IsPre030RunOptIn(Command, LegacyPortablePath) then begin
    LegacyEvidenceCaptured := True;
    LegacyBridgeEnabled := True;
  end;
end;

function ShouldCreateStartMenuShortcut(): Boolean;
begin
  Result := not FileExists(ExpandConstant('{autoprograms}\{#AppName}.lnk'));
end;

function WriteMigrationMarker: Boolean;
var
  ExistingState, ExistingExe, ExistingBridge, ExePath, State: String;
begin
  Result := False;
  MigrationMarkerReady := False;
  ExePath := ExpandConstant('{app}\{#AppExe}');
  if RegKeyExists(HKCU, '{#MigrationKey}') then begin
    if RegQueryStringValue(HKCU, '{#MigrationKey}', 'State', ExistingState) and
       RegQueryStringValue(HKCU, '{#MigrationKey}', 'InstallExe', ExistingExe) and
       SamePath(ExistingExe, ExePath) then begin
      if CompareText(ExistingState, 'Done') = 0 then begin
        Result := True;
        if RegQueryStringValue(HKCU, '{#MigrationKey}', 'BridgeOptIn', ExistingBridge) and
           (ExistingBridge = '1') then
          MigrationMarkerReady := True;
        exit;
      end;
      if (CompareText(ExistingState, 'Pending') = 0) and
         RegQueryStringValue(HKCU, '{#MigrationKey}', 'BridgeOptIn', ExistingBridge) and
         (ExistingBridge = '1') then begin
        MigrationMarkerReady := True;
        Result := True;
        exit;
      end;
    end;
    { Preserve every existing marker, especially Pending and terminal Done. }
    Result := not LegacyEvidenceCaptured;
    exit;
  end;

  if not (LegacyEvidenceCaptured and LegacyBridgeEnabled) then begin
    { Do not make Done terminal until the install actually succeeds. }
    Result := True;
    exit;
  end;
  State := 'Pending';
  Result := RegWriteStringValue(HKCU, '{#MigrationKey}', 'InstallExe', ExePath);
  if not Result then begin
    RegDeleteKeyIfEmpty(HKCU, '{#MigrationKey}');
    exit;
  end;
  Result := RegWriteStringValue(HKCU, '{#MigrationKey}', 'BridgeOptIn', '1');
  if not Result then begin
    RegDeleteValue(HKCU, '{#MigrationKey}', 'InstallExe');
    RegDeleteKeyIfEmpty(HKCU, '{#MigrationKey}');
    exit;
  end;
  { State is written last, so a partial write is never consumable. }
  Result := RegWriteStringValue(HKCU, '{#MigrationKey}', 'State', State);
  if Result then
    MigrationMarkerReady := State = 'Pending'
  else begin
    RegDeleteValue(HKCU, '{#MigrationKey}', 'BridgeOptIn');
    RegDeleteValue(HKCU, '{#MigrationKey}', 'InstallExe');
    RegDeleteKeyIfEmpty(HKCU, '{#MigrationKey}');
  end;
end;

procedure RefreshOwnedStartMenuShortcut;
var
  Shell, Link: Variant;
  LinkPath: String;
begin
  if not LegacyStartMenuOwned then exit;
  LinkPath := ExpandConstant('{autoprograms}\{#AppName}.lnk');
  try
    Shell := CreateOleObject('WScript.Shell');
    Link := Shell.CreateShortcut(LinkPath);
    Link.TargetPath := ExpandConstant('{app}\{#AppExe}');
    Link.Arguments := '';
    Link.WorkingDirectory := ExpandConstant('{app}');
    Link.Save;
  except
    Log('Could not refresh the verified AUM Start Menu shortcut; it was left unchanged.');
  end;
end;

procedure RemoveMigratedRunFlag;
var
  Command: String;
begin
  if not (LegacyRunOwned and MigrationMarkerReady) then exit;
  Command := '"' + ExpandConstant('{app}\{#AppExe}') + '"';
  if not RegWriteStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run',
      'AgentUsageMonitor', Command) then
    Log('Could not remove the migrated Claude bridge argument from the AUM Run value.');
end;

function ShouldOfferUpgradeLaunch(): Boolean;
begin
  Result := PreviousInstallFound or ExistingLegacyPortable;
end;

function TryReserveAumMutex(): Boolean;
begin
  Result := False;
  MutexWin32Error := 0;
  if CheckForMutexes('{#AppMutexName}') then exit;
  SetLastError(0);
  AppMutexHandle := CreateMutexW(0, False, '{#AppMutexName}');
  MutexWin32Error := GetLastError();
  if AppMutexHandle = 0 then exit;
  if MutexWin32Error = 183 then begin
    CloseHandle(AppMutexHandle);
    AppMutexHandle := 0;
    exit;
  end;
  Result := True;
end;

function WaitForAumReservation(AllowWait, UninstallContext: Boolean; var ErrorText: String): Boolean;
var
  Asked, StillRunning: Boolean;
  PromptText: String;
  WaitTicks: Integer;
begin
  Result := False;
  MutexCheckFailed := False;
  Asked := False;
  WaitTicks := 0;
  while True do begin
    if TryReserveAumMutex() then begin
      Result := True;
      exit;
    end;
    StillRunning := CheckForMutexes('{#AppMutexName}');
    if not StillRunning then begin
      MutexCheckFailed := True;
      ErrorText := FmtMessage(CustomMessage('MutexFailure'), [IntToStr(MutexWin32Error)]);
      exit;
    end;
    if not AllowWait then begin
      ErrorText := CustomMessage('RunningBlocker');
      exit;
    end;
    if not Asked then begin
      if UninstallContext then
        PromptText := CustomMessage('UninstallRunningWait')
      else
        PromptText := CustomMessage('RunningBlocker');
      if MsgBox(PromptText, mbConfirmation, MB_OKCANCEL) <> IDOK then begin
        if UninstallContext then
          ErrorText := CustomMessage('UninstallRunningBlocker')
        else
          ErrorText := CustomMessage('RunningBlocker');
        exit;
      end;
      Asked := True;
    end;
    { InitializeUninstall runs before UninstallProgressForm exists. Keep the
      uninstall preflight independent of both forms; Setup may repaint its wizard. }
    if not UninstallContext then WizardForm.Update;
    Sleep(500);
    WaitTicks := WaitTicks + 1;
    if WaitTicks >= 20 then begin
      if UninstallContext then
        PromptText := CustomMessage('UninstallStillBlocked')
      else
        PromptText := CustomMessage('RunningStillBlocked');
      if MsgBox(PromptText, mbConfirmation, MB_OKCANCEL) <> IDOK then begin
        if UninstallContext then
          ErrorText := CustomMessage('UninstallRunningBlocker')
        else
          ErrorText := CustomMessage('RunningBlocker');
        exit;
      end;
      WaitTicks := 0;
    end;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ErrorText, AppExePath: String;
begin
  Result := '';
  if PreviousInstallFound and
     not SamePath(PreviousInstallPath, ExpandConstant('{app}')) then begin
    Result := CustomMessage('InstallPathMismatch');
    exit;
  end;
  AppExePath := ExpandConstant('{app}\{#AppExe}');
  if not PreviousInstallFound then begin
    DetectPortableRunOptIn;
    if FileExists(AppExePath) then begin
      if LegacyEvidenceCaptured and SamePath(LegacyPortablePath, AppExePath) then begin
        ExistingLegacyPortable := True;
        LegacyRunOwned := True;
      end else begin
        Result := CustomMessage('InstallPathOccupied');
        exit;
      end;
    end else begin
      { A Portable instance in another folder is separate; don't migrate or edit it. }
      LegacyEvidenceCaptured := False;
      LegacyBridgeEnabled := False;
      LegacyPortablePath := '';
    end;
  end;
  if not WaitForAumReservation(not WizardSilent, False, ErrorText) then begin
    Result := ErrorText;
    exit;
  end;
  if not WriteMigrationMarker then begin
    if AppMutexHandle <> 0 then begin
      CloseHandle(AppMutexHandle);
      AppMutexHandle := 0;
    end;
    Result := CustomMessage('MigrationPersistenceFailed');
  end;
end;

function InitializeUninstall(): Boolean;
var
  ErrorText: String;
begin
  Result := WaitForAumReservation(not UninstallSilent, True, ErrorText);
  if (not Result) and (not UninstallSilent) then begin
    if MutexCheckFailed then
      MsgBox(CustomMessage('UninstallMutexFailure'), mbError, MB_OK)
    else
      MsgBox(CustomMessage('UninstallRunningBlocker'), mbError, MB_OK);
  end;
end;

function WriteCompletedMigrationMarker: Boolean;
var
  ExePath: String;
begin
  Result := True;
  if RegKeyExists(HKCU, '{#MigrationKey}') then exit;
  ExePath := ExpandConstant('{app}\{#AppExe}');
  Result := RegWriteStringValue(HKCU, '{#MigrationKey}', 'InstallExe', ExePath);
  if not Result then begin
    RegDeleteKeyIfEmpty(HKCU, '{#MigrationKey}');
    Log('Could not record completed migration state after successful installation.');
    exit;
  end;
  Result := RegWriteStringValue(HKCU, '{#MigrationKey}', 'State', 'Done');
  if not Result then begin
    RegDeleteValue(HKCU, '{#MigrationKey}', 'InstallExe');
    RegDeleteKeyIfEmpty(HKCU, '{#MigrationKey}');
    Log('Could not record completed migration state after successful installation.');
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
begin
  if CurStep <> ssPostInstall then exit;
  { With no positive evidence, write terminal Done only after installation
    completed. A failed or canceled setup can therefore retry migration. }
  WriteCompletedMigrationMarker;
  { Positive-evidence markers were persisted during PrepareToInstall. }
  if MigrationMarkerReady then begin
    RefreshOwnedStartMenuShortcut;
    RemoveMigratedRunFlag;
  end;
  { Files and registry are complete. Release the launch guard before any app start. }
  if AppMutexHandle <> 0 then begin
    CloseHandle(AppMutexHandle);
    AppMutexHandle := 0;
  end;
  if not PreviousInstallFound and not ExistingLegacyPortable and not WizardSilent then
    Exec(ExpandConstant('{app}\{#AppExe}'), '', ExpandConstant('{app}'), SW_SHOWNORMAL, ewNoWait, ResultCode);
end;

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

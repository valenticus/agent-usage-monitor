//! Per-user Windows sign-in launch registration.
//!
//! Unpackaged (Inno Setup, portable): AUM owns exactly one value under HKCU's
//! Run key. The command points at the current executable, so installed and
//! portable copies use the same path. These functions are synchronous: they
//! are short registry calls, unchanged from the EXE releases.
//!
//! Packaged (MSIX / Microsoft Store): HKCU writes land in the package's private
//! registry and Windows never reads them for sign-in launch, so the package's
//! `windows.startupTask` (see `packaged::STARTUP_TASK_ID`) is used instead,
//! through [`StartupController`]. Every StartupTask call runs on a worker
//! thread; the UI thread only submits a request and later receives the result,
//! so it never waits on a WinRT async operation. The user can turn the task off
//! in Windows Settings or Task Manager; such a state cannot be overridden by the
//! app and is reported, not fought.
//!
//! When package identity could not be determined (`PackageState::Unknown`),
//! nothing is written: the HKCU `Run` value may only be changed by a process
//! that is confirmed to be unpackaged.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "AgentUsageMonitor";

/// Configure the AUM-owned HKCU `Run` value (unpackaged builds only). The
/// value is only updated/deleted by name; no other Run entries are enumerated
/// or modified. Packaged builds use [`packaged_controller`] instead.
#[cfg(windows)]
pub fn set_enabled(
    enabled: bool,
    executable: &std::path::Path,
    claude_bridge: bool,
) -> Result<(), String> {
    match crate::packaged::state() {
        crate::packaged::PackageState::Unpackaged => {}
        crate::packaged::PackageState::Packaged => {
            return Err("the packaged build uses its startup task, not the Run key".into());
        }
        crate::packaged::PackageState::Unknown => {
            return Err("package identity is unknown; startup registration was not changed".into());
        }
    }
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
        RegSetValueExW,
    };

    let mut key = std::ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
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
        return Err(format!("could not open per-user startup key ({result})"));
    }

    let outcome = if enabled {
        let command = startup_command(executable, claude_bridge);
        let mut data: Vec<u16> = std::ffi::OsStr::new(&command).encode_wide().collect();
        data.push(0);
        unsafe {
            RegSetValueExW(
                key,
                wide(VALUE_NAME).as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * std::mem::size_of::<u16>()) as u32,
            )
        }
    } else {
        let result = unsafe { RegDeleteValueW(key, wide(VALUE_NAME).as_ptr()) };
        // Deleting an absent AUM-owned entry is already the requested state.
        if result == 2 { 0 } else { result }
    };
    unsafe { RegCloseKey(key) };
    if outcome == 0 {
        Ok(())
    } else {
        Err(format!(
            "could not update per-user startup entry ({outcome})"
        ))
    }
}

/// Return whether the exact AUM Run value targets this executable. A stale
/// path is reported as disabled until reconciliation succeeds. Read-only; not
/// used by the packaged build (its state comes from [`packaged_controller`]).
#[cfg(windows)]
pub fn is_enabled_for(executable: &std::path::Path) -> Result<bool, String> {
    if crate::packaged::is_packaged() {
        return Err("the packaged build reads its startup task asynchronously".into());
    }
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_QUERY_VALUE, REG_SZ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
    };

    let mut key = std::ptr::null_mut();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if result == 2 {
        return Ok(false);
    }
    if result != 0 {
        return Err(format!("could not read per-user startup key ({result})"));
    }

    let mut kind = 0;
    let mut bytes = 0u32;
    let mut query = unsafe {
        RegQueryValueExW(
            key,
            wide(VALUE_NAME).as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if query == 2 {
        unsafe { RegCloseKey(key) };
        return Ok(false);
    }
    if query != 0 || kind != REG_SZ || bytes < 2 || bytes % 2 != 0 {
        unsafe { RegCloseKey(key) };
        return if query == 0 {
            Ok(false)
        } else {
            Err(format!("could not read per-user startup entry ({query})"))
        };
    }
    let mut data = vec![0u16; (bytes / 2) as usize];
    query = unsafe {
        RegQueryValueExW(
            key,
            wide(VALUE_NAME).as_ptr(),
            std::ptr::null(),
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    unsafe { RegCloseKey(key) };
    if query != 0 || kind != REG_SZ {
        return Err(format!("could not read per-user startup entry ({query})"));
    }
    let length = data
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(data.len());
    let command = String::from_utf16_lossy(&data[..length]);
    Ok(command_matches_executable(&command, executable))
}

/// Who decides whether AUM launches at sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupControl {
    /// AUM's own toggle decides (HKCU `Run`, or a packaged task that is
    /// simply enabled or disabled).
    App,
    /// MSIX only: the user turned the startup task off in Windows (Settings >
    /// Apps > Startup or Task Manager). Only the user can turn it back on there.
    DisabledInWindows,
    /// MSIX only: policy fixes the startup task on or off (or the device does
    /// not support startup tasks).
    Policy,
}

/// Actual sign-in launch state, read from one source of truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartupStatus {
    pub enabled: bool,
    pub control: StartupControl,
}

pub type StartupResult = Result<StartupStatus, String>;

/// Reads the HKCU `Run` state of an unpackaged build (synchronous registry read).
pub fn status_for(executable: &std::path::Path) -> StartupResult {
    is_enabled_for(executable).map(|enabled| StartupStatus {
        enabled,
        control: StartupControl::App,
    })
}

/// What a change request must do, given the actual current state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangePlan {
    /// Already in the requested state.
    Keep,
    /// Windows (the user or policy) owns the state; the app must not change it.
    Locked,
    Enable,
    Disable,
}

pub fn plan_change(current: StartupStatus, want: bool) -> ChangePlan {
    if current.enabled == want {
        ChangePlan::Keep
    } else if current.control != StartupControl::App {
        ChangePlan::Locked
    } else if want {
        ChangePlan::Enable
    } else {
        ChangePlan::Disable
    }
}

/// A request to the packaged startup controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupRequest {
    /// Read the actual state (launch, Settings opened). Never changes it.
    Refresh,
    /// The user's toggle: ask for this state. Locked states are left alone.
    Set(bool),
}

/// The request a click on the sign-in toggle submits. Without a verified
/// current state the click only re-reads the state, so a stale or unknown UI
/// can never flip the task blindly.
pub fn toggle_request(last: Option<&StartupResult>) -> StartupRequest {
    match last {
        Some(Ok(status)) => StartupRequest::Set(!status.enabled),
        _ => StartupRequest::Refresh,
    }
}

/// Blocking access to the startup mechanism. Called only on the controller's
/// worker thread.
pub trait StartupBackend: Send + Sync + 'static {
    fn query(&self) -> StartupResult;
    /// Applies [`plan_change`] against a fresh read and returns the actual
    /// state afterwards (which can differ from the request, e.g. when locked).
    fn set(&self, enabled: bool) -> StartupResult;
}

type Listener = Box<dyn Fn(StartupRequest, &StartupResult) + Send + Sync>;

struct ControllerState {
    busy: AtomicBool,
    last: Mutex<Option<StartupResult>>,
    listener: OnceLock<Listener>,
}

/// Runs startup requests one at a time on a short-lived worker thread.
///
/// * `submit` never blocks; it returns false while a request is in flight, so
///   repeated clicks cannot start concurrent or duplicate enable requests.
/// * The result is stored (`last`) and then handed to the listener on the
///   worker thread; the listener posts it to the UI event loop.
/// * A panicking backend is reported as an error and never leaves the
///   controller stuck busy.
pub struct StartupController {
    backend: Arc<dyn StartupBackend>,
    state: Arc<ControllerState>,
}

impl StartupController {
    pub fn new(backend: Arc<dyn StartupBackend>) -> Self {
        Self {
            backend,
            state: Arc::new(ControllerState {
                busy: AtomicBool::new(false),
                last: Mutex::new(None),
                listener: OnceLock::new(),
            }),
        }
    }

    /// Installs the completion listener (once; later calls are ignored).
    pub fn set_listener(
        &self,
        listener: impl Fn(StartupRequest, &StartupResult) + Send + Sync + 'static,
    ) {
        let _ = self.state.listener.set(Box::new(listener));
    }

    pub fn busy(&self) -> bool {
        self.state.busy.load(Ordering::Acquire)
    }

    /// The most recent completed result, if any.
    pub fn last(&self) -> Option<StartupResult> {
        self.state.last.lock().ok().and_then(|last| last.clone())
    }

    /// Starts `request` unless another one is still running.
    pub fn submit(&self, request: StartupRequest) -> bool {
        if self
            .state
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let backend = self.backend.clone();
        let state = self.state.clone();
        let spawned = std::thread::Builder::new()
            .name("aum-startup-task".into())
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match request {
                        StartupRequest::Refresh => backend.query(),
                        StartupRequest::Set(enabled) => backend.set(enabled),
                    }))
                    .unwrap_or_else(|_| Err("startup task worker panicked".into()));
                finish(&state, request, result);
            });
        if let Err(error) = spawned {
            finish(
                &self.state,
                request,
                Err(format!("startup task worker: {error}")),
            );
        }
        true
    }
}

fn finish(state: &ControllerState, request: StartupRequest, result: StartupResult) {
    if let Ok(mut last) = state.last.lock() {
        *last = Some(result.clone());
    }
    state.busy.store(false, Ordering::Release);
    if let Some(listener) = state.listener.get() {
        listener(request, &result);
    }
}

/// The packaged build's startup controller (StartupTask backend).
pub fn packaged_controller() -> &'static StartupController {
    static CONTROLLER: OnceLock<StartupController> = OnceLock::new();
    CONTROLLER.get_or_init(|| StartupController::new(Arc::new(PackagedBackend)))
}

struct PackagedBackend;

impl StartupBackend for PackagedBackend {
    fn query(&self) -> StartupResult {
        #[cfg(windows)]
        {
            startup_task::query()
        }
        #[cfg(not(windows))]
        {
            Err("startup tasks require Windows".into())
        }
    }

    fn set(&self, enabled: bool) -> StartupResult {
        #[cfg(windows)]
        {
            startup_task::set(enabled)
        }
        #[cfg(not(windows))]
        {
            let _ = enabled;
            Err("startup tasks require Windows".into())
        }
    }
}

/// MSIX `StartupTask` access, called only on the controller's worker thread.
///
/// Microsoft documents a UI-thread requirement for `RequestEnableAsync` only
/// for UWP apps (it shows a consent dialog there). For a packaged desktop app
/// no dialog is shown and the class is agile (`MarshalingType.Agile`,
/// `ThreadingModel.Both`). Each call first initializes the worker thread
/// explicitly into the multithreaded apartment (`winrt::Apartment`,
/// `RoInitialize(RO_INIT_MULTITHREADED)`, balanced on return), where blocking
/// on the async result is allowed; every WinRT object is released before the
/// apartment guard. The Slint UI thread (STA) is never blocked or touched.
#[cfg(windows)]
mod startup_task {
    use super::{ChangePlan, StartupResult, plan_change};
    use windows::ApplicationModel::{StartupTask, StartupTaskState};
    use windows::core::HSTRING;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum State {
        Disabled,
        DisabledByUser,
        Enabled,
        DisabledByPolicy,
        EnabledByPolicy,
    }

    impl State {
        pub fn from_winrt(state: StartupTaskState) -> Result<Self, String> {
            Ok(match state {
                StartupTaskState::Disabled => State::Disabled,
                StartupTaskState::DisabledByUser => State::DisabledByUser,
                StartupTaskState::Enabled => State::Enabled,
                StartupTaskState::DisabledByPolicy => State::DisabledByPolicy,
                StartupTaskState::EnabledByPolicy => State::EnabledByPolicy,
                other => return Err(format!("unknown startup task state {}", other.0)),
            })
        }

        pub fn is_enabled(self) -> bool {
            matches!(self, State::Enabled | State::EnabledByPolicy)
        }

        pub fn control(self) -> super::StartupControl {
            match self {
                State::Disabled | State::Enabled => super::StartupControl::App,
                State::DisabledByUser => super::StartupControl::DisabledInWindows,
                State::DisabledByPolicy | State::EnabledByPolicy => super::StartupControl::Policy,
            }
        }

        pub fn status(self) -> super::StartupStatus {
            super::StartupStatus {
                enabled: self.is_enabled(),
                control: self.control(),
            }
        }
    }

    fn task() -> Result<StartupTask, String> {
        StartupTask::GetAsync(&HSTRING::from(crate::packaged::STARTUP_TASK_ID))
            .and_then(|operation| operation.join())
            .map_err(|error| format!("StartupTask.GetAsync: {error}"))
    }

    fn state_of(task: &StartupTask) -> Result<State, String> {
        State::from_winrt(
            task.State()
                .map_err(|error| format!("StartupTask.State: {error}"))?,
        )
    }

    pub fn query() -> StartupResult {
        // Declared first, dropped last: after every WinRT object below.
        let _apartment = crate::winrt::Apartment::require_multithreaded()?;
        state_of(&task()?).map(State::status)
    }

    pub fn set(enabled: bool) -> StartupResult {
        let _apartment = crate::winrt::Apartment::require_multithreaded()?;
        let task = task()?;
        let current = state_of(&task)?;
        match plan_change(current.status(), enabled) {
            ChangePlan::Keep | ChangePlan::Locked => Ok(current.status()),
            ChangePlan::Enable => {
                // Returns the resulting state; DisabledByUser / policy states
                // are reported, never overridden.
                let result = task
                    .RequestEnableAsync()
                    .and_then(|operation| operation.join())
                    .map_err(|error| format!("StartupTask.RequestEnableAsync: {error}"))?;
                State::from_winrt(result).map(State::status)
            }
            ChangePlan::Disable => {
                task.Disable()
                    .map_err(|error| format!("StartupTask.Disable: {error}"))?;
                state_of(&task).map(State::status)
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::{StartupControl, StartupStatus};
        use super::*;

        #[test]
        fn startup_task_states_map_to_enabled_and_windows_controlled() {
            use StartupControl::{App, DisabledInWindows, Policy};
            let cases = [
                (StartupTaskState::Disabled, State::Disabled, false, App),
                (
                    StartupTaskState::DisabledByUser,
                    State::DisabledByUser,
                    false,
                    DisabledInWindows,
                ),
                (StartupTaskState::Enabled, State::Enabled, true, App),
                (
                    StartupTaskState::DisabledByPolicy,
                    State::DisabledByPolicy,
                    false,
                    Policy,
                ),
                (
                    StartupTaskState::EnabledByPolicy,
                    State::EnabledByPolicy,
                    true,
                    Policy,
                ),
            ];
            for (winrt, expected, enabled, control) in cases {
                let state = State::from_winrt(winrt).unwrap();
                assert_eq!(state, expected);
                assert_eq!(state.is_enabled(), enabled, "{state:?}");
                assert_eq!(state.control(), control, "{state:?}");
                assert_eq!(state.status(), StartupStatus { enabled, control });
            }
            assert!(State::from_winrt(StartupTaskState(99)).is_err());
        }
    }
}

#[cfg(not(windows))]
pub fn set_enabled(
    _enabled: bool,
    _executable: &std::path::Path,
    _claude_bridge: bool,
) -> Result<(), String> {
    Err("Windows sign-in startup is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn is_enabled_for(_executable: &std::path::Path) -> Result<bool, String> {
    Ok(false)
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn quote_executable(executable: &std::path::Path) -> String {
    format!("\"{}\"", executable.to_string_lossy())
}

fn startup_command(executable: &std::path::Path, claude_bridge: bool) -> String {
    let mut command = quote_executable(executable);
    if claude_bridge {
        command.push_str(" --enable-claude-bridge");
    }
    command
}

fn command_matches_executable(command: &str, executable: &std::path::Path) -> bool {
    let Some((path, arguments)) = command
        .strip_prefix('"')
        .and_then(|value| value.split_once('"'))
    else {
        return false;
    };
    let expected_path = executable.to_string_lossy();
    let path_matches = path.eq_ignore_ascii_case(&expected_path);
    let arguments_match = arguments.is_empty() || arguments == " --enable-claude-bridge";
    path_matches && arguments_match
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn startup_command_quotes_paths_with_spaces_and_unicode() {
        assert_eq!(
            startup_command(
                Path::new(r"C:\Users\Éva Kovács\AUM\agent-usage-monitor.exe"),
                false
            ),
            r#""C:\Users\Éva Kovács\AUM\agent-usage-monitor.exe""#
        );
        assert_eq!(
            startup_command(Path::new(r"C:\AUM\agent-usage-monitor.exe"), true),
            r#""C:\AUM\agent-usage-monitor.exe" --enable-claude-bridge"#
        );
    }

    #[test]
    fn only_exact_current_executable_is_reported_enabled() {
        let exe = Path::new(r"C:\Portable AUM\agent-usage-monitor.exe");
        assert!(command_matches_executable(
            r#""C:\Portable AUM\agent-usage-monitor.exe""#,
            exe
        ));
        assert!(command_matches_executable(
            r#""c:\portable aum\AGENT-USAGE-MONITOR.EXE""#,
            exe
        ));
        assert!(command_matches_executable(
            r#""C:\Portable AUM\agent-usage-monitor.exe" --enable-claude-bridge"#,
            exe
        ));
        assert!(!command_matches_executable(
            r#""C:\Old AUM\agent-usage-monitor.exe""#,
            exe
        ));
        assert!(!command_matches_executable(
            r#""C:\Portable AUM\agent-usage-monitor.exe" --demo"#,
            exe
        ));
        assert!(!command_matches_executable("not quoted", exe));
    }

    #[test]
    fn test_processes_never_touch_the_run_key_through_the_packaged_path() {
        // `cargo test` is unpackaged; the packaged controller is a separate
        // path and plain Run-key reads stay available.
        assert!(!crate::packaged::is_packaged());
    }

    fn status(enabled: bool, control: StartupControl) -> StartupStatus {
        StartupStatus { enabled, control }
    }

    #[test]
    fn change_plan_never_overrides_windows_or_policy() {
        use StartupControl::{App, DisabledInWindows, Policy};
        assert_eq!(plan_change(status(false, App), true), ChangePlan::Enable);
        assert_eq!(plan_change(status(true, App), false), ChangePlan::Disable);
        assert_eq!(plan_change(status(true, App), true), ChangePlan::Keep);
        assert_eq!(plan_change(status(false, App), false), ChangePlan::Keep);
        // Disabled by the user in Windows: only the user can re-enable it there.
        assert_eq!(
            plan_change(status(false, DisabledInWindows), true),
            ChangePlan::Locked
        );
        assert_eq!(
            plan_change(status(false, DisabledInWindows), false),
            ChangePlan::Keep
        );
        // Policy, in either direction.
        assert_eq!(plan_change(status(false, Policy), true), ChangePlan::Locked);
        assert_eq!(plan_change(status(true, Policy), false), ChangePlan::Locked);
        assert_eq!(plan_change(status(true, Policy), true), ChangePlan::Keep);
    }

    #[test]
    fn toggle_without_a_verified_state_only_refreshes() {
        assert_eq!(toggle_request(None), StartupRequest::Refresh);
        assert_eq!(
            toggle_request(Some(&Err("unavailable".into()))),
            StartupRequest::Refresh
        );
        assert_eq!(
            toggle_request(Some(&Ok(status(false, StartupControl::App)))),
            StartupRequest::Set(true)
        );
        assert_eq!(
            toggle_request(Some(&Ok(status(true, StartupControl::App)))),
            StartupRequest::Set(false)
        );
    }

    /// Backend whose calls block until the test releases them.
    struct GatedBackend {
        gate: Mutex<mpsc::Receiver<StartupResult>>,
        calls: Mutex<Vec<StartupRequest>>,
    }

    impl GatedBackend {
        fn next(&self, request: StartupRequest) -> StartupResult {
            self.calls.lock().unwrap().push(request);
            self.gate
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| Err("test gate timed out".into()))
        }
    }

    impl StartupBackend for GatedBackend {
        fn query(&self) -> StartupResult {
            self.next(StartupRequest::Refresh)
        }
        fn set(&self, enabled: bool) -> StartupResult {
            self.next(StartupRequest::Set(enabled))
        }
    }

    /// Controller, its backend, the gate that releases backend calls, and the
    /// listener's completions.
    type Gated = (
        StartupController,
        Arc<GatedBackend>,
        mpsc::Sender<StartupResult>,
        mpsc::Receiver<(StartupRequest, StartupResult)>,
    );

    fn gated_controller() -> Gated {
        let (release, gate) = mpsc::channel();
        let backend = Arc::new(GatedBackend {
            gate: Mutex::new(gate),
            calls: Mutex::new(Vec::new()),
        });
        let controller = StartupController::new(backend.clone());
        let (done_sender, done) = mpsc::channel();
        let done_sender = Mutex::new(done_sender);
        controller.set_listener(move |request, result| {
            let _ = done_sender.lock().unwrap().send((request, result.clone()));
        });
        (controller, backend, release, done)
    }

    #[test]
    fn controller_runs_one_request_at_a_time_and_reports_the_result() {
        let (controller, backend, release, done) = gated_controller();
        assert!(controller.last().is_none());
        assert!(controller.submit(StartupRequest::Set(true)));
        assert!(controller.busy());
        // A second click while the first request runs is rejected, never queued
        // as a concurrent enable request.
        assert!(!controller.submit(StartupRequest::Set(true)));
        assert!(!controller.submit(StartupRequest::Refresh));
        release.send(Ok(status(true, StartupControl::App))).unwrap();
        let (request, result) = done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(request, StartupRequest::Set(true));
        assert_eq!(result, Ok(status(true, StartupControl::App)));
        assert!(!controller.busy());
        assert_eq!(
            controller.last(),
            Some(Ok(status(true, StartupControl::App)))
        );
        assert_eq!(
            *backend.calls.lock().unwrap(),
            vec![StartupRequest::Set(true)]
        );
        // The next request is accepted once the first completed.
        assert!(controller.submit(StartupRequest::Refresh));
        release
            .send(Ok(status(false, StartupControl::DisabledInWindows)))
            .unwrap();
        let (request, result) = done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(request, StartupRequest::Refresh);
        assert_eq!(result, Ok(status(false, StartupControl::DisabledInWindows)));
    }

    #[test]
    fn controller_reports_failures_and_recovers() {
        let (controller, _backend, release, done) = gated_controller();
        assert!(controller.submit(StartupRequest::Set(true)));
        release.send(Err("access denied".into())).unwrap();
        let (_, result) = done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(result, Err("access denied".into()));
        assert_eq!(controller.last(), Some(Err("access denied".into())));
        assert!(!controller.busy());
    }

    struct PanickingBackend;
    impl StartupBackend for PanickingBackend {
        fn query(&self) -> StartupResult {
            panic!("backend failure")
        }
        fn set(&self, _enabled: bool) -> StartupResult {
            panic!("backend failure")
        }
    }

    #[test]
    fn panicking_backend_never_leaves_the_controller_busy() {
        let controller = StartupController::new(Arc::new(PanickingBackend));
        let (sender, done) = mpsc::channel();
        let sender = Mutex::new(sender);
        controller.set_listener(move |_, result| {
            let _ = sender.lock().unwrap().send(result.clone());
        });
        assert!(controller.submit(StartupRequest::Refresh));
        assert!(done.recv_timeout(Duration::from_secs(5)).unwrap().is_err());
        assert!(!controller.busy());
        assert!(controller.submit(StartupRequest::Refresh));
        assert!(done.recv_timeout(Duration::from_secs(5)).unwrap().is_err());
    }
}

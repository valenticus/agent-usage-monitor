#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// Cursor rests on the tray icon: show the compact preview.
    Hover,
    /// Cursor moved over the tray icon (hover candidate; shown after a short
    /// rest, independent of the shell's own hover notification).
    Pointer,
    /// Double-click or menu "Open": show the full, pinned flyout.
    OpenFull,
    /// Menu "Refresh": real refresh plus a temporary preview.
    Refresh,
    OpenSettings,
    Quit,
    /// Right-click (or keyboard menu key) on the tray icon: open the custom
    /// context menu at this point (physical screen pixels).
    ContextMenu {
        x: i32,
        y: i32,
    },
    /// Windows resumed from sleep/hibernation: refresh quotas in the background.
    Resumed,
}

/// Tray-icon rectangle in physical screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl IconRect {
    pub fn contains(&self, x: i32, y: i32, slack: i32) -> bool {
        x >= self.left - slack
            && x < self.right + slack
            && y >= self.top - slack
            && y < self.bottom + slack
    }
}

/// Updates the language of the tray context menu after a settings change.
pub fn set_locale(locale: crate::localization::Locale) {
    #[cfg(windows)]
    TRAY_LOCALE.store(locale_code(locale), std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(windows))]
    let _ = locale;
}

#[cfg(windows)]
fn locale_code(locale: crate::localization::Locale) -> u8 {
    match locale {
        crate::localization::Locale::Uk => 0,
        crate::localization::Locale::En => 1,
        crate::localization::Locale::Pl => 2,
        crate::localization::Locale::Ru => 3,
    }
}

/// Context-menu labels, strictly in this order: Open, Refresh, Settings, Exit.
pub fn menu_labels(locale: crate::localization::Locale) -> [&'static str; 4] {
    match locale {
        crate::localization::Locale::Uk => ["Відкрити", "Оновити ліміти", "Налаштування", "Вихід"],
        crate::localization::Locale::En => ["Open", "Refresh limits", "Settings", "Exit"],
        crate::localization::Locale::Pl => ["Otwórz", "Odśwież limity", "Ustawienia", "Zakończ"],
        crate::localization::Locale::Ru => ["Открыть", "Обновить лимиты", "Настройки", "Выход"],
    }
}

#[derive(Clone)]
pub struct TrayNotifier {
    #[cfg(windows)]
    window: usize,
    #[cfg(windows)]
    icon: usize,
}

impl TrayNotifier {
    /// Renders each provider's constrained quota locally and asks the tray
    /// window thread to replace its Windows notification-area icon.
    pub fn update_icon(
        &self,
        values: [QuotaValue; 2],
        dark_taskbar: bool,
        warning_threshold: u8,
        critical_threshold: u8,
    ) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;
            let key = (values, dark_taskbar, warning_threshold, critical_threshold);
            let state = TRAY_ICON_STATE.get_or_init(|| std::sync::Mutex::new(None));
            if let Ok(mut last) = state.lock() {
                if last.as_ref() == Some(&key) {
                    return;
                }
                *last = Some(key);
            }
            let Some(pixels) = render_rgba_with_thresholds(
                16,
                values,
                dark_taskbar,
                warning_threshold,
                critical_threshold,
            ) else {
                return;
            };
            let pending = TRAY_ICON_PIXELS.get_or_init(|| std::sync::Mutex::new(None));
            if let Ok(mut pending) = pending.lock() {
                *pending = Some((16, pixels));
            }
            if unsafe { PostMessageW(self.window as _, TRAY_ICON_UPDATE, 0, 0) } == 0 {
                eprintln!("TRAY_ICON: could not schedule the Windows icon update");
                if let Ok(mut last) = state.lock() {
                    *last = None;
                }
            }
        }
        #[cfg(not(windows))]
        let _ = (values, dark_taskbar, warning_threshold, critical_threshold);
    }

    /// Current tray-icon rectangle, when the shell reports one.
    pub fn icon_rect(&self) -> Option<IconRect> {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::Foundation::RECT;
            use windows_sys::Win32::UI::Shell::{NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect};
            let identifier = NOTIFYICONIDENTIFIER {
                cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: self.window as _,
                uID: 1,
                ..Default::default()
            };
            let mut rect = RECT::default();
            if Shell_NotifyIconGetRect(&identifier, &mut rect) == 0
                && rect.right > rect.left
                && rect.bottom > rect.top
            {
                return Some(IconRect {
                    left: rect.left,
                    top: rect.top,
                    right: rect.right,
                    bottom: rect.bottom,
                });
            }
            None
        }
        #[cfg(not(windows))]
        None
    }

    /// The single delivery path for quota alerts and Settings → Test: a
    /// Windows toast under AUM's own app identity (see `notifications`).
    pub fn notify(&self, title: &str, message: &str, duration_seconds: u16) {
        #[cfg(windows)]
        {
            let tag = crate::notifications::tag_for(title);
            match crate::notifications::show(title, message, duration_seconds, &tag) {
                Ok(()) => eprintln!("NOTIFY: toast delivered (tag={tag})"),
                Err(error) => {
                    // Visible in the log; the legacy balloon keeps the alert
                    // from being lost but shows the generic shell identity.
                    eprintln!("NOTIFY: toast failed ({error}); using legacy tray balloon");
                    self.notify_legacy_balloon(title, message, duration_seconds);
                }
            }
        }
        #[cfg(not(windows))]
        let _ = (title, message, duration_seconds);
    }

    #[cfg(windows)]
    fn notify_legacy_balloon(&self, title: &str, message: &str, duration_seconds: u16) {
        unsafe {
            use windows_sys::Win32::UI::Shell::{
                NIF_INFO, NIIF_INFO, NIM_MODIFY, NOTIFYICONDATAW_0, Shell_NotifyIconW,
            };
            let mut data = tray_data(self.window as _, self.icon as _);
            data.uFlags = NIF_INFO;
            data.Anonymous = NOTIFYICONDATAW_0 {
                uTimeout: u32::from(duration_seconds).saturating_mul(1000),
            };
            data.dwInfoFlags = NIIF_INFO;
            copy_utf16(title, &mut data.szInfoTitle);
            copy_utf16(message, &mut data.szInfo);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }
}

#[cfg(windows)]
fn copy_utf16<const N: usize>(value: &str, target: &mut [u16; N]) {
    let mut index = 0;
    for character in value.chars() {
        let mut encoded = [0; 2];
        for unit in character.encode_utf16(&mut encoded).iter().copied() {
            if index + 1 >= N {
                return;
            }
            target[index] = unit;
            index += 1;
        }
    }
}

#[cfg(windows)]
pub struct NativeTray {
    window: usize,
    icon: usize,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
impl NativeTray {
    pub fn notifier(&self) -> TrayNotifier {
        TrayNotifier {
            window: self.window,
            icon: self.icon,
        }
    }

    pub fn start(
        locale: crate::localization::Locale,
    ) -> Result<(Self, std::sync::mpsc::Receiver<TrayEvent>), String> {
        use std::sync::mpsc;
        use std::time::Duration;

        let (events_tx, events_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("aum-windows-tray".into())
            .spawn(move || {
                if TRAY_EVENTS.set(events_tx).is_err() {
                    let _ = ready_tx.send(Err("tray event channel is already active".into()));
                    return;
                }
                set_locale(locale);
                match create_tray_window() {
                    Ok((window, icon, class_name, instance)) => {
                        let _ = ready_tx.send(Ok((window as usize, icon as usize)));
                        run_tray_loop(window, icon, &class_name, instance);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                }
            })
            .map_err(|error| format!("could not start tray thread: {error}"))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok((window, icon))) => Ok((
                Self {
                    window,
                    icon,
                    worker: Some(worker),
                },
                events_rx,
            )),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                let _ = worker.join();
                Err(format!("tray initialization did not finish: {error}"))
            }
        }
    }
}

#[cfg(windows)]
impl Drop for NativeTray {
    fn drop(&mut self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        unsafe {
            let _ = PostMessageW(self.window as _, WM_CLOSE, 0, 0);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(windows)]
static TRAY_EVENTS: std::sync::OnceLock<std::sync::mpsc::Sender<TrayEvent>> =
    std::sync::OnceLock::new();

#[cfg(windows)]
static TRAY_LOCALE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// True when the shell accepted NOTIFYICON_VERSION_4 (hover via
/// NIN_POPUPOPEN, menu via WM_CONTEXTMENU, event code in LOWORD(lParam)).
#[cfg(windows)]
static TRAY_VERSION_4: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
fn send_event(event: TrayEvent) {
    if let Some(events) = TRAY_EVENTS.get() {
        let _ = events.send(event);
    }
}

#[cfg(windows)]
const TRAY_CALLBACK: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 0x42;

/// Shell broadcast sent when the taskbar is (re)created, e.g. after an
/// Explorer restart. Registered once; 0 until then.
#[cfg(windows)]
static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The static AUM icon used until the first quota icon exists (not owned when
/// loaded from the EXE resource or the stock icon; never destroyed here).
#[cfg(windows)]
static TRAY_STATIC_ICON: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// True while the shell is known to hold AUM's notification-area icon.
#[cfg(windows)]
static TRAY_REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Logs a registration failure once per outage instead of on every retry.
#[cfg(windows)]
static TRAY_FAILURE_LOG: OnceLog = OnceLog::new();

/// A latch that lets the first failure of an outage be logged and reports
/// when the outage ended. Lock-free; safe from any thread.
pub(crate) struct OnceLog(std::sync::atomic::AtomicBool);

impl OnceLog {
    pub(crate) const fn new() -> Self {
        Self(std::sync::atomic::AtomicBool::new(false))
    }

    /// True only for the first failure since the last `recovered`.
    pub(crate) fn first_failure(&self) -> bool {
        !self.0.swap(true, std::sync::atomic::Ordering::AcqRel)
    }

    /// True when a failure had been logged and is now cleared.
    pub(crate) fn recovered(&self) -> bool {
        self.0.swap(false, std::sync::atomic::Ordering::AcqRel)
    }
}

/// `WM_POWERBROADCAST` codes that mean the system has resumed. Windows sends
/// `PBT_APMRESUMEAUTOMATIC` on every resume and `PBT_APMRESUMESUSPEND` as well
/// when the resume was user-initiated; callers coalesce the two.
pub(crate) fn is_resume_notification(code: usize) -> bool {
    const PBT_APMRESUMESUSPEND: usize = 0x0007;
    const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
    matches!(code, PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC)
}

/// Forces the next quota-icon update to render and send a fresh HICON even if
/// the quota values did not change (after resume or a shell restart).
#[cfg(windows)]
fn invalidate_rendered_icon() {
    if let Some(state) = TRAY_ICON_STATE.get()
        && let Ok(mut state) = state.lock()
    {
        *state = None;
    }
}

/// The icon the shell should currently show: the latest quota icon, else the
/// static AUM icon.
#[cfg(windows)]
fn current_tray_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    let dynamic = TRAY_CURRENT_DYNAMIC_ICON.load(std::sync::atomic::Ordering::Acquire);
    let icon = if dynamic != 0 {
        dynamic
    } else {
        TRAY_STATIC_ICON.load(std::sync::atomic::Ordering::Acquire)
    };
    icon as _
}

/// Adds AUM's icon to the notification area (NIM_ADD), or refreshes it with
/// NIM_MODIFY when the shell still has it (TaskbarCreated can also arrive for
/// taskbar DPI/theme changes). The identity is always (window, uID 1), so this
/// never creates a duplicate. Re-applies NOTIFYICON_VERSION_4 on success.
#[cfg(windows)]
fn register_tray_icon(
    window: windows_sys::Win32::Foundation::HWND,
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
) -> bool {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4,
        NOTIFYICONDATAW_0, Shell_NotifyIconW,
    };
    let mut data = tray_data(window, icon);
    let tip = "Agent Usage Monitor".encode_utf16().collect::<Vec<_>>();
    data.szTip[..tip.len()].copy_from_slice(&tip);
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = TRAY_CALLBACK;
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) } != 0;
    let error = if added { 0 } else { unsafe { GetLastError() } };
    let present = added || unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) } != 0;
    if !present {
        TRAY_REGISTERED.store(false, Ordering::Release);
        if TRAY_FAILURE_LOG.first_failure() {
            eprintln!(
                "TRAY_ICON: notification-area icon could not be registered (Windows error {error}); will retry when the taskbar is available"
            );
        }
        return false;
    }
    // Version 4 suppresses the plain tooltip (no NIF_SHOWTIP) so it does not
    // overlap the compact preview, and reports hover as NIN_POPUPOPEN.
    let mut version = tray_data(window, icon);
    version.Anonymous = NOTIFYICONDATAW_0 {
        uVersion: NOTIFYICON_VERSION_4,
    };
    let accepted = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &version) } != 0;
    TRAY_VERSION_4.store(accepted, Ordering::Relaxed);
    TRAY_REGISTERED.store(true, Ordering::Release);
    if TRAY_FAILURE_LOG.recovered() {
        eprintln!("TRAY_ICON: notification-area icon registered again");
    }
    true
}

#[cfg(windows)]
fn create_tray_window() -> Result<
    (
        windows_sys::Win32::Foundation::HWND,
        windows_sys::Win32::UI::WindowsAndMessaging::HICON,
        Vec<u16>,
        windows_sys::Win32::Foundation::HINSTANCE,
    ),
    String,
> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE, LoadIconW,
        LoadImageW, RegisterClassW, RegisterWindowMessageW, WNDCLASSW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };

    let class_name: Vec<u16> = "AgentUsageMonitor.TrayWindow\0".encode_utf16().collect();
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if instance.is_null() {
        return Err("could not access the current application module".into());
    }
    let class = WNDCLASSW {
        lpfnWndProc: Some(tray_window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(format!(
            "RegisterClassW failed with Windows error {}",
            unsafe { GetLastError() }
        ));
    }
    // A hidden top-level window (never shown; tool window, so it is never on
    // the taskbar or in Alt+Tab). Unlike a message-only window it receives the
    // shell's TaskbarCreated broadcast and WM_POWERBROADCAST.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if window.is_null() {
        return Err(format!(
            "CreateWindowExW failed with Windows error {}",
            unsafe { GetLastError() }
        ));
    }
    let mut icon = unsafe { LoadIconW(instance, std::ptr::without_provenance::<u16>(1)) };
    if icon.is_null() {
        if let Some(path) = option_env!("AUM_ICON_PATH") {
            let icon_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            icon = unsafe {
                LoadImageW(
                    std::ptr::null_mut(),
                    icon_path.as_ptr(),
                    IMAGE_ICON,
                    0,
                    0,
                    LR_LOADFROMFILE | LR_DEFAULTSIZE,
                ) as _
            };
        }
    }
    if icon.is_null() {
        icon = unsafe {
            LoadIconW(
                std::ptr::null_mut(),
                std::ptr::without_provenance::<u16>(IDI_APPLICATION as usize),
            )
        };
    }
    if icon.is_null() {
        return Err(format!(
            "LoadIconW and generated ICO fallback failed with Windows error {}",
            unsafe { GetLastError() }
        ));
    }
    TRAY_STATIC_ICON.store(icon as usize, std::sync::atomic::Ordering::Release);
    let taskbar_created: Vec<u16> = "TaskbarCreated\0".encode_utf16().collect();
    let message = unsafe { RegisterWindowMessageW(taskbar_created.as_ptr()) };
    if message == 0 {
        eprintln!(
            "TRAY_ICON: TaskbarCreated could not be registered (Windows error {}); Explorer restarts will not restore the icon",
            unsafe { GetLastError() }
        );
    }
    TASKBAR_CREATED.store(message, std::sync::atomic::Ordering::Release);
    // If the shell is not ready yet (early sign-in), registration is retried
    // on TaskbarCreated and on the next quota-icon update.
    let _ = register_tray_icon(window, icon);
    Ok((window, icon, class_name, instance))
}

#[cfg(windows)]
fn tray_data(
    window: windows_sys::Win32::Foundation::HWND,
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
) -> windows_sys::Win32::UI::Shell::NOTIFYICONDATAW {
    use windows_sys::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NOTIFYICONDATAW};
    windows_sys::Win32::UI::Shell::NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE,
        uCallbackMessage: TRAY_CALLBACK,
        hIcon: icon,
        ..Default::default()
    }
}

#[cfg(windows)]
fn run_tray_loop(
    window: windows_sys::Win32::Foundation::HWND,
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    class_name: &[u16],
    instance: windows_sys::Win32::Foundation::HINSTANCE,
) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{NIM_DELETE, Shell_NotifyIconW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, TranslateMessage, UnregisterClassW,
    };
    let mut message = MSG::default();
    unsafe {
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let data = tray_data(window, icon);
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        let _ = UnregisterClassW(
            class_name.as_ptr(),
            if instance.is_null() {
                GetModuleHandleW(std::ptr::null())
            } else {
                instance
            },
        );
    }
    let dynamic = TRAY_CURRENT_DYNAMIC_ICON.swap(0, std::sync::atomic::Ordering::AcqRel);
    if dynamic != 0 {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon(dynamic as _);
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn tray_window_proc(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::NIN_POPUPOPEN;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, DestroyWindow, PostQuitMessage, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY,
        WM_LBUTTONDBLCLK, WM_MOUSEMOVE, WM_POWERBROADCAST, WM_RBUTTONUP,
    };
    if message == TRAY_ICON_UPDATE {
        update_native_icon(window);
        return 0;
    }
    let taskbar_created = TASKBAR_CREATED.load(std::sync::atomic::Ordering::Acquire);
    if taskbar_created != 0 && message == taskbar_created {
        // Explorer (re)created the taskbar: our icon is gone. Re-add the
        // current icon now and redraw the quota icon on the next update.
        eprintln!("TRAY_ICON: taskbar recreated; restoring the notification-area icon");
        TRAY_REGISTERED.store(false, std::sync::atomic::Ordering::Release);
        let _ = register_tray_icon(window, current_tray_icon());
        invalidate_rendered_icon();
        return 0;
    }
    if message == WM_POWERBROADCAST {
        if is_resume_notification(wparam) {
            invalidate_rendered_icon();
            send_event(TrayEvent::Resumed);
        }
        // TRUE: AUM never vetoes power transitions.
        return 1;
    }
    if message == TRAY_CALLBACK {
        let version_4 = TRAY_VERSION_4.load(std::sync::atomic::Ordering::Relaxed);
        // Version 4: event in LOWORD(lParam). Legacy: lParam is the event.
        let event = (lparam as u32) & 0xffff;
        match event {
            // A single click (WM_LBUTTONUP / NIN_SELECT) is deliberately inert.
            WM_LBUTTONDBLCLK => send_event(TrayEvent::OpenFull),
            NIN_POPUPOPEN if version_4 => send_event(TrayEvent::Hover),
            WM_MOUSEMOVE => send_event(TrayEvent::Pointer),
            // Version 4 passes the anchor (cursor or icon for the keyboard menu
            // key) in wParam as signed 16-bit x/y.
            WM_CONTEXTMENU if version_4 => send_event(TrayEvent::ContextMenu {
                x: i32::from((wparam & 0xffff) as u16 as i16),
                y: i32::from(((wparam >> 16) & 0xffff) as u16 as i16),
            }),
            WM_RBUTTONUP if !version_4 => {
                if let Some((x, y)) = cursor_position() {
                    send_event(TrayEvent::ContextMenu { x, y });
                }
            }
            _ => {}
        }
        return 0;
    }
    if message == WM_CLOSE {
        unsafe {
            DestroyWindow(window);
        }
        return 0;
    }
    if message == WM_DESTROY {
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[cfg(windows)]
fn cursor_position() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x, point.y))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFreshness {
    Fresh,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaValue {
    pub remaining_percent: Option<u8>,
    pub freshness: DataFreshness,
}

/// Tray bar colours (RGBA). Warning is the shared semantic yellow #FACC15,
/// which stays visible on both dark and light taskbars at 16 px.
pub const TRAY_NORMAL: [u8; 4] = [76, 190, 126, 255];
pub const TRAY_WARNING: [u8; 4] = [250, 204, 21, 255];
pub const TRAY_CRITICAL: [u8; 4] = [224, 88, 95, 255];

/// Deterministic RGBA icon pixels, row-major, with transparent unused pixels.
/// Each bar grows from the bottom in proportion to its own remaining quota.
pub fn render_rgba_with_thresholds(
    size: u32,
    values: [QuotaValue; 2],
    _dark_taskbar: bool,
    warning_threshold: u8,
    critical_threshold: u8,
) -> Option<Vec<u8>> {
    if ![16, 20, 24, 32].contains(&size) {
        return None;
    }
    let neutral = [128, 136, 144, 255];
    let track = [82, 88, 96, 255];
    let mut pixels = vec![0; (size * size * 4) as usize];
    let col_width = (size / 4).max(3);
    let gap = (size / 8).max(2);
    let total_width = col_width * 2 + gap;
    let x_start = (size - total_width) / 2;
    let top = 2;
    let bottom = size - 2;
    let height = bottom - top;
    for (column, value) in values.into_iter().enumerate() {
        let x0 = x_start + column as u32 * (col_width + gap);
        // Paint a full-height neutral rail first; the quota fill overlays it.
        paint_rect(&mut pixels, size, x0, top, col_width, height, track);
        let Some(pct) = value.remaining_percent else {
            // A muted full track clearly indicates unavailable data.
            paint_rect(&mut pixels, size, x0, top, col_width, height, neutral);
            continue;
        };
        let pct = pct.min(100);
        let fill_height = ((height * u32::from(pct)) / 100).max(1);
        let color = match value.freshness {
            DataFreshness::Fresh if pct <= critical_threshold => TRAY_CRITICAL,
            DataFreshness::Fresh if pct <= warning_threshold => TRAY_WARNING,
            DataFreshness::Fresh => TRAY_NORMAL,
            DataFreshness::Stale => [159, 151, 132, 255],
            DataFreshness::Unavailable => neutral,
        };
        for y in bottom - fill_height..bottom {
            // Stale readings use a subtle two-row break pattern instead of a
            // misleading current healthy/warning/critical color.
            if value.freshness == DataFreshness::Stale && (bottom - y) % 3 == 0 {
                continue;
            }
            paint_rect(&mut pixels, size, x0, y, col_width, 1, color);
        }
    }
    Some(pixels)
}

fn paint_rect(
    pixels: &mut [u8],
    size: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    for row in y..y + height {
        for column in x..x + width {
            let index = ((row * size + column) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&color);
        }
    }
}

#[cfg(windows)]
const TRAY_ICON_UPDATE: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 0x43;

/// Pending icon frame: (size, BGRA pixels).
#[cfg(windows)]
type PendingIcon = Option<(u32, Vec<u8>)>;

#[cfg(windows)]
static TRAY_ICON_PIXELS: std::sync::OnceLock<std::sync::Mutex<PendingIcon>> =
    std::sync::OnceLock::new();

/// Last rendered input: (values, dark taskbar, warning, critical).
#[cfg(windows)]
type RenderedIconKey = Option<([QuotaValue; 2], bool, u8, u8)>;

#[cfg(windows)]
static TRAY_ICON_STATE: std::sync::OnceLock<std::sync::Mutex<RenderedIconKey>> =
    std::sync::OnceLock::new();

#[cfg(windows)]
static TRAY_CURRENT_DYNAMIC_ICON: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(windows)]
fn update_native_icon(window: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::UI::Shell::{NIF_ICON, NIM_MODIFY, Shell_NotifyIconW};
    let update = TRAY_ICON_PIXELS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut pending| pending.take());
    let Some((size, pixels)) = update else {
        return;
    };
    let icon = match create_icon_from_rgba(size, &pixels) {
        Some(icon) => icon,
        None => {
            eprintln!("TRAY_ICON: Windows rejected the generated bitmap");
            return;
        }
    };
    // Normal path: NIM_MODIFY. If the shell lost the icon (Explorer restart,
    // shell busy after resume), fall back once to a full re-registration.
    let modified = TRAY_REGISTERED.load(std::sync::atomic::Ordering::Acquire) && {
        let mut data = tray_data(window, icon);
        data.uFlags = NIF_ICON;
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) != 0 }
    };
    if !modified && !register_tray_icon(window, icon) {
        // The shell keeps no reference on failure: AUM still owns this icon.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon(icon) };
        // Re-send on the next 1 s icon tick; the failure is logged only once.
        invalidate_rendered_icon();
        return;
    }
    eprintln!("TRAY_ICON: Windows accepted refreshed 16x16 quota icon");
    let previous =
        TRAY_CURRENT_DYNAMIC_ICON.swap(icon as usize, std::sync::atomic::Ordering::AcqRel);
    if previous != 0 {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon(previous as _);
        }
    }
}

#[cfg(windows)]
fn create_icon_from_rgba(
    size: u32,
    rgba: &[u8],
) -> Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
        DeleteObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO};

    if rgba.len() != (size * size * 4) as usize {
        return None;
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let color = unsafe {
        CreateDIBSection(
            std::ptr::null_mut(),
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        )
    };
    if color.is_null() || bits.is_null() {
        if !color.is_null() {
            unsafe { DeleteObject(color) };
        }
        return None;
    }
    let source = rgba.chunks_exact(4);
    let destination = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), rgba.len()) };
    for (source, destination) in source.zip(destination.chunks_exact_mut(4)) {
        destination.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
    }
    let mask = unsafe { CreateBitmap(size as i32, size as i32, 1, 1, std::ptr::null()) };
    if mask.is_null() {
        unsafe { DeleteObject(color) };
        return None;
    }
    let icon_info = ICONINFO {
        fIcon: 1,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: color,
    };
    let icon = unsafe { CreateIconIndirect(&icon_info) };
    unsafe {
        DeleteObject(mask);
        DeleteObject(color);
    }
    (!icon.is_null()).then_some(icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_resume_power_notifications_trigger_recovery() {
        // PBT_APMRESUMESUSPEND, PBT_APMRESUMEAUTOMATIC
        assert!(is_resume_notification(0x0007));
        assert!(is_resume_notification(0x0012));
        // PBT_APMSUSPEND, PBT_APMPOWERSTATUSCHANGE, PBT_POWERSETTINGCHANGE
        for code in [0x0004, 0x000A, 0x8013] {
            assert!(!is_resume_notification(code), "{code:#x}");
        }
    }

    #[test]
    fn registration_failures_are_logged_once_per_outage() {
        let log = OnceLog::new();
        assert!(!log.recovered(), "nothing to report before a failure");
        assert!(log.first_failure());
        // Repeated retries during the same outage stay silent.
        assert!(!log.first_failure());
        assert!(!log.first_failure());
        assert!(log.recovered());
        assert!(!log.recovered());
        // A new outage is reported again.
        assert!(log.first_failure());
    }

    #[test]
    fn context_menu_has_exactly_four_localized_commands_in_order() {
        use crate::localization::Locale;
        assert_eq!(
            menu_labels(Locale::Uk),
            ["Відкрити", "Оновити ліміти", "Налаштування", "Вихід"]
        );
        for locale in [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru] {
            assert!(menu_labels(locale).iter().all(|label| !label.is_empty()));
        }
    }

    #[test]
    fn warning_bars_use_the_semantic_yellow_facc15_at_every_size() {
        assert_eq!(TRAY_WARNING, [0xfa, 0xcc, 0x15, 0xff]);
        let warning = QuotaValue {
            remaining_percent: Some(20),
            freshness: DataFreshness::Fresh,
        };
        for size in [16, 20, 24, 32] {
            let pixels = render_rgba_with_thresholds(size, [warning; 2], true, 25, 10).unwrap();
            assert!(pixels.chunks_exact(4).any(|pixel| pixel == TRAY_WARNING));
        }
        // Boundaries are unchanged: 25 % is still warning, 26 % normal, 10 % critical.
        let at = |percent| QuotaValue {
            remaining_percent: Some(percent),
            freshness: DataFreshness::Fresh,
        };
        let has = |pixels: &[u8], color: [u8; 4]| pixels.chunks_exact(4).any(|p| p == color);
        let p25 = render_rgba_with_thresholds(32, [at(25); 2], true, 25, 10).unwrap();
        assert!(has(&p25, TRAY_WARNING));
        let p26 = render_rgba_with_thresholds(32, [at(26); 2], true, 25, 10).unwrap();
        assert!(has(&p26, TRAY_NORMAL) && !has(&p26, TRAY_WARNING));
        let p10 = render_rgba_with_thresholds(32, [at(10); 2], true, 25, 10).unwrap();
        assert!(has(&p10, TRAY_CRITICAL) && !has(&p10, TRAY_WARNING));
    }

    #[test]
    fn renderer_supports_target_sizes_and_missing_data_is_neutral() {
        for size in [16, 20, 24, 32] {
            let pixels = render_rgba_with_thresholds(
                size,
                [QuotaValue {
                    remaining_percent: None,
                    freshness: DataFreshness::Unavailable,
                }; 2],
                true,
                25,
                10,
            )
            .unwrap();
            assert_eq!(pixels.len(), (size * size * 4) as usize);
            assert!(pixels.iter().any(|byte| *byte != 0));
        }
        assert!(
            render_rgba_with_thresholds(
                18,
                [QuotaValue {
                    remaining_percent: Some(50),
                    freshness: DataFreshness::Fresh
                }; 2],
                true,
                25,
                10,
            )
            .is_none()
        );
    }

    #[test]
    fn progress_geometry_fills_from_bottom_toward_top() {
        let empty = render_rgba_with_thresholds(
            32,
            [QuotaValue {
                remaining_percent: Some(0),
                freshness: DataFreshness::Fresh,
            }; 2],
            true,
            25,
            10,
        )
        .unwrap();
        let full = render_rgba_with_thresholds(
            32,
            [QuotaValue {
                remaining_percent: Some(100),
                freshness: DataFreshness::Fresh,
            }; 2],
            true,
            25,
            10,
        )
        .unwrap();
        assert_ne!(empty, full);
    }

    #[test]
    fn synthetic_quota_cases_render_and_export_review_images() {
        let fresh = |percent| QuotaValue {
            remaining_percent: Some(percent),
            freshness: DataFreshness::Fresh,
        };
        let missing = QuotaValue {
            remaining_percent: None,
            freshness: DataFreshness::Unavailable,
        };
        let cases = [
            ("case-a", [fresh(96), fresh(49)]),
            ("case-b", [fresh(20), fresh(80)]),
            ("case-c", [missing, fresh(65)]),
            ("case-d", [missing, missing]),
        ];
        let output = std::env::temp_dir().join("AUM-icon-review");
        std::fs::create_dir_all(&output).unwrap();
        for (name, values) in cases {
            for (dark_taskbar, background) in [(true, "dark"), (false, "light")] {
                for size in [16, 20, 24, 32] {
                    let rgba =
                        render_rgba_with_thresholds(size, values, dark_taskbar, 25, 10).unwrap();
                    std::fs::write(
                        output.join(format!("{name}-{size}-{background}.bmp")),
                        rgba_bmp(size, &rgba),
                    )
                    .unwrap();
                    std::fs::write(
                        output.join(format!("{name}-{size}-{background}-nearest-4x.bmp")),
                        upscale_bmp_rgba(size, &rgba, 4),
                    )
                    .unwrap();
                }
            }
        }

        let a = render_rgba_with_thresholds(16, cases[0].1, true, 25, 10).unwrap();
        assert_eq!(count_color(&a, 16, [76, 190, 126, 255], 3, 7), 44);
        assert_eq!(count_color(&a, 16, [76, 190, 126, 255], 9, 13), 20);
        let b = render_rgba_with_thresholds(16, cases[1].1, true, 25, 10).unwrap();
        assert_eq!(count_color(&b, 16, TRAY_WARNING, 3, 7), 8);
        assert_eq!(count_color(&b, 16, [76, 190, 126, 255], 9, 13), 36);
        let c = render_rgba_with_thresholds(16, cases[2].1, true, 25, 10).unwrap();
        assert_eq!(count_color(&c, 16, [128, 136, 144, 255], 3, 7), 48);
        assert_eq!(count_color(&c, 16, [76, 190, 126, 255], 9, 13), 28);
        let d = render_rgba_with_thresholds(16, cases[3].1, true, 25, 10).unwrap();
        assert_eq!(count_color(&d, 16, [128, 136, 144, 255], 3, 7), 48);
        assert_eq!(count_color(&d, 16, [128, 136, 144, 255], 9, 13), 48);
        assert_eq!(&d[..3], &[0, 0, 0]);
    }

    fn count_color(pixels: &[u8], size: u32, color: [u8; 4], x0: u32, x1: u32) -> usize {
        (0..size)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let index = ((*y * size + *x) * 4) as usize;
                pixels[index..index + 4] == color
            })
            .count()
    }

    fn rgba_bmp(size: u32, rgba: &[u8]) -> Vec<u8> {
        let pixel_bytes = size * size * 4;
        let mut bmp = Vec::with_capacity(54 + pixel_bytes as usize);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54 + pixel_bytes).to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&(size as i32).to_le_bytes());
        bmp.extend_from_slice(&(size as i32).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&32u16.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&pixel_bytes.to_le_bytes());
        bmp.extend_from_slice(&[0; 16]);
        for y in (0..size).rev() {
            for x in 0..size {
                let index = ((y * size + x) * 4) as usize;
                bmp.extend_from_slice(&[
                    rgba[index + 2],
                    rgba[index + 1],
                    rgba[index],
                    rgba[index + 3],
                ]);
            }
        }
        bmp
    }

    fn upscale_bmp_rgba(size: u32, rgba: &[u8], scale: u32) -> Vec<u8> {
        let enlarged = size * scale;
        let mut pixels = vec![0; (enlarged * enlarged * 4) as usize];
        for y in 0..enlarged {
            for x in 0..enlarged {
                let source = (((y / scale) * size + x / scale) * 4) as usize;
                let target = ((y * enlarged + x) * 4) as usize;
                pixels[target..target + 4].copy_from_slice(&rgba[source..source + 4]);
            }
        }
        rgba_bmp(enlarged, &pixels)
    }
}

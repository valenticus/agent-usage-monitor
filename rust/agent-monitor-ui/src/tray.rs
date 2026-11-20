#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    TogglePopup,
    OpenSettings,
    Quit,
}

#[derive(Clone)]
pub struct TrayNotifier {
    #[cfg(windows)]
    window: usize,
    #[cfg(windows)]
    icon: usize,
}

impl TrayNotifier {
    pub fn notify(&self, title: &str, message: &str, duration_seconds: u16) {
        #[cfg(windows)]
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
        #[cfg(not(windows))]
        let _ = (title, message, duration_seconds);
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
                let _ = TRAY_LOCALE.set(locale);
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
static TRAY_LOCALE: std::sync::OnceLock<crate::localization::Locale> = std::sync::OnceLock::new();

#[cfg(windows)]
const TRAY_CALLBACK: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 0x42;

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
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, HWND_MESSAGE, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTSIZE,
        LR_LOADFROMFILE, LoadIconW, LoadImageW, RegisterClassW, WNDCLASSW,
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
    let window = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
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
    let mut data = tray_data(window, icon);
    let tip = "Agent Usage Monitor".encode_utf16().collect::<Vec<_>>();
    data.szTip[..tip.len()].copy_from_slice(&tip);
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = TRAY_CALLBACK;
    if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
        return Err(format!(
            "Shell_NotifyIconW(NIM_ADD) failed with Windows error {}",
            unsafe { GetLastError() }
        ));
    }
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
}

#[cfg(windows)]
unsafe extern "system" fn tray_window_proc(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, DestroyWindow, PostQuitMessage, WM_CLOSE, WM_DESTROY, WM_LBUTTONUP,
        WM_RBUTTONUP,
    };
    if message == TRAY_CALLBACK {
        match lparam as u32 {
            WM_LBUTTONUP => {
                if let Some(events) = TRAY_EVENTS.get() {
                    let _ = events.send(TrayEvent::TogglePopup);
                }
                return 0;
            }
            WM_RBUTTONUP => {
                unsafe {
                    show_tray_menu(window);
                }
                return 0;
            }
            _ => {}
        }
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
unsafe fn show_tray_menu(window: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, MF_STRING, PostMessageW,
        SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
    };
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let labels = match TRAY_LOCALE
            .get()
            .copied()
            .unwrap_or(crate::localization::Locale::En)
        {
            crate::localization::Locale::Uk => ["Показати ліміти", "Налаштування", "Вийти"],
            crate::localization::Locale::En => ["Show quota", "Settings", "Quit"],
            crate::localization::Locale::Pl => ["Pokaż limity", "Ustawienia", "Zakończ"],
        };
        for (id, label) in [1001usize, 1002, 1003].into_iter().zip(labels) {
            let label: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = AppendMenuW(menu, MF_STRING, id, label.as_ptr());
        }
        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point) != 0 {
            let _ = SetForegroundWindow(window);
            let selected = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                window,
                std::ptr::null(),
            );
            let _ = PostMessageW(window, WM_NULL, 0, 0);
            let event = match selected {
                1001 => Some(TrayEvent::TogglePopup),
                1002 => Some(TrayEvent::OpenSettings),
                1003 => Some(TrayEvent::Quit),
                _ => None,
            };
            if let (Some(events), Some(event)) = (TRAY_EVENTS.get(), event) {
                let _ = events.send(event);
            }
        }
        let _ = DestroyMenu(menu);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFreshness {
    Fresh,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuotaValue {
    pub remaining_percent: Option<u8>,
    pub freshness: DataFreshness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaSource {
    CodexFiveHour,
    ClaudeFiveHour,
    CodexWeekly,
    ClaudeWeekly,
    Inactive,
}

pub fn sources(codex_enabled: bool, claude_enabled: bool) -> [QuotaSource; 2] {
    match (codex_enabled, claude_enabled) {
        (true, true) => [QuotaSource::CodexFiveHour, QuotaSource::ClaudeFiveHour],
        (true, false) => [QuotaSource::CodexFiveHour, QuotaSource::CodexWeekly],
        (false, true) => [QuotaSource::ClaudeFiveHour, QuotaSource::ClaudeWeekly],
        (false, false) => [QuotaSource::Inactive, QuotaSource::Inactive],
    }
}

/// Deterministic premultiplied-independent RGBA icon pixels, row-major.
/// Missing values remain neutral; exhaustion is explicitly red and empty.
pub fn render_rgba(size: u32, values: [QuotaValue; 2], dark_taskbar: bool) -> Option<Vec<u8>> {
    if ![16, 20, 24, 32].contains(&size) {
        return None;
    }
    let track = if dark_taskbar {
        [132, 143, 157, 255]
    } else {
        [104, 115, 129, 255]
    };
    let outline = if dark_taskbar {
        [231, 236, 242, 255]
    } else {
        [46, 55, 66, 255]
    };
    let mut pixels = vec![0; (size * size * 4) as usize];
    let col_width = (size / 4).max(3);
    let gap = (size / 8).max(2);
    let total_width = col_width * 2 + gap;
    let x_start = (size - total_width) / 2;
    let top = (size / 8).max(2);
    let bottom = size - top;
    let height = bottom - top;
    for (column, value) in values.into_iter().enumerate() {
        let x0 = x_start + column as u32 * (col_width + gap);
        for y in top..bottom {
            for x in x0..x0 + col_width {
                let edge = x == x0 || x + 1 == x0 + col_width || y == top || y + 1 == bottom;
                let index = ((y * size + x) * 4) as usize;
                let exhausted =
                    value.freshness == DataFreshness::Fresh && value.remaining_percent == Some(0);
                let border = if exhausted {
                    [224, 88, 95, 255]
                } else {
                    outline
                };
                pixels[index..index + 4].copy_from_slice(if edge { &border } else { &track });
                if !edge
                    && value.freshness == DataFreshness::Stale
                    && value.remaining_percent.is_none()
                    && (x + y) % 3 == 0
                {
                    let stale_unknown = [151, 132, 103, 255];
                    pixels[index..index + 4].copy_from_slice(&stale_unknown);
                }
                if !edge && value.freshness != DataFreshness::Unavailable {
                    let Some(pct) = value.remaining_percent else {
                        continue;
                    };
                    let pct = pct.min(100) as u32;
                    let fill_height = height.saturating_sub(2) * pct / 100;
                    if y >= bottom - 1 - fill_height {
                        let fill = match value.freshness {
                            DataFreshness::Stale => [180, 166, 137, 255],
                            DataFreshness::Unavailable => track,
                            DataFreshness::Fresh if pct == 0 => track,
                            DataFreshness::Fresh if pct <= 10 => [224, 88, 95, 255],
                            DataFreshness::Fresh if pct <= 25 => [224, 173, 74, 255],
                            DataFreshness::Fresh => [102, 194, 139, 255],
                        };
                        pixels[index..index + 4].copy_from_slice(&fill);
                    }
                }
            }
        }
    }
    Some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_all_four_provider_combinations_to_two_columns() {
        assert_eq!(
            sources(true, true),
            [QuotaSource::CodexFiveHour, QuotaSource::ClaudeFiveHour]
        );
        assert_eq!(
            sources(true, false),
            [QuotaSource::CodexFiveHour, QuotaSource::CodexWeekly]
        );
        assert_eq!(
            sources(false, true),
            [QuotaSource::ClaudeFiveHour, QuotaSource::ClaudeWeekly]
        );
        assert_eq!(
            sources(false, false),
            [QuotaSource::Inactive, QuotaSource::Inactive]
        );
    }

    #[test]
    fn renderer_supports_target_sizes_and_missing_data_is_neutral() {
        for size in [16, 20, 24, 32] {
            let pixels = render_rgba(
                size,
                [QuotaValue {
                    remaining_percent: None,
                    freshness: DataFreshness::Unavailable,
                }; 2],
                true,
            )
            .unwrap();
            assert_eq!(pixels.len(), (size * size * 4) as usize);
            assert!(pixels.iter().any(|byte| *byte != 0));
        }
        assert!(
            render_rgba(
                18,
                [QuotaValue {
                    remaining_percent: Some(50),
                    freshness: DataFreshness::Fresh
                }; 2],
                true
            )
            .is_none()
        );
    }

    #[test]
    fn progress_geometry_fills_from_bottom_toward_top() {
        let empty = render_rgba(
            32,
            [QuotaValue {
                remaining_percent: Some(0),
                freshness: DataFreshness::Fresh,
            }; 2],
            true,
        )
        .unwrap();
        let full = render_rgba(
            32,
            [QuotaValue {
                remaining_percent: Some(100),
                freshness: DataFreshness::Fresh,
            }; 2],
            true,
        )
        .unwrap();
        assert_ne!(empty, full);
    }

    #[test]
    fn stale_missing_is_distinct_from_unavailable_and_exhausted_is_red() {
        let unknown = QuotaValue {
            remaining_percent: None,
            freshness: DataFreshness::Unavailable,
        };
        let stale = QuotaValue {
            remaining_percent: None,
            freshness: DataFreshness::Stale,
        };
        let empty = QuotaValue {
            remaining_percent: Some(0),
            freshness: DataFreshness::Fresh,
        };
        let neutral = render_rgba(24, [unknown; 2], true).unwrap();
        let stale = render_rgba(24, [stale; 2], true).unwrap();
        let exhausted = render_rgba(24, [empty; 2], true).unwrap();
        assert_ne!(neutral, stale);
        assert_ne!(neutral, exhausted);
    }
}

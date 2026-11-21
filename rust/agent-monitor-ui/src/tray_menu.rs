//! Theme-aware tray context menu. It replaces the native `HMENU`, which
//! Windows always draws with the light system menu theme.
//!
//! One `TrayMenu` window exists for the whole run, so repeated right-clicks
//! reuse it (no duplicate menus). It is an opaque Slint window: Windows 11
//! DWM rounds its corners and draws the shadow (the software renderer has no
//! per-pixel alpha, so Slint-drawn transparent corners would turn black).
//! Keyboard: Up/Down/Home/End move, Enter/Space activate, Escape closes;
//! activating another window (an outside click) closes it too.

use slint::ComponentHandle;

use crate::TrayMenu;
use crate::flyout::WorkArea;

pub const OPEN: i32 = 0;
pub const REFRESH: i32 = 1;
pub const SETTINGS: i32 = 2;
pub const EXIT: i32 = 3;

/// Opens (or re-opens) the menu next to `anchor`, in physical screen pixels.
pub fn open(
    menu: &TrayMenu,
    anchor: (i32, i32),
    theme_kind: i32,
    locale: crate::localization::Locale,
) {
    let [open, refresh, settings, exit] = crate::tray::menu_labels(locale);
    menu.set_open_label(open.into());
    menu.set_refresh_label(refresh.into());
    menu.set_settings_label(settings.into());
    menu.set_exit_label(exit.into());
    menu.set_theme_kind(theme_kind);

    let work = crate::flyout::work_area_at(anchor.0, anchor.1);
    let place_at = |scale: f32| {
        let width = (menu.get_menu_width() * scale).round() as i32;
        let height = (menu.get_menu_height() * scale).round() as i32;
        let (x, y) = place(anchor, width, height, work);
        slint::PhysicalPosition::new(x, y)
    };
    menu.window()
        .set_position(place_at(menu.window().scale_factor()));
    let _ = menu.show();
    // The window may have moved to a monitor with another DPI: place it again
    // with the scale it actually got.
    menu.window()
        .set_position(place_at(menu.window().scale_factor()));
    #[cfg(windows)]
    if let Some(hwnd) = hwnd(menu) {
        native_setup(hwnd);
    }
    menu.invoke_reset();
}

pub fn close(menu: &TrayMenu) {
    if menu.window().is_visible() {
        let _ = menu.hide();
    }
}

/// Shell-like placement: the menu opens above and to the right of the
/// cursor, flips left/down when that would leave the monitor work area, and
/// is always clamped inside it (taskbar on any edge, multiple monitors).
pub(crate) fn place(
    anchor: (i32, i32),
    width: i32,
    height: i32,
    work: Option<WorkArea>,
) -> (i32, i32) {
    let (ax, ay) = anchor;
    let mut x = ax;
    let mut y = ay - height;
    if let Some(work) = work {
        if x + width > work.right {
            x = ax - width;
        }
        if y < work.top {
            y = ay;
        }
        x = x.clamp(work.left, (work.right - width).max(work.left));
        y = y.clamp(work.top, (work.bottom - height).max(work.top));
    }
    (x, y)
}

#[cfg(windows)]
fn hwnd(menu: &TrayMenu) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = menu.window().window_handle();
    let handle = handle.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get() as windows_sys::Win32::Foundation::HWND)
}

/// Tool window (no taskbar/Alt+Tab entry), no native caption, system
/// rounded corners without the DWM outline, then keyboard focus.
#[cfg(windows)]
fn native_setup(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DwmSetWindowAttribute,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, WS_EX_APPWINDOW,
        WS_EX_TOOLWINDOW,
    };
    crate::flyout::guard_caption_hwnd(hwnd);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let updated = (style | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW;
        if style != updated {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, updated as isize);
            let _ = SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
            );
        }
        let corners = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&corners as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
        let border: u32 = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            (&border as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        );
        // The shell gives the tray icon's process foreground rights for the
        // click that opened the menu; without focus the menu could not get
        // keyboard input or notice an outside click.
        if SetForegroundWindow(hwnd) == 0 {
            eprintln!("TRAY_MENU: could not take keyboard focus");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: WorkArea = WorkArea {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1032,
    };

    #[test]
    fn opens_above_right_of_the_cursor() {
        assert_eq!(place((1000, 1050), 232, 145, Some(WORK)), (1000, 887));
    }

    #[test]
    fn flips_left_near_the_right_edge_and_stays_inside() {
        assert_eq!(place((1900, 1050), 232, 145, Some(WORK)), (1668, 887));
        let (x, _) = place((1919, 500), 232, 145, Some(WORK));
        assert!(x + 232 <= WORK.right);
    }

    #[test]
    fn opens_below_when_the_taskbar_is_on_top() {
        let work = WorkArea { top: 48, ..WORK };
        assert_eq!(place((1000, 20), 232, 145, Some(work)), (1000, 48));
    }

    #[test]
    fn second_monitor_with_negative_coordinates() {
        let work = WorkArea {
            left: -2560,
            top: 0,
            right: 0,
            bottom: 1392,
        };
        let (x, y) = place((-100, 1420), 232, 145, Some(work));
        assert!(x >= -2560 && x + 232 <= 0);
        assert!(y >= 0 && y + 145 <= 1392);
    }

    #[test]
    fn localized_labels_are_complete() {
        for locale in [
            crate::localization::Locale::Uk,
            crate::localization::Locale::Pl,
            crate::localization::Locale::En,
            crate::localization::Locale::Ru,
        ] {
            assert!(
                crate::tray::menu_labels(locale)
                    .iter()
                    .all(|label| !label.is_empty())
            );
        }
    }
}

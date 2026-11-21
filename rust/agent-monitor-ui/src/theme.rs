pub fn system_accent_color() -> slint::Color {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        use windows_sys::Win32::System::Registry::{
            HKEY_CURRENT_USER, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RegGetValueW,
        };

        let key: Vec<u16> = "Software\\Microsoft\\Windows\\DWM\0"
            .encode_utf16()
            .collect();
        let name: Vec<u16> = "AccentColor\0".encode_utf16().collect();
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD as REG_ROUTINE_FLAGS,
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast::<c_void>(),
                &mut size,
            )
        };
        if result == 0 && size == std::mem::size_of::<u32>() as u32 {
            return slint::Color::from_rgb_u8(value as u8, (value >> 8) as u8, (value >> 16) as u8);
        }
    }
    slint::Color::from_rgb_u8(0, 120, 212)
}

/// Colors the native Settings title bar like the sidebar of the active theme
/// (Graphite #1B1B1B, Bluish Dark #0E141C, Light #E8E8E8). No-op until the
/// window has a native handle (returns false then), and on systems without
/// DWM caption colors.
pub fn apply_title_bar(window: &slint::Window, mode: crate::settings::ThemeMode) -> bool {
    #[cfg(windows)]
    {
        use crate::settings::ThemeMode;
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use std::ffi::c_void;
        use windows_sys::Win32::Graphics::Dwm::{
            DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
            DwmSetWindowAttribute,
        };

        let handle = window.window_handle();
        let Ok(handle) = handle.window_handle() else {
            return false;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return false;
        };
        let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        let (dark, caption, text): (i32, u32, u32) = match mode {
            ThemeMode::Graphite => (1, colorref(0x1b, 0x1b, 0x1b), colorref(0xf3, 0xf3, 0xf3)),
            ThemeMode::BluishDark => (1, colorref(0x0e, 0x14, 0x1c), colorref(0xea, 0xf0, 0xf6)),
            ThemeMode::Light => (0, colorref(0xe8, 0xe8, 0xe8), colorref(0x1b, 0x1b, 0x1b)),
        };
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                (&dark as *const i32).cast::<c_void>(),
                std::mem::size_of::<i32>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR as u32,
                (&caption as *const u32).cast::<c_void>(),
                std::mem::size_of::<u32>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TEXT_COLOR as u32,
                (&text as *const u32).cast::<c_void>(),
                std::mem::size_of::<u32>() as u32,
            );
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = (window, mode);
        true
    }
}

#[cfg(windows)]
fn colorref(red: u8, green: u8, blue: u8) -> u32 {
    u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
}

/// The AUM application mark (spec "Іконка програми") as a Slint image, for
/// the Settings window icon. Rendered by the shared [`crate::aum_icon`] code.
pub fn aum_icon_image(size: u32) -> slint::Image {
    let rgba = crate::aum_icon::render_rgba(size);
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&rgba, size, size);
    slint::Image::from_rgba8(buffer)
}

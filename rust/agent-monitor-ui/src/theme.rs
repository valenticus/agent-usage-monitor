#[cfg(windows)]
pub fn system_prefers_light() -> bool {
    use std::ffi::c_void;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RegGetValueW,
    };

    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let value: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
    let mut setting = 1u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // This is the public Windows per-application theme preference. If it is absent,
    // default to light application surfaces only for the System mode fallback.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD as REG_ROUTINE_FLAGS,
            std::ptr::null_mut(),
            (&mut setting as *mut u32).cast::<c_void>(),
            &mut size,
        )
    };
    result != 0 || setting != 0
}

#[cfg(not(windows))]
pub fn system_prefers_light() -> bool {
    false
}

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

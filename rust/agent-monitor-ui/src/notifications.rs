//! Windows toast notifications with AUM's own identity, for an unpackaged
//! Win32 app.
//!
//! Windows shows a toast's sender name and header icon from the app's
//! AppUserModelID (AUMID). An unpackaged app declares that identity per user
//! under `HKCU\Software\Classes\AppUserModelId\<AUMID>` (`DisplayName`,
//! `IconUri`) — the same registration the Windows App SDK's
//! `AppNotificationManager` and the Windows Community Toolkit write for
//! unpackaged apps. AUM writes only that key and one PNG of its static mark
//! in its own data folder. Sign-in launch is managed separately by
//! `autostart.rs` and is disabled by default.
//! Delivery goes through `ToastNotificationManager`, so the Notification
//! Center, Do Not Disturb / Focus and the per-app notification switch apply.
//!
//! Packaged (MSIX / Microsoft Store): the package already gives the process an
//! AUMID (`<PackageFamilyName>!AgentUsageMonitor`) with the manifest's display
//! name and logo. Nothing is registered, no explicit AUMID is set (it would
//! override the package identity) and the default notifier is used.

pub const APP_USER_MODEL_ID: &str = "Valenticus.AgentUsageMonitor";
pub const DISPLAY_NAME: &str = "AgentUsageMonitor";
const ICON_FILE: &str = "notification-icon.png";
const ICON_SIZE: u32 = 256;

/// Declares the app identity for this process and the current user. Safe to
/// call on every start: the registry values and PNG are rewritten in place.
#[cfg(windows)]
pub fn register_identity() -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    match crate::packaged::state() {
        crate::packaged::PackageState::Unpackaged => {}
        // The package provides the identity; nothing is registered.
        crate::packaged::PackageState::Packaged => return Ok(()),
        // Never write the unpackaged registration without confirmed identity.
        crate::packaged::PackageState::Unknown => {
            return Err("package identity unknown; AUMID registration skipped".into());
        }
    }
    let aumid = wide(APP_USER_MODEL_ID);
    let result = unsafe { SetCurrentProcessExplicitAppUserModelID(aumid.as_ptr()) };
    if result < 0 {
        return Err(format!(
            "SetCurrentProcessExplicitAppUserModelID failed: 0x{:08X}",
            result as u32
        ));
    }
    let icon = write_icon_png()?;
    let key = format!(r"Software\Classes\AppUserModelId\{APP_USER_MODEL_ID}");
    set_registry_string(&key, "DisplayName", DISPLAY_NAME)?;
    set_registry_string(&key, "IconUri", &icon.to_string_lossy())?;
    Ok(())
}

/// Shows one toast. `tag` replaces an earlier toast with the same tag in the
/// Notification Center instead of stacking duplicates.
#[cfg(windows)]
pub fn show(title: &str, message: &str, duration_seconds: u16, tag: &str) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{
        NotificationSetting, ToastNotification, ToastNotificationManager,
    };
    use windows::core::HSTRING;

    let duration = if duration_seconds > 10 {
        "long"
    } else {
        "short"
    };
    let packaged = crate::packaged::is_packaged();
    // Packaged toasts take their logo from the package manifest.
    let logo = crate::settings::settings_directory().map(|directory| directory.join(ICON_FILE));
    let logo_element = if let Some(logo) = logo.filter(|logo| !packaged && logo.is_file()) {
        format!(
            r#"<image placement="appLogoOverride" src="{}"/>"#,
            escape_xml(&file_uri(&logo))
        )
    } else {
        String::new()
    };
    let payload = format!(
        r#"<toast duration="{duration}"><visual><binding template="ToastGeneric"><text hint-maxLines="2">{}</text><text>{}</text>{logo_element}</binding></visual></toast>"#,
        escape_xml(title),
        escape_xml(message),
    );

    let document = XmlDocument::new().map_err(|error| format!("XmlDocument: {error}"))?;
    document
        .LoadXml(&HSTRING::from(payload))
        .map_err(|error| format!("toast XML: {error}"))?;
    let toast = ToastNotification::CreateToastNotification(&document)
        .map_err(|error| format!("CreateToastNotification: {error}"))?;
    let tag: String = tag.chars().take(64).collect();
    // Tag + Group is the toast's key in the Notification Center.
    toast
        .SetTag(&HSTRING::from(tag))
        .map_err(|error| format!("SetTag: {error}"))?;
    toast
        .SetGroup(&HSTRING::from("aum"))
        .map_err(|error| format!("SetGroup: {error}"))?;
    let notifier = if packaged {
        ToastNotificationManager::CreateToastNotifier()
            .map_err(|error| format!("CreateToastNotifier: {error}"))?
    } else {
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_USER_MODEL_ID))
            .map_err(|error| format!("CreateToastNotifierWithId: {error}"))?
    };
    // Windows silently drops toasts the user or policy has turned off; report
    // that state instead of treating it as a delivery failure.
    if let Ok(setting) = notifier.Setting() {
        if setting != NotificationSetting::Enabled {
            eprintln!("NOTIFY: Windows reports notifications disabled for AUM ({setting:?})");
        }
    }
    notifier
        .Show(&toast)
        .map_err(|error| format!("ToastNotifier.Show: {error}"))
}

/// Stable short tag for a notification title (FNV-1a), so a repeated alert
/// or Test replaces its previous toast rather than piling up.
pub fn tag_for(title: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in title.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("aum-{hash:016x}")
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(windows)]
fn file_uri(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file:///");
    for character in text.chars() {
        match character {
            ' ' => uri.push_str("%20"),
            '#' => uri.push_str("%23"),
            '%' => uri.push_str("%25"),
            _ => uri.push(character),
        }
    }
    uri
}

#[cfg(windows)]
fn write_icon_png() -> Result<std::path::PathBuf, String> {
    // Only reached for confirmed unpackaged processes (see register_identity).
    let directory = crate::settings::settings_directory()
        .ok_or_else(|| "no persistent data folder".to_string())?;
    std::fs::create_dir_all(&directory).map_err(|error| format!("data folder: {error}"))?;
    let path = directory.join(ICON_FILE);
    let rgba = crate::aum_icon::render_rgba(ICON_SIZE);
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, ICON_SIZE, ICON_SIZE);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("PNG header: {error}"))?;
        writer
            .write_image_data(&rgba)
            .map_err(|error| format!("PNG data: {error}"))?;
    }
    if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::write(&path, &bytes).map_err(|error| format!("PNG write: {error}"))?;
    }
    Ok(path)
}

#[cfg(windows)]
fn set_registry_string(key: &str, name: &str, value: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };

    let key_name = wide(key);
    let mut handle: HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_name.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut handle,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(format!("RegCreateKeyExW({key}) failed: {status}"));
    }
    let value_name = wide(name);
    let data = wide(value);
    let status = unsafe {
        RegSetValueExW(
            handle,
            value_name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    unsafe { RegCloseKey(handle) };
    if status != 0 {
        return Err(format!("RegSetValueExW({name}) failed: {status}"));
    }
    Ok(())
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_toast_text() {
        assert_eq!(
            escape_xml(r#"Codex <5 год> & "Claude" 'x'"#),
            "Codex &lt;5 год&gt; &amp; &quot;Claude&quot; &apos;x&apos;"
        );
    }

    #[test]
    fn tags_are_stable_short_and_distinct() {
        let a = tag_for("Codex · 5 год · попередження");
        assert_eq!(a, tag_for("Codex · 5 год · попередження"));
        assert_ne!(a, tag_for("Claude · 5 год · попередження"));
        assert!(a.len() <= 64);
    }

    #[test]
    fn static_mark_has_tile_and_bars_only_small_frame() {
        let large = crate::aum_icon::render_rgba(32);
        // Tile corner is transparent (radius), tile centre-top is opaque.
        assert_eq!(large[3], 0);
        assert_eq!(large[((32 + 16) * 4 + 3) as usize], 255);
        let small = crate::aum_icon::render_rgba(16);
        // Bars-only frame: the gap between the bars stays transparent.
        assert_eq!(small[((8 * 16 + 8) * 4 + 3) as usize], 0);
    }
}

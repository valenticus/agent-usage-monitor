use std::path::PathBuf;

#[path = "src/aum_icon.rs"]
#[allow(dead_code)]
mod aum_icon;

/// Frame sizes in the EXE icon: the spec's 16/24/32/48/256 plus 20/40/64 for
/// 125%/150%/200% scaling of the 16/32 px shell sizes.
const ICON_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

fn main() {
    slint_build::compile("ui/main.slint").expect("Slint UI compilation failed");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/aum_icon.rs");
    bundle_provider_icons();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            if let Err(error) = embed_windows_icon() {
                println!("cargo:warning=Windows icon resource was not embedded: {error}");
            }
        } else {
            println!(
                "cargo:warning=Windows icon resource embedding is configured for the MSVC linker only"
            );
        }
    }
}

/// User-provided build overrides accepted by the runtime. These are bundled
/// only when AUM_PROVIDER_ICONS_DIR is explicitly set.
const OVERRIDE_PROVIDER_ICON_FILES: [&str; 18] = [
    "codex-on-dark.png",
    "OAI_OpenAI-Blossom_White.png",
    "codex-on-light.png",
    "OAI_OpenAI-Blossom_Black.png",
    "codex.png",
    "claude-on-dark.png",
    "claude-on-light.png",
    "claude.png",
    "Claude Spark - Clay.png",
    "codex-on-dark.svg",
    "codex-on-light.svg",
    "codex.svg",
    "OAI_OpenAI-Blossom_White.svg",
    "OAI_OpenAI-Blossom_Black.svg",
    "claude-on-dark.svg",
    "claude-on-light.svg",
    "claude.svg",
    "Claude Spark - Clay.svg",
];
const TRACKED_PROVIDER_ICON_FILES: [&str; 3] = [
    "OAI_OpenAI-Blossom_White.svg",
    "OAI_OpenAI-Blossom_Black.svg",
    "Claude Spark - Clay.svg",
];
const MAX_PROVIDER_ICON_BYTES: u64 = 2 * 1024 * 1024;

/// Embeds tracked original SVGs automatically. An optional directory can add
/// user-supplied build overrides without affecting clean checkout builds.
fn bundle_provider_icons() {
    println!("cargo:rerun-if-env-changed=AUM_PROVIDER_ICONS_DIR");
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let tracked_directory = manifest.join("assets").join("providers");
    let mut sources: Vec<(PathBuf, &[&str])> = Vec::new();
    if let Some(directory) = std::env::var_os("AUM_PROVIDER_ICONS_DIR") {
        sources.push((PathBuf::from(directory), &OVERRIDE_PROVIDER_ICON_FILES));
    }
    sources.push((tracked_directory.clone(), &TRACKED_PROVIDER_ICON_FILES));
    let mut entries = String::new();
    let mut names = Vec::new();
    for (directory, files) in sources {
        println!("cargo:rerun-if-changed={}", directory.display());
        for name in files {
            let path = directory.join(name);
            println!("cargo:rerun-if-changed={}", path.display());
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            if !metadata.is_file() || metadata.len() > MAX_PROVIDER_ICON_BYTES {
                println!("cargo:warning=Provider icon skipped (not a file or over 2 MiB): {name}");
                continue;
            }
            let absolute = path.canonicalize().unwrap_or(path);
            entries.push_str(&format!(
                "    ({name:?}, include_bytes!({:?})),\n",
                absolute.to_string_lossy()
            ));
            names.push(name);
        }
    }
    if names.is_empty() {
        println!(
            "cargo:warning=No provider icons bundled (tracked sources missing in {})",
            tracked_directory.display()
        );
    }
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        output.join("bundled_provider_icons.rs"),
        format!(
            "/// Provider marks embedded at build time: (lookup name, file bytes).\npub const BUNDLED: &[(&str, &[u8])] = &[\n{entries}];\n"
        ),
    )
    .expect("write bundled_provider_icons.rs");
}

/// Embeds the static AUM mark as icon resource 1 without `rc.exe`: the
/// compiled resource (`.res`) format is written directly and handed to the
/// MSVC linker, which converts it with its bundled `cvtres`.
fn embed_windows_icon() -> Result<(), String> {
    let output = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?);
    let frames: Vec<(u32, Vec<u8>)> = ICON_SIZES
        .into_iter()
        .map(|size| (size, icon_dib(size)))
        .collect();

    // Loose .ico for the runtime fallback path in tray.rs (AUM_ICON_PATH).
    let icon = output.join("aum-app.ico");
    std::fs::write(&icon, ico_file(&frames)).map_err(|error| error.to_string())?;
    println!("cargo:rustc-env=AUM_ICON_PATH={}", icon.display());

    let resource = output.join("aum-icon.res");
    std::fs::write(&resource, res_file(&frames)).map_err(|error| error.to_string())?;
    println!(
        "cargo:rustc-link-arg-bin=agent-usage-monitor={}",
        resource.display()
    );
    Ok(())
}

/// One 32-bit icon image: BITMAPINFOHEADER (double height), bottom-up BGRA
/// pixels, then an all-zero AND mask (alpha carries the transparency).
fn icon_dib(size: u32) -> Vec<u8> {
    let rgba = aum_icon::render_rgba(size);
    let mask_stride = size.div_ceil(32) * 4;
    let image_size = size * size * 4;
    let mut dib = Vec::with_capacity((40 + image_size + mask_stride * size) as usize);
    push_u32(&mut dib, 40);
    push_u32(&mut dib, size);
    push_u32(&mut dib, size * 2);
    push_u16(&mut dib, 1);
    push_u16(&mut dib, 32);
    push_u32(&mut dib, 0);
    push_u32(&mut dib, image_size);
    for _ in 0..4 {
        push_u32(&mut dib, 0);
    }
    for row in (0..size).rev() {
        for column in 0..size {
            let offset = ((row * size + column) * 4) as usize;
            let [r, g, b, a] = [
                rgba[offset],
                rgba[offset + 1],
                rgba[offset + 2],
                rgba[offset + 3],
            ];
            dib.extend_from_slice(&[b, g, r, a]);
        }
    }
    dib.resize(dib.len() + (mask_stride * size) as usize, 0);
    dib
}

fn dimension_byte(size: u32) -> u8 {
    if size >= 256 { 0 } else { size as u8 }
}

fn ico_file(frames: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut ico = Vec::new();
    push_u16(&mut ico, 0);
    push_u16(&mut ico, 1);
    push_u16(&mut ico, frames.len() as u16);
    let mut offset = (6 + frames.len() * 16) as u32;
    for (size, frame) in frames {
        ico.push(dimension_byte(*size));
        ico.push(dimension_byte(*size));
        ico.extend_from_slice(&[0, 0]);
        push_u16(&mut ico, 1);
        push_u16(&mut ico, 32);
        push_u32(&mut ico, frame.len() as u32);
        push_u32(&mut ico, offset);
        offset += frame.len() as u32;
    }
    for (_, frame) in frames {
        ico.extend_from_slice(frame);
    }
    ico
}

/// Win32 compiled resource file: a null header entry, one RT_ICON per frame
/// (ids 1..n) and RT_GROUP_ICON 1 referencing them, which `LoadIconW(hinst, 1)`
/// and Explorer use.
fn res_file(frames: &[(u32, Vec<u8>)]) -> Vec<u8> {
    const RT_ICON: u16 = 3;
    const RT_GROUP_ICON: u16 = 14;
    const MOVEABLE_PURE_DISCARDABLE: u16 = 0x1030;
    const MOVEABLE_DISCARDABLE: u16 = 0x1010;

    let mut res = Vec::new();
    res_entry(&mut res, 0, 0, 0, &[]);
    let mut group = Vec::new();
    push_u16(&mut group, 0);
    push_u16(&mut group, 1);
    push_u16(&mut group, frames.len() as u16);
    for (index, (size, frame)) in frames.iter().enumerate() {
        let id = index as u16 + 1;
        res_entry(&mut res, RT_ICON, id, MOVEABLE_DISCARDABLE, frame);
        group.push(dimension_byte(*size));
        group.push(dimension_byte(*size));
        group.extend_from_slice(&[0, 0]);
        push_u16(&mut group, 1);
        push_u16(&mut group, 32);
        push_u32(&mut group, frame.len() as u32);
        push_u16(&mut group, id);
    }
    res_entry(
        &mut res,
        RT_GROUP_ICON,
        1,
        MOVEABLE_PURE_DISCARDABLE,
        &group,
    );
    res
}

/// RESOURCEHEADER with ordinal type and name (32 bytes), data, DWORD padding.
fn res_entry(res: &mut Vec<u8>, type_id: u16, name_id: u16, flags: u16, data: &[u8]) {
    push_u32(res, data.len() as u32);
    push_u32(res, 32);
    push_u16(res, 0xFFFF);
    push_u16(res, type_id);
    push_u16(res, 0xFFFF);
    push_u16(res, name_id);
    push_u32(res, 0); // DataVersion
    push_u16(res, flags);
    push_u16(res, 0); // LANG_NEUTRAL
    push_u32(res, 0); // Version
    push_u32(res, 0); // Characteristics
    res.extend_from_slice(data);
    while res.len() % 4 != 0 {
        res.push(0);
    }
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

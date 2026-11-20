use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    slint_build::compile("ui/main.slint").expect("Slint UI compilation failed");
    println!("cargo:rerun-if-changed=build.rs");

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

fn embed_windows_icon() -> Result<(), String> {
    let output = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?);
    let icon = output.join("aum-two-column.ico");
    std::fs::write(&icon, build_icon()).map_err(|error| error.to_string())?;
    let resource = output.join("aum-icon.rc");
    let resource_path = icon.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(&resource, format!("1 ICON \"{resource_path}\"\r\n"))
        .map_err(|error| error.to_string())?;

    let compiler = find_resource_compiler().ok_or("Windows SDK rc.exe was not found")?;
    let object = output.join("aum-icon.res");
    let mut child = Command::new(compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&object)
        .arg(&resource)
        .current_dir(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not start rc.exe: {error}"))?;
    let status = wait_child(&mut child, Duration::from_secs(15))
        .map_err(|error| format!("resource compilation failed: {error}"))?;
    if !status.success() {
        return Err(format!("rc.exe exited with {status}"));
    }
    println!(
        "cargo:rustc-link-arg-bin=agent-usage-monitor={}",
        object.display()
    );
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    Ok(())
}

fn find_resource_compiler() -> Option<PathBuf> {
    if let Some(sdk) = std::env::var_os("WindowsSdkDir") {
        let bin = PathBuf::from(sdk).join("bin");
        if let Some(candidate) = find_rc_in_sdk_bin(&bin) {
            return Some(candidate);
        }
    }
    if let Some(sdk) = std::env::var_os("WindowsKitsDir") {
        let candidate = PathBuf::from(sdk).join("10").join("bin");
        if let Some(found) = find_rc_in_sdk_bin(&candidate) {
            return Some(found);
        }
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join("rc.exe"))
        .find(|candidate| candidate.is_file())
}

fn find_rc_in_sdk_bin(bin: &std::path::Path) -> Option<PathBuf> {
    let mut versions: Vec<_> = std::fs::read_dir(bin)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    versions.sort();
    versions.into_iter().rev().find_map(|version| {
        ["x64", "x86", "arm64"]
            .into_iter()
            .map(|arch| version.join(arch).join("rc.exe"))
            .find(|candidate| candidate.is_file())
    })
}

fn wait_child(child: &mut Child, timeout: Duration) -> std::io::Result<ExitStatus> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "rc.exe exceeded its 15-second build limit",
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn build_icon() -> Vec<u8> {
    let sizes = [16u32, 20, 24, 32, 40, 48, 64, 128, 256];
    let mut frames = Vec::new();
    for size in sizes {
        let mask_stride = size.div_ceil(32) * 4;
        let image_size = size * size * 4;
        let mask_size = mask_stride * size;
        let mut dib = Vec::with_capacity((40 + image_size + mask_size) as usize);
        push_u32(&mut dib, 40);
        push_u32(&mut dib, size);
        push_u32(&mut dib, size * 2);
        push_u16(&mut dib, 1);
        push_u16(&mut dib, 32);
        push_u32(&mut dib, 0);
        push_u32(&mut dib, image_size);
        push_u32(&mut dib, 0);
        push_u32(&mut dib, 0);
        push_u32(&mut dib, 0);
        push_u32(&mut dib, 0);

        let column_width = (size / 4).max(2);
        let gap = (size / 8).max(2);
        let top = (size / 8).max(2);
        let bottom = size - top;
        let x_start = (size - (column_width * 2 + gap)) / 2;
        let mut pixels = vec![[0u8; 4]; (size * size) as usize];
        for column in 0..2u32 {
            let x0 = x_start + column * (column_width + gap);
            for y in top..bottom {
                for x in x0..(x0 + column_width) {
                    let edge = x == x0 || x + 1 == x0 + column_width || y == top || y + 1 == bottom;
                    let color = if edge {
                        [219, 228, 239, 255]
                    } else {
                        [90, 126, 168, 255]
                    };
                    pixels[(y * size + x) as usize] = [color[2], color[1], color[0], color[3]];
                }
            }
        }
        for row in (0..size).rev() {
            for pixel in 0..size {
                dib.extend_from_slice(&pixels[(row * size + pixel) as usize]);
            }
        }
        dib.resize(dib.len() + mask_size as usize, 0);
        frames.push(dib);
    }

    let header_size = 6 + frames.len() * 16;
    let mut ico = Vec::new();
    push_u16(&mut ico, 0);
    push_u16(&mut ico, 1);
    push_u16(&mut ico, frames.len() as u16);
    let mut offset = header_size as u32;
    for (size, frame) in sizes.into_iter().zip(&frames) {
        ico.push(if size == 256 { 0 } else { size as u8 });
        ico.push(if size == 256 { 0 } else { size as u8 });
        ico.push(0);
        ico.push(0);
        push_u16(&mut ico, 1);
        push_u16(&mut ico, 32);
        push_u32(&mut ico, frame.len() as u32);
        push_u32(&mut ico, offset);
        offset += frame.len() as u32;
    }
    for frame in frames {
        ico.extend_from_slice(&frame);
    }
    ico
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

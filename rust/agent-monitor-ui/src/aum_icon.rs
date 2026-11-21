//! The static AUM application mark (design spec v4, "Іконка програми (exe)").
//!
//! 256-unit tile with radius 56 and a #252F3E → #0F141B gradient; bars at
//! x 58 / 146, width 52, y 52..204, radius 15; fills 80% green #5FBF8F and
//! ~46% amber #D6A84F. At 16 px and below the tile is dropped and only the
//! bars remain, as the spec requires.
//!
//! This file has no dependencies so `build.rs` can include it (EXE icon
//! resource) and the runtime can use it (window icon, notification identity).
//! It is the application identity, never the dynamic quota tray indicator.

/// Renders the mark as straight-alpha RGBA rows, top row first.
pub fn render_rgba(size: u32) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let bars_only = size <= 16;
    // Bars-only frames crop to the bar area (centre 128, ±88 units) so the
    // bars fill the small icon instead of floating inside an empty tile.
    let (origin, span) = if bars_only {
        (40.0, 176.0)
    } else {
        (0.0, 256.0)
    };
    let scale = size as f32 / span;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0.0f32; 3];
            let mut alpha_sum = 0.0f32;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = origin + (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / scale;
                    let py = origin + (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / scale;
                    let [r, g, b, a] = sample(px, py, bars_only);
                    sum[0] += r * a;
                    sum[1] += g * a;
                    sum[2] += b * a;
                    alpha_sum += a;
                }
            }
            let offset = ((y * size + x) * 4) as usize;
            if alpha_sum > 0.0 {
                for channel in 0..3 {
                    rgba[offset + channel] = (sum[channel] / alpha_sum).round() as u8;
                }
            }
            rgba[offset + 3] = (alpha_sum / (SAMPLES * SAMPLES) as f32 * 255.0).round() as u8;
        }
    }
    rgba
}

fn sample(px: f32, py: f32, bars_only: bool) -> [f32; 4] {
    let t = (py / 256.0).clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let tile = [lerp(37.0, 15.0), lerp(47.0, 20.0), lerp(62.0, 27.0)];
    if !bars_only && !in_rounded_rect(px, py, 0.0, 0.0, 256.0, 256.0, 56.0) {
        return [0.0; 4];
    }
    for (left, fill, fill_color) in [
        (58.0, 0.80, [95.0, 191.0, 143.0]),
        (146.0, 0.46, [214.0, 168.0, 79.0]),
    ] {
        if in_rounded_rect(px, py, left, 52.0, left + 52.0, 204.0, 15.0) {
            if py >= 204.0 - 152.0 * fill {
                return [fill_color[0], fill_color[1], fill_color[2], 1.0];
            }
            // Track: white at 18% over the tile colour (also used opaque in
            // the bars-only frame so the track stays visible on any taskbar).
            let track = tile.map(|channel| channel + (255.0 - channel) * 0.18);
            return [track[0], track[1], track[2], 1.0];
        }
    }
    if bars_only {
        [0.0; 4]
    } else {
        [tile[0], tile[1], tile[2], 1.0]
    }
}

fn in_rounded_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) -> bool {
    if px < x0 || px > x1 || py < y0 || py > y1 {
        return false;
    }
    let dx = (x0 + radius - px).max(px - (x1 - radius)).max(0.0);
    let dy = (y0 + radius - py).max(py - (y1 - radius)).max(0.0);
    dx * dx + dy * dy <= radius * radius
}

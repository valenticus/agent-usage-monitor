use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// Largest accepted source file (SVG or PNG).
pub const MAX_ICON_BYTES: u64 = 2 * 1024 * 1024;

/// Every file name the local `provider-icons` folder is searched for, each
/// once (used by the Store build's optional settings import).
pub fn all_candidate_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for provider in [Provider::Codex, Provider::Claude] {
        for light in [false, true] {
            for name in candidates(provider, light) {
                if !names.contains(name) {
                    names.push(name);
                }
            }
        }
    }
    names
}
/// Largest accepted raster source edge, to bound decode memory.
const MAX_SOURCE_EDGE: u32 = 4096;
/// Raster sources are trimmed and kept at most this large after loading.
const MASTER_EDGE: u32 = 256;
/// SVGs are rasterized with their longer canvas side at this many px before
/// trimming, so even artwork that fills only a quarter of its canvas still
/// yields a full `MASTER_EDGE` master.
const SVG_RENDER_EDGE: u32 = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Provider {
    Codex,
    Claude,
}

/// Lookup order per provider and background. The first readable, decodable
/// file wins. Existing user-provided icon files remain supported as fallback;
/// tracked original SVGs supply clean-checkout defaults.
/// `ClaudeIcon-Rounded.png` / `ClaudeIcon-Square.png` (app tiles) are never
/// candidates: the flyout shows the Claude Spark symbol only.
pub fn candidates(provider: Provider, light: bool) -> &'static [&'static str] {
    match (provider, light) {
        (Provider::Codex, false) => &[
            "codex-on-dark.png",
            "OAI_OpenAI-Blossom_White.png",
            "OAI_OpenAI-Blossom_White.svg",
            "codex-on-dark.svg",
            "codex.png",
            "codex.svg",
        ],
        (Provider::Codex, true) => &[
            "codex-on-light.png",
            "OAI_OpenAI-Blossom_Black.png",
            "OAI_OpenAI-Blossom_Black.svg",
            "codex-on-light.svg",
            "codex.png",
            "codex.svg",
        ],
        (Provider::Claude, false) => &[
            "claude-on-dark.png",
            "claude-on-dark.svg",
            "claude.png",
            "claude.svg",
            "Claude Spark - Clay.png",
            "Claude Spark - Clay.svg",
        ],
        (Provider::Claude, true) => &[
            "claude-on-light.png",
            "claude-on-light.svg",
            "claude.png",
            "claude.svg",
            "Claude Spark - Clay.png",
            "Claude Spark - Clay.svg",
        ],
    }
}

/// A decoded source. SVGs and PNGs alike are trimmed to their visible
/// content, squared and kept as a small straight-alpha RGBA master for
/// exact-size downscaling. Trimming matters for SVGs too: the original OpenAI
/// Blossom artwork fills only 49.5 % of its 716-unit viewBox while the Claude
/// Spark fills 99.6 % of its 94-unit one, so untrimmed vectors at the same
/// box size showed the Codex mark at half the Claude mark's visible size.
#[derive(Clone)]
enum Source {
    Raster {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
}

/// Optional, user-supplied provider marks for dark backgrounds (Graphite,
/// Bluish Dark) and light backgrounds (Light).
#[derive(Default)]
pub struct ProviderIcons {
    sources: HashMap<(Provider, bool), Source>,
    scaled: RefCell<HashMap<(Provider, bool, u32), Image>>,
}

impl ProviderIcons {
    /// Image for `logical_size` px shown at `scale_factor`. Raster sources are
    /// resampled once per physical size and cached, so a frame never decodes
    /// or resizes; the result maps 1:1 to device pixels at 100–200 % scaling.
    pub fn icon(
        &self,
        provider: Provider,
        light: bool,
        logical_size: f32,
        scale_factor: f32,
    ) -> Option<Image> {
        match self.sources.get(&(provider, light))? {
            Source::Raster {
                rgba,
                width,
                height,
            } => {
                let physical = (logical_size * scale_factor.max(0.5))
                    .round()
                    .clamp(1.0, 512.0) as u32;
                let key = (provider, light, physical);
                if let Some(image) = self.scaled.borrow().get(&key) {
                    return Some(image.clone());
                }
                let pixels = resample_area(rgba, *width, *height, physical, physical);
                let buffer =
                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&pixels, physical, physical);
                let image = Image::from_rgba8(buffer);
                self.scaled.borrow_mut().insert(key, image.clone());
                Some(image)
            }
        }
    }

    #[cfg(test)]
    fn has(&self, provider: Provider, light: bool) -> bool {
        self.sources.contains_key(&(provider, light))
    }
}

thread_local! {
    static LOCAL_ICONS: RefCell<Option<std::rc::Rc<ProviderIcons>>> = const { RefCell::new(None) };
}

/// Provider marks embedded into the executable by `build.rs` from tracked
/// original SVGs and optional user-supplied build overrides.
mod bundled {
    include!(concat!(env!("OUT_DIR"), "/bundled_provider_icons.rs"));
}

/// Where a provider mark was loaded from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Bundled,
    LocalFolder,
}

/// Provider marks, loaded once per process (UI thread). Each slot (provider ×
/// background) takes the first candidate embedded in the executable; only a
/// slot without an embedded file falls back to the user's local
/// `%APPDATA%\Agent Usage Monitor\provider-icons` folder. Nothing is downloaded.
pub fn local() -> std::rc::Rc<ProviderIcons> {
    LOCAL_ICONS.with(|cached| {
        cached
            .borrow_mut()
            .get_or_insert_with(|| {
                // Without persistent storage only the embedded marks are used.
                let directory = crate::settings::settings_directory()
                    .map(|directory| directory.join("provider-icons"));
                let (icons, origins) = load_icons_from(bundled::BUNDLED, directory.as_deref());
                for ((provider, light), (name, origin)) in &origins {
                    eprintln!(
                        "PROVIDER_ICONS: {provider:?} {} source={origin:?} file={name}",
                        if *light { "light" } else { "dark" }
                    );
                }
                std::rc::Rc::new(icons)
            })
            .clone()
    })
}

type Slot = (Provider, bool);
/// Which file (and from where) filled each slot.
type SlotOrigins = Vec<(Slot, (&'static str, Origin))>;

#[cfg(test)]
fn load_icons(embedded: &[(&str, &[u8])], directory: &Path) -> (ProviderIcons, SlotOrigins) {
    load_icons_from(embedded, Some(directory))
}

fn load_icons_from(
    embedded: &[(&str, &[u8])],
    directory: Option<&Path>,
) -> (ProviderIcons, SlotOrigins) {
    let mut sources = HashMap::new();
    let mut origins = Vec::new();
    for provider in [Provider::Codex, Provider::Claude] {
        for light in [false, true] {
            let names = candidates(provider, light);
            let found = names
                .iter()
                .find_map(|name| {
                    Some((
                        load_source(&directory?.join(name))?,
                        (*name, Origin::LocalFolder),
                    ))
                })
                .or_else(|| {
                    names.iter().find_map(|name| {
                        let (_, bytes) = embedded.iter().find(|(bundled, _)| bundled == name)?;
                        Some((load_embedded_source(name, bytes)?, (*name, Origin::Bundled)))
                    })
                });
            if let Some((source, origin)) = found {
                let source = with_optical_scale(source, optical_scale(provider));
                sources.insert((provider, light), source);
                origins.push(((provider, light), origin));
            }
        }
    }
    (
        ProviderIcons {
            sources,
            scaled: RefCell::default(),
        },
        origins,
    )
}

/// Share of the icon box the trimmed artwork may fill. Claude is the visual
/// reference (1.0). The OpenAI Blossom is a dense closed ring whose visible
/// edge runs all the way round, so at the same trimmed size it reads larger
/// and heavier than the open Claude Spark; it is drawn at 90 % of the box.
fn optical_scale(provider: Provider) -> f32 {
    match provider {
        Provider::Codex => 0.90,
        Provider::Claude => 1.0,
    }
}

/// Centres the trimmed master in a larger transparent square so the artwork
/// fills `scale` of the box. The padding is equal on every side (the margin
/// is rounded to whole master pixels), so area resampling keeps the symbol
/// centred at every physical size. A scale of 1.0 returns the source as is.
fn with_optical_scale(source: Source, scale: f32) -> Source {
    let Source::Raster {
        rgba,
        width,
        height,
    } = source;
    if !(scale > 0.0 && scale < 1.0) {
        return Source::Raster {
            rgba,
            width,
            height,
        };
    }
    let margin = ((width as f32 / scale - width as f32) / 2.0).round() as u32;
    let edge = width + 2 * margin;
    let mut padded = vec![0u8; (edge * edge * 4) as usize];
    for y in 0..height {
        let source = (y * width * 4) as usize;
        let target = (((margin + y) * edge + margin) * 4) as usize;
        let length = (width * 4) as usize;
        padded[target..target + length].copy_from_slice(&rgba[source..source + length]);
    }
    Source::Raster {
        rgba: padded,
        width: edge,
        height: edge,
    }
}

#[cfg(test)]
fn load_from_directory(directory: &Path) -> ProviderIcons {
    load_icons(&[], directory).0
}

fn load_source(path: &Path) -> Option<Source> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_ICON_BYTES {
        return None;
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "svg" => load_svg_bytes(&std::fs::read(path).ok()?),
        "png" => load_png(path),
        _ => None,
    }
}

fn load_embedded_source(name: &str, bytes: &[u8]) -> Option<Source> {
    if bytes.len() as u64 > MAX_ICON_BYTES {
        return None;
    }
    match Path::new(name)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "svg" => load_svg_bytes(bytes),
        "png" => load_png_bytes(bytes),
        _ => None,
    }
}

/// Renders an SVG (file bytes are never modified) with its longer canvas side
/// at `SVG_RENDER_EDGE` px, aspect ratio preserved, then runs the pixels
/// through the PNG trim and resampling pipeline.
fn load_svg_bytes(bytes: &[u8]) -> Option<Source> {
    use resvg::{tiny_skia, usvg};
    if bytes.len() as u64 > MAX_ICON_BYTES {
        return None;
    }
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let longest = size.width().max(size.height());
    if !(longest.is_finite() && longest > 0.0) {
        return None;
    }
    let scale = SVG_RENDER_EDGE as f32 / longest;
    let width = ((size.width() * scale).ceil() as u32).clamp(1, SVG_RENDER_EDGE);
    let height = ((size.height() * scale).ceil() as u32).clamp(1, SVG_RENDER_EDGE);
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        rgba.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }
    raster_from_rgba(rgba, width, height)
}

fn load_png(path: &Path) -> Option<Source> {
    let image = Image::load_from_path(path).ok()?;
    let size = image.size();
    if size.width == 0
        || size.height == 0
        || size.width > MAX_SOURCE_EDGE
        || size.height > MAX_SOURCE_EDGE
    {
        return None;
    }
    let buffer = image.to_rgba8()?;
    let (width, height) = (buffer.width(), buffer.height());
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for pixel in buffer.as_slice() {
        rgba.extend_from_slice(&[pixel.r, pixel.g, pixel.b, pixel.a]);
    }
    raster_from_rgba(rgba, width, height)
}

/// Decodes an embedded PNG to straight-alpha RGBA8 (same pixels as the file
/// path above) and runs it through the same trim and resampling pipeline.
fn load_png_bytes(bytes: &[u8]) -> Option<Source> {
    if bytes.len() as u64 > MAX_ICON_BYTES {
        return None;
    }
    let (rgba, width, height) = decode_png_rgba(bytes)?;
    // Slint keeps decoded files premultiplied and un-premultiplies them in
    // `to_rgba8()`. Running the embedded pixels through the same public
    // conversions gives bit-identical input to `load_png` (same appearance).
    let straight = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, width, height);
    let premultiplied = Image::from_rgba8(straight).to_rgba8_premultiplied()?;
    let buffer = Image::from_rgba8_premultiplied(premultiplied).to_rgba8()?;
    let mut rgba = Vec::with_capacity(rgba.len());
    for pixel in buffer.as_slice() {
        rgba.extend_from_slice(&[pixel.r, pixel.g, pixel.b, pixel.a]);
    }
    raster_from_rgba(rgba, width, height)
}

fn decode_png_rgba(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width == 0 || height == 0 || width > MAX_SOURCE_EDGE || height > MAX_SOURCE_EDGE {
        return None;
    }
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buffer).ok()?;
    let data = &buffer[..frame.buffer_size()];
    let rgba = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((rgba, width, height))
}

/// Shared raster pipeline: trim transparent margins to a centred square and
/// keep a master of at most `MASTER_EDGE` px.
fn raster_from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Option<Source> {
    let (rgba, edge) = trim_to_square(&rgba, width, height)?;
    let master = edge.min(MASTER_EDGE);
    let rgba = if master == edge {
        rgba
    } else {
        resample_area(&rgba, edge, edge, master, master)
    };
    Some(Source::Raster {
        rgba,
        width: master,
        height: master,
    })
}

/// Crops transparent margins (alpha 0) and centres the content in the
/// smallest enclosing square, so marks with different padding render at the
/// same visible size and keep their aspect ratio. `None` if fully transparent.
fn trim_to_square(rgba: &[u8], width: u32, height: u32) -> Option<(Vec<u8>, u32)> {
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0u32, 0u32);
    for y in 0..height {
        for x in 0..width {
            if rgba[((y * width + x) * 4 + 3) as usize] > 0 {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    if right <= left || bottom <= top {
        return None;
    }
    let (content_w, content_h) = (right - left, bottom - top);
    let edge = content_w.max(content_h);
    let offset_x = (edge - content_w) / 2;
    let offset_y = (edge - content_h) / 2;
    let mut square = vec![0u8; (edge * edge * 4) as usize];
    for y in 0..content_h {
        let source = (((top + y) * width + left) * 4) as usize;
        let target = (((offset_y + y) * edge + offset_x) * 4) as usize;
        let length = (content_w * 4) as usize;
        square[target..target + length].copy_from_slice(&rgba[source..source + length]);
    }
    Some((square, edge))
}

/// Area-average (box) resampling with premultiplied alpha: every destination
/// pixel is the coverage-weighted mean of the source pixels under it, which
/// keeps thin strokes and antialiased edges clean when downscaling.
fn resample_area(rgba: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let weights = |source: u32, target: u32| -> Vec<Vec<(usize, f32)>> {
        let ratio = source as f32 / target as f32;
        (0..target)
            .map(|index| {
                let start = index as f32 * ratio;
                let end = start + ratio;
                let mut list = Vec::new();
                let mut position = start.floor() as u32;
                while (position as f32) < end && position < source {
                    let overlap =
                        (end.min(position as f32 + 1.0) - start.max(position as f32)).max(0.0);
                    if overlap > 0.0 {
                        list.push((position as usize, overlap / ratio));
                    }
                    position += 1;
                }
                list
            })
            .collect()
    };
    let columns = weights(sw, dw);
    let rows = weights(sh, dh);
    let mut output = vec![0u8; (dw * dh * 4) as usize];
    for (dy, row_weights) in rows.iter().enumerate() {
        for (dx, column_weights) in columns.iter().enumerate() {
            let mut sum = [0.0f32; 4];
            for &(sy, wy) in row_weights {
                for &(sx, wx) in column_weights {
                    let offset = (sy * sw as usize + sx) * 4;
                    let weight = wx * wy;
                    let alpha = rgba[offset + 3] as f32 * weight;
                    sum[0] += rgba[offset] as f32 * alpha;
                    sum[1] += rgba[offset + 1] as f32 * alpha;
                    sum[2] += rgba[offset + 2] as f32 * alpha;
                    sum[3] += alpha;
                }
            }
            let offset = (dy * dw as usize + dx) * 4;
            if sum[3] > 0.0 {
                for channel in 0..3 {
                    output[offset + channel] =
                        (sum[channel] / sum[3]).round().clamp(0.0, 255.0) as u8;
                }
            }
            output[offset + 3] = sum[3].round().clamp(0.0, 255.0) as u8;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = "<!-- AUM GENERIC TEST ASSET; not a provider mark --><svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 2 2\"><rect width=\"2\" height=\"2\" fill=\"#12ab34\"/></svg>";

    /// Writes a generic (non-provider) PNG: an opaque `color` square of
    /// `content` px centred in a transparent `size` px canvas.
    fn write_png(path: &Path, size: u32, content: u32, color: [u8; 3]) {
        let start = (size - content) / 2;
        let mut data = vec![0u8; (size * size * 4) as usize];
        for y in start..start + content {
            for x in start..start + content {
                let offset = ((y * size + x) * 4) as usize;
                data[offset..offset + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&data)
            .unwrap();
    }

    fn pixel(image: &Image, x: u32, y: u32) -> Rgba8Pixel {
        let buffer = image.to_rgba8().unwrap();
        buffer.as_slice()[(y * buffer.width() + x) as usize]
    }

    #[test]
    fn svg_files_still_load_with_theme_variants_and_fallback() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("codex-on-dark.svg"), SVG).unwrap();
        std::fs::write(directory.path().join("codex-on-light.svg"), SVG).unwrap();
        std::fs::write(directory.path().join("claude.svg"), SVG).unwrap();
        let icons = load_from_directory(directory.path());
        for light in [false, true] {
            assert!(icons.icon(Provider::Codex, light, 20.0, 1.0).is_some());
            assert!(icons.icon(Provider::Claude, light, 20.0, 1.0).is_some());
        }
    }

    /// Visible (alpha > 0) bounds of an icon as (left, top, right, bottom).
    fn visible_bounds(image: &Image) -> (u32, u32, u32, u32) {
        let buffer = image.to_rgba8().unwrap();
        let width = buffer.width();
        let (mut bounds, mut any) = ((u32::MAX, u32::MAX, 0, 0), false);
        for (index, pixel) in buffer.as_slice().iter().enumerate() {
            if pixel.a > 0 {
                let (x, y) = (index as u32 % width, index as u32 / width);
                bounds = (
                    bounds.0.min(x),
                    bounds.1.min(y),
                    bounds.2.max(x + 1),
                    bounds.3.max(y + 1),
                );
                any = true;
            }
        }
        assert!(any, "icon has visible pixels");
        bounds
    }

    /// Linear fill of the box (sqrt of alpha coverage) and the horizontal and
    /// vertical balance of the ink (left − right, top − bottom halves, as a
    /// share of all ink). A full opaque square gives (1.0, 0.0, 0.0).
    fn fill_and_balance(image: &Image) -> (f32, f32, f32) {
        let buffer = image.to_rgba8().unwrap();
        let (width, height) = (buffer.width(), buffer.height());
        let (mut total, mut horizontal, mut vertical) = (0.0f32, 0.0f32, 0.0f32);
        for (index, pixel) in buffer.as_slice().iter().enumerate() {
            let alpha = pixel.a as f32 / 255.0;
            let (x, y) = (index as u32 % width, index as u32 / width);
            total += alpha;
            horizontal += if x * 2 + 1 < width {
                alpha
            } else if x * 2 + 1 > width {
                -alpha
            } else {
                0.0
            };
            vertical += if y * 2 + 1 < height {
                alpha
            } else if y * 2 + 1 > height {
                -alpha
            } else {
                0.0
            };
        }
        (
            (total / (width * height) as f32).sqrt(),
            horizontal / total,
            vertical / total,
        )
    }

    /// Claude is the visual reference at full size; Codex alone is drawn at
    /// 90 % of the same box, centred, at every container size, scale and theme.
    #[test]
    fn codex_has_provider_specific_optical_scale_and_claude_is_unchanged() {
        assert_eq!(optical_scale(Provider::Claude), 1.0);
        assert_eq!(optical_scale(Provider::Codex), 0.90);
        let directory = tempfile::tempdir().unwrap();
        // Generic full-bleed square in every slot (not a provider mark).
        for name in [
            "codex-on-dark.png",
            "codex-on-light.png",
            "claude-on-dark.png",
            "claude-on-light.png",
        ] {
            write_png(&directory.path().join(name), 256, 256, [255, 255, 255]);
        }
        let icons = load_from_directory(directory.path());
        for logical in [14.0f32, 16.0] {
            for scale in [1.0f32, 1.25, 1.5, 2.0] {
                let edge = (logical * scale).round() as u32;
                for light in [false, true] {
                    let claude = icons.icon(Provider::Claude, light, logical, scale).unwrap();
                    let codex = icons.icon(Provider::Codex, light, logical, scale).unwrap();
                    // Containers are unchanged: both images are exactly edge².
                    assert_eq!((claude.size().width, claude.size().height), (edge, edge));
                    assert_eq!((codex.size().width, codex.size().height), (edge, edge));
                    let label = format!("{logical}px@{scale} light={light}");
                    let (fill, _, _) = fill_and_balance(&claude);
                    assert!((fill - 1.0).abs() < 1e-3, "Claude {label}: fill {fill}");
                    let (fill, horizontal, vertical) = fill_and_balance(&codex);
                    assert!((fill - 0.90).abs() < 0.01, "Codex {label}: fill {fill}");
                    assert!(
                        horizontal.abs() < 1e-3,
                        "Codex {label}: off-centre x {horizontal}"
                    );
                    assert!(
                        vertical.abs() < 1e-3,
                        "Codex {label}: off-centre y {vertical}"
                    );
                }
            }
        }
    }

    #[test]
    fn svg_is_trimmed_to_visible_artwork_and_scaled_to_exact_physical_size() {
        let directory = tempfile::tempdir().unwrap();
        // Generic test asset: content is half the canvas, centred.
        std::fs::write(
            directory.path().join("claude-on-dark.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"40\" viewBox=\"0 0 4 4\"><rect x=\"1\" y=\"1\" width=\"2\" height=\"2\" fill=\"#ffffff\"/></svg>",
        )
        .unwrap();
        let icons = load_from_directory(directory.path());
        for (scale, expected) in [(1.0, 14), (1.25, 18), (1.5, 21), (2.0, 28)] {
            let image = icons.icon(Provider::Claude, false, 14.0, scale).unwrap();
            assert_eq!(image.size().width, expected);
            assert_eq!(image.size().height, expected);
            assert_eq!(visible_bounds(&image), (0, 0, expected, expected));
            assert_eq!(pixel(&image, 0, 0).a, 255);
        }
    }

    /// The bundled Codex and Claude marks have very different viewBox
    /// padding. After trimming, the Claude Spark spans the full icon box and
    /// the Blossom sits centred inside it with only its optical inset, in the
    /// flyout (14 px) and Settings (16 px) at every scale.
    #[test]
    fn bundled_provider_marks_are_trimmed_and_centred() {
        let empty = tempfile::tempdir().unwrap();
        let (icons, _) = load_icons(bundled::BUNDLED, empty.path());
        for logical in [14.0f32, 16.0] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let edge = (logical * scale).round() as u32;
                for light in [false, true] {
                    let label = format!("light={light} {logical}px@{scale}");
                    let claude = icons.icon(Provider::Claude, light, logical, scale).unwrap();
                    let (left, top, right, bottom) = visible_bounds(&claude);
                    // Allow one device px of antialiasing slack per side.
                    assert!(
                        left <= 1 && top <= 1 && right + 1 >= edge && bottom + 1 >= edge,
                        "Claude {label}: bounds {left},{top}-{right},{bottom} in {edge}"
                    );
                    let codex = icons.icon(Provider::Codex, light, logical, scale).unwrap();
                    let (left, top, right, bottom) = visible_bounds(&codex);
                    // 5 % inset per side (≤ 1.6 device px at 32 px) plus antialiasing.
                    assert!(
                        left <= 2 && top <= 2 && right + 2 >= edge && bottom + 2 >= edge,
                        "Codex {label}: bounds {left},{top}-{right},{bottom} in {edge}"
                    );
                    assert!(
                        left.abs_diff(edge - right) <= 1 && top.abs_diff(edge - bottom) <= 1,
                        "Codex {label}: not centred, bounds {left},{top}-{right},{bottom} in {edge}"
                    );
                }
            }
        }
    }

    #[test]
    fn png_is_trimmed_and_scaled_to_exact_physical_size() {
        let directory = tempfile::tempdir().unwrap();
        // Content is half the canvas, like the supplied Blossom files.
        write_png(
            &directory.path().join("claude-on-dark.png"),
            64,
            32,
            [255, 255, 255],
        );
        let icons = load_from_directory(directory.path());
        for (scale, expected) in [(1.0, 20), (1.25, 25), (1.5, 30), (2.0, 40)] {
            let image = icons.icon(Provider::Claude, false, 20.0, scale).unwrap();
            assert_eq!(image.size().width, expected);
            assert_eq!(image.size().height, expected);
            // Margins were trimmed: the corner is fully opaque content.
            assert_eq!(pixel(&image, 0, 0).a, 255);
        }
    }

    #[test]
    fn theme_selection_uses_dark_and_light_files_and_original_names() {
        let directory = tempfile::tempdir().unwrap();
        write_png(
            &directory.path().join("OAI_OpenAI-Blossom_White.png"),
            16,
            8,
            [255, 255, 255],
        );
        write_png(
            &directory.path().join("OAI_OpenAI-Blossom_Black.png"),
            16,
            8,
            [0, 0, 0],
        );
        write_png(
            &directory.path().join("claude-on-dark.png"),
            16,
            16,
            [255, 255, 255],
        );
        write_png(
            &directory.path().join("claude-on-light.png"),
            16,
            16,
            [17, 17, 17],
        );
        // App tiles must never be picked for the flyout.
        write_png(
            &directory.path().join("ClaudeIcon-Rounded.png"),
            16,
            16,
            [217, 119, 87],
        );
        let icons = load_from_directory(directory.path());
        let color = |provider, light| {
            let p = pixel(&icons.icon(provider, light, 4.0, 1.0).unwrap(), 2, 2);
            [p.r, p.g, p.b]
        };
        assert_eq!(color(Provider::Codex, false), [255, 255, 255]);
        assert_eq!(color(Provider::Codex, true), [0, 0, 0]);
        assert_eq!(color(Provider::Claude, false), [255, 255, 255]);
        assert_eq!(color(Provider::Claude, true), [17, 17, 17]);
    }

    #[test]
    fn theme_specific_png_wins_over_original_and_svg() {
        let directory = tempfile::tempdir().unwrap();
        write_png(
            &directory.path().join("codex-on-dark.png"),
            8,
            8,
            [10, 20, 30],
        );
        write_png(
            &directory.path().join("OAI_OpenAI-Blossom_White.png"),
            8,
            8,
            [255, 255, 255],
        );
        std::fs::write(directory.path().join("codex-on-dark.svg"), SVG).unwrap();
        let icons = load_from_directory(directory.path());
        let p = pixel(&icons.icon(Provider::Codex, false, 4.0, 1.0).unwrap(), 1, 1);
        assert_eq!([p.r, p.g, p.b], [10, 20, 30]);
    }

    #[test]
    fn missing_corrupted_oversized_and_blank_files_are_ignored() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("codex-on-dark.png"), b"not a png").unwrap();
        std::fs::write(
            directory.path().join("claude-on-dark.png"),
            vec![0u8; MAX_ICON_BYTES as usize + 1],
        )
        .unwrap();
        write_png(
            &directory.path().join("claude-on-light.png"),
            8,
            0,
            [0, 0, 0],
        );
        // A corrupt preferred file falls through to the next valid candidate.
        std::fs::write(directory.path().join("codex-on-light.svg"), SVG).unwrap();
        let icons = load_from_directory(directory.path());
        assert!(!icons.has(Provider::Codex, false));
        assert!(!icons.has(Provider::Claude, false));
        assert!(!icons.has(Provider::Claude, true));
        assert!(icons.has(Provider::Codex, true));

        let empty = load_from_directory(&directory.path().join("missing"));
        assert!(empty.icon(Provider::Codex, false, 20.0, 1.0).is_none());
    }

    #[test]
    fn scaled_images_are_cached_per_physical_size() {
        let directory = tempfile::tempdir().unwrap();
        write_png(
            &directory.path().join("claude-on-dark.png"),
            32,
            32,
            [255, 255, 255],
        );
        let icons = load_from_directory(directory.path());
        let first = icons.icon(Provider::Claude, false, 20.0, 1.5).unwrap();
        let second = icons.icon(Provider::Claude, false, 20.0, 1.5).unwrap();
        assert_eq!(first, second);
        assert_eq!(icons.scaled.borrow().len(), 1);
        icons.icon(Provider::Claude, false, 20.0, 2.0).unwrap();
        assert_eq!(icons.scaled.borrow().len(), 2);
    }

    fn png_bytes(size: u32, color: [u8; 3]) -> Vec<u8> {
        let data: Vec<u8> = (0..size * size)
            .flat_map(|_| [color[0], color[1], color[2], 255])
            .collect();
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&data)
            .unwrap();
        bytes
    }

    #[test]
    fn local_icons_override_bundled_defaults_and_bundled_icons_fill_missing_slots() {
        let directory = tempfile::tempdir().unwrap();
        write_png(
            &directory.path().join("claude-on-dark.png"),
            8,
            8,
            [1, 2, 3],
        );
        write_png(
            &directory.path().join("claude-on-light.png"),
            8,
            8,
            [4, 5, 6],
        );
        let codex_white = png_bytes(8, [255, 255, 255]);
        let claude_dark = png_bytes(8, [200, 100, 50]);
        let embedded: [(&str, &[u8]); 2] = [
            ("OAI_OpenAI-Blossom_White.png", &codex_white),
            ("claude-on-dark.png", &claude_dark),
        ];
        let (icons, origins) = load_icons(&embedded, directory.path());
        let color = |provider, light| {
            let p = pixel(&icons.icon(provider, light, 4.0, 1.0).unwrap(), 1, 1);
            [p.r, p.g, p.b]
        };
        assert_eq!(color(Provider::Codex, false), [255, 255, 255]);
        assert_eq!(color(Provider::Claude, false), [1, 2, 3]);
        assert_eq!(color(Provider::Claude, true), [4, 5, 6]);
        assert!(!icons.has(Provider::Codex, true));
        let origin = |slot| origins.iter().find(|(s, _)| *s == slot).map(|(_, o)| o.1);
        assert_eq!(origin((Provider::Codex, false)), Some(Origin::Bundled));
        assert_eq!(origin((Provider::Claude, false)), Some(Origin::LocalFolder));
        assert_eq!(origin((Provider::Claude, true)), Some(Origin::LocalFolder));
    }

    #[test]
    fn corrupted_embedded_bytes_are_ignored() {
        let directory = tempfile::tempdir().unwrap();
        let embedded: [(&str, &[u8]); 1] = [("claude-on-dark.png", b"not a png")];
        let (icons, _) = load_icons(&embedded, directory.path());
        assert!(!icons.has(Provider::Claude, false));
    }

    /// A clean checkout embeds the original SVG files, and all four provider
    /// slots resolve from those bytes without the user's local icon folder.
    #[test]
    fn tracked_svg_assets_match_file_rendering_and_fill_all_slots() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("providers");
        for name in [
            "OAI_OpenAI-Blossom_White.svg",
            "OAI_OpenAI-Blossom_Black.svg",
            "Claude Spark - Clay.svg",
        ] {
            let (_, bytes) = bundled::BUNDLED
                .iter()
                .find(|(bundled_name, _)| *bundled_name == name)
                .expect("tracked SVG is embedded");
            let from_bytes = Image::load_from_svg_data(bytes).expect("embedded SVG decodes");
            let path = assets.join(name);
            let from_file = Image::load_from_path(&path).expect("source SVG loads");
            assert_eq!(from_bytes.size(), from_file.size(), "{name}: size differs");
            assert_eq!(
                from_bytes.to_rgba8().unwrap().as_slice(),
                from_file.to_rgba8().unwrap().as_slice(),
                "{name}: embedded render differs from source file"
            );
        }
        let empty = tempfile::tempdir().unwrap();
        let (icons, origins) = load_icons(bundled::BUNDLED, empty.path());
        for provider in [Provider::Codex, Provider::Claude] {
            for light in [false, true] {
                assert!(icons.has(provider, light), "{provider:?} light={light}");
            }
        }
        assert!(
            origins
                .iter()
                .all(|(_, (_, origin))| *origin == Origin::Bundled)
        );
        assert_eq!(
            origins
                .iter()
                .find(|(slot, _)| *slot == (Provider::Codex, false))
                .map(|(_, (name, _))| *name),
            Some("OAI_OpenAI-Blossom_White.svg")
        );
        assert_eq!(
            origins
                .iter()
                .find(|(slot, _)| *slot == (Provider::Codex, true))
                .map(|(_, (name, _))| *name),
            Some("OAI_OpenAI-Blossom_Black.svg")
        );
        assert_eq!(
            origins
                .iter()
                .find(|(slot, _)| *slot == (Provider::Claude, false))
                .map(|(_, (name, _))| *name),
            Some("Claude Spark - Clay.svg")
        );
    }

    #[test]
    fn area_resampling_keeps_partial_alpha_edges_clean() {
        // 2x1 → 1x1: white opaque + fully transparent black → white at 50 %.
        let source = [255, 255, 255, 255, 0, 0, 0, 0];
        assert_eq!(resample_area(&source, 2, 1, 1, 1), vec![255, 255, 255, 128]);
    }
}

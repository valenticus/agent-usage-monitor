use std::cell::RefCell;
use std::path::Path;

use slint::Image;

const MAX_ICON_BYTES: u64 = 256 * 1024;

#[derive(Clone, Default)]
pub struct ProviderIcons {
    pub codex: Option<Image>,
    pub claude: Option<Image>,
}

thread_local! {
    static LOCAL_ICONS: RefCell<Option<ProviderIcons>> = const { RefCell::new(None) };
}

/// Loads only user-supplied SVG files from AUM's data directory. No provider
/// artwork is bundled or downloaded by the application.
pub fn local() -> ProviderIcons {
    LOCAL_ICONS.with(|cached| {
        cached
            .borrow_mut()
            .get_or_insert_with(|| {
                let directory = crate::settings::settings_directory().join("provider-icons");
                ProviderIcons {
                    codex: load_svg(&directory.join("codex.svg")),
                    claude: load_svg(&directory.join("claude.svg")),
                }
            })
            .clone()
    })
}

fn load_svg(path: &Path) -> Option<Image> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_ICON_BYTES {
        return None;
    }
    Image::load_from_path(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_existing_small_svg_images() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("provider.svg");
        std::fs::write(&path, "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"><path d=\"M0 0h1v1H0z\"/></svg>").unwrap();
        assert!(load_svg(&path).is_some());

        std::fs::write(&path, vec![b' '; MAX_ICON_BYTES as usize + 1]).unwrap();
        assert!(load_svg(&path).is_none());
        assert!(load_svg(&directory.path().join("missing.svg")).is_none());
    }
}

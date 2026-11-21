# Local provider icon overrides

The clean-checkout defaults are the original SVG assets in the sibling `providers/` folder. `build.rs` embeds those tracked files automatically.

This `provider-icons/` directory is for optional local build overrides and remains git-ignored. Set `AUM_PROVIDER_ICONS_DIR` explicitly to add these files to a custom build. Without that setting, they do not affect the embedded defaults.

At runtime, user-provided PNG or SVG files in `%APPDATA%\Agent Usage Monitor\provider-icons\` override the bundled default for their matching provider/theme slot. Missing or invalid files fall back to the embedded SVG. AUM does not modify user files.

Provider logos remain trademarks of OpenAI and Anthropic. They are not covered by AUM's MIT license; see `docs/design/brand-assets.md`.

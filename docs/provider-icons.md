# Provider icons

AUM embeds the original SVG provider marks tracked in `rust/agent-monitor-ui/assets/providers/` into each application build. A clean Git checkout therefore contains the same default icons without local files or network access.

| Source file | Use |
| --- | --- |
| `OAI_OpenAI-Blossom_White.svg` | Codex card on Graphite and Bluish Dark |
| `OAI_OpenAI-Blossom_Black.svg` | Codex card on Light |
| `Claude Spark - Clay.svg` | Claude card on all themes |

The files are embedded byte-for-byte by `build.rs`. `AUM_PROVIDER_ICONS_DIR` can optionally supply extra files for a custom build. At runtime, AUM first checks the user's `%APPDATA%\Agent Usage Monitor\provider-icons\` folder, so user-provided icons override the embedded defaults. Missing or invalid user files fall back to the embedded assets. Neither path modifies the source SVGs.

Provider marks are third-party assets and trademarks. They are not covered by the repository's MIT license. See [`docs/design/brand-assets.md`](design/brand-assets.md) for provenance and separate trademark notices.

`ClaudeIcon-Rounded.svg` and `ClaudeIcon-Square.svg` (app tiles) are not used. The AUM application icon and dynamic tray indicator do not use provider marks.

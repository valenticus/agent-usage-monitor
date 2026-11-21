# Provider icons

AUM embeds the original SVG provider marks tracked in `rust/agent-monitor-ui/assets/providers/` into each application build. A clean Git checkout therefore contains the same default icons without local files or network access.

| Source file | Use |
| --- | --- |
| `OAI_OpenAI-Blossom_White.svg` | Codex card on Graphite and Bluish Dark |
| `OAI_OpenAI-Blossom_Black.svg` | Codex card on Light |
| `Claude Spark - Clay.svg` | Claude card on all themes |

The files are embedded byte-for-byte by `build.rs`. `AUM_PROVIDER_ICONS_DIR` can optionally supply extra files for a custom build. At runtime, AUM first checks the user's `%APPDATA%\Agent Usage Monitor\provider-icons\` folder, so user-provided icons override the embedded defaults. Missing or invalid user files fall back to the embedded assets. Neither path modifies the source SVGs.

## Sizing

Every source, embedded or user-provided, SVG or PNG, goes through the same pipeline in `rust/agent-monitor-ui/src/provider_icons.rs`:

1. **Visible-content normalization.** SVGs are rasterized with `resvg` (longer side 1024 px, aspect ratio kept); PNGs are decoded. Transparent margins are trimmed, and the visible artwork is centred in the smallest enclosing square. This matters because source files pad their artwork very differently: the OpenAI Blossom fills about 49.5 % of its viewBox, while the Claude Spark fills about 99.6 %.
2. **Per-provider optical scale.** The trimmed artwork is placed in the icon box at a provider-specific share of its size, with equal transparent padding on every side:

   | Provider | Optical scale | Why |
   | --- | --- | --- |
   | Claude | 1.00 | Visual reference; drawn to the full box. |
   | Codex | 0.90 | The dense, closed Blossom reads larger and heavier than the open Spark at the same trimmed size. |

   The scale belongs to the provider, not the file, so a user-provided Codex icon is also drawn at 0.90.
3. **Exact-size resampling.** The result is area-resampled once per device size (logical size × display scale) and cached. Boxes are 14 px in the flyout and 16 px in Settings, so icons stay sharp and centred at 100–200 % scaling in all themes.

## Licensing

Provider marks are third-party assets and trademarks. They are not covered by the repository's MIT license. See [`docs/design/brand-assets.md`](design/brand-assets.md) for provenance and separate trademark notices.

`ClaudeIcon-Rounded.svg` and `ClaudeIcon-Square.svg` (app tiles) are not used. The AUM application icon and dynamic tray indicator do not use provider marks.

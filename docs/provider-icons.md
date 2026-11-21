# Optional provider icons

The repository contains no provider logos. The logos remain trademarks of OpenAI and Anthropic, and the public brand notices reviewed do not establish permission to redistribute them in this MIT application.

There are two sources, checked per slot (provider × background):

1. **Embedded (local release builds).** `build.rs` embeds PNGs found in the git-ignored `rust/agent-monitor-ui/assets/provider-icons/` folder (or `AUM_PROVIDER_ICONS_DIR`) into the EXE. They are decoded through the same pipeline, so they look identical to the file path, and the app then does not need the folder below. A test checks this. Builds without those files (public CI) contain no logos. Do not distribute a build with embedded logos publicly without the owners' permission.
2. **User folder (fallback).** Only for slots without an embedded file, AUM reads the user's folder described below. The startup log reports `PROVIDER_ICONS: <slot> source=Bundled|LocalFolder`.

## Where to put the files

Place files in `%APPDATA%\Agent Usage Monitor\provider-icons\`. PNG and SVG are both supported. For each provider and background, AUM uses the first file in this list that is a readable regular file of at most 2 MiB (PNG at most 4096 px per edge) and that decodes:

| Provider | Dark themes (Graphite, Bluish Dark) | Light theme |
| --- | --- | --- |
| Codex | `codex-on-dark.png`, `OAI_OpenAI-Blossom_White.png`, `codex-on-dark.svg`, `codex.png`, `codex.svg` | `codex-on-light.png`, `OAI_OpenAI-Blossom_Black.png`, `codex-on-light.svg`, `codex.png`, `codex.svg` |
| Claude | `claude-on-dark.png`, `claude-on-dark.svg`, `claude.png`, `claude.svg`, `Claude Spark - Clay.png` | `claude-on-light.png`, `claude-on-light.svg`, `claude.png`, `claude.svg`, `Claude Spark - Clay.png` |

`ClaudeIcon-Rounded.png` and `ClaudeIcon-Square.png` (Claude app tiles) are never used. The flyout shows the Claude Spark symbol.

The Claude monochrome variants can be made from `Claude Spark - Clay.png`. Keep its alpha channel exactly and replace only RGB with `#FFFFFF` (`claude-on-dark.png`) or `#111111` (`claude-on-light.png`). Do not change the original file.

## How AUM displays them

- Files are read and decoded once per run, on the UI thread. Missing, corrupted, oversized or fully transparent files are skipped and the next candidate is tried. With no usable file, the provider name is shown alone. No substitute mark is drawn.
- PNG transparent margins are trimmed, and the content is centred in a square, so marks with different padding appear at the same size. The result is kept as a master of at most 256 px. It is then downscaled once per device size by area averaging with premultiplied alpha, and cached. The flyout mark is 14 logical px, which is 14, 18 (17.5 rounded), 21 and 28 device px at 100, 125, 150 and 200 % scaling; the Settings card uses 16 px. SVGs are rendered by Slint at display size.
- Changing the theme in Settings switches the variant immediately, without a restart. Adding new files requires restarting AUM.
- AUM never recolors or filters the files itself. The AUM application icon and the dynamic tray indicator never use provider marks.

See `docs/design/brand-assets.md` for the licensing review and official sources.

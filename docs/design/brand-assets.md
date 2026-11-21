# Third-party provider marks

## Separate from the MIT license

The project owner has approved distribution of the existing Codex and Claude provider assets in AUM. The files remain the property and trademarks of their respective owners. This project-side approval does not transfer ownership or change the owners' terms. The marks are **not** licensed under this repository's MIT license; MIT applies to AUM's own code and original project assets only.

The provider marks are used only inside their corresponding provider cards. They are not used in AUM's application icon, tray indicator, installer branding, or product name.

## Bundled source files and provenance

The three tracked SVG files in `rust/agent-monitor-ui/assets/providers/` are copied byte-for-byte from the owners' official asset packages:

| Tracked file | Official source and package path | Use |
| --- | --- | --- |
| `OAI_OpenAI-Blossom_White.svg` | [OpenAI Brand](https://openai.com/brand/) logo package, `OpenAI-logos/SVGs/OAI_OpenAI-Blossom_White.svg` | Codex card on dark themes |
| `OAI_OpenAI-Blossom_Black.svg` | [OpenAI Brand](https://openai.com/brand/) logo package, `OpenAI-logos/SVGs/OAI_OpenAI-Blossom_Black.svg` | Codex card on Light theme |
| `Claude Spark - Clay.svg` | [Anthropic Press Kit](https://www.anthropic.com/press-kit) package, `Anthropic media resources/Anthropic logos/Claude logos/3 Claude Spark/SVG/Claude Spark - Clay.svg` | Claude card on all themes |

OpenAI does not publish a separate Codex mark in that package; AUM uses the OpenAI Blossom to identify the Codex provider card. The Anthropic Spark source is kept unmodified and in its supplied color. The prior local white and dark raster derivatives are not used by clean-checkout builds.

`build.rs` embeds the tracked SVG bytes into the executable. The build does not download assets. User-provided build and runtime icon fallback paths remain available as documented in `docs/provider-icons.md`.

## Owner guidance

- [OpenAI brand guidelines](https://openai.com/brand/) identify OpenAI names and marks as OpenAI property, require use to follow its terms, and say to use marks as provided. The downloaded logo package is supplied through that page.
- [Anthropic trademark guidelines](https://www.anthropic.com/legal/trademark-guidelines) govern use of Anthropic and Claude marks. The official press kit supplies the SVG source referenced above.

These notes identify asset provenance and ownership; they do not grant permissions beyond the distribution approval recorded above. Do not describe the marks as AUM or MIT-licensed assets.

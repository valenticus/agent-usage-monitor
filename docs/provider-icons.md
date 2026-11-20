# Optional provider icons

AUM does not bundle provider logos. The logos remain trademarks of OpenAI and Anthropic, and the public asset and brand notices reviewed for this change do not establish permission to redistribute them in this MIT application.

Users who have permission to use the authentic, unmodified provider artwork may place SVG files at:

- `%APPDATA%\Agent Usage Monitor\provider-icons\codex.svg`
- `%APPDATA%\Agent Usage Monitor\provider-icons\claude.svg`

AUM loads each file once at startup, only if it is a regular file no larger than 256 KiB and Slint can decode it. Missing or invalid files are ignored; provider names and quota data remain visible without a substitute mark. The same optional image is shown in the flyout and provider settings card. These local files are not part of the repository or Windows CI artifacts.

The referenced [Nook repository](https://github.com/valenticus/nook) identifies its interface symbols as Lucide and Simple Icons assets in [`LICENSE-ASSETS.md`](https://github.com/valenticus/nook/blob/main/LICENSE-ASSETS.md). Those notices cover the named icon projects and Nook's own assets; they do not grant rights to third-party provider trademarks. AUM therefore does not copy or redistribute those provider marks.

# Provider brand assets

Update, 21 November 2026 (v0.1.0 pre-release): the approved local marks (OpenAI Blossom white/black, unmodified; Claude Spark recoloured to #FFFFFF and #111111) can be embedded into a local build from the git-ignored `rust/agent-monitor-ui/assets/provider-icons/` folder. Permission is **still unresolved**. OpenAI's marks terms require use "exactly as provided" and per the brand guidelines. Anthropic requires prior written approval and forbids colour alterations, so the recoloured Spark variants are not compliant as they stand. Builds containing these marks are for private review only until permission is granted.

Earlier status (superseded by the update above), 21 November 2026: **unresolved.** No official provider logos are bundled, because neither owner's published terms permit a third-party app to embed them without prior permission. Until official artwork is permitted, the Codex and Claude cards show the provider name only. No substitute mark is drawn, so nothing invented stands in for the real logos. Earlier placeholders (`>_`, a sparkle, and the "Cx"/"Cl" tiles) were removed.

Provider logos are never used in the AUM application icon, the tray icon, installer branding or the window title. AUM's own identity is the two-bar mark.

## Official sources reviewed

| Owner | Official source | What is offered | Terms for third parties |
| --- | --- | --- | --- |
| OpenAI | https://openai.com/brand/ | A "Download logos" button in the Blossom section. The page shows images such as `Blossom_Light.svg`. The full guide at brand.openai.com did not render for automated review. | Under "How to use our Logos", allowed use includes "Use the logo only when it directly relates to OpenAI services" and "Use the logo exactly as provided". Not allowed: "Use the logo without permission or outside OpenAI's terms". Permission is non-exclusive and non-transferable, and OpenAI "may terminate permission … at any time". |
| OpenAI (Codex) | https://chatgpt.com/codex/ | Marketing copy only. No Codex-specific mark or brand kit was found. | Not applicable. |
| Anthropic | https://www.anthropic.com/legal/trademark-guidelines (effective 1 Aug 2024; PDF linked from that page) | No public logo files. "We will supply an image (or images) of the trademark(s) for your use". | "You may only use our trademarks as specifically permitted by us and only in materials we approve beforehand." No alterations of color, font or proportion are allowed. |
| Anthropic | https://www.anthropic.com/news → "Download press kit" (`/press-kit`, redirects to a ZIP on assets.claude.com) | An official media-asset ZIP. Its contents were not inspected. | The trademark guidelines above still apply, so approval is required. |

No page documents an official pair of dark-background and light-background variants for either provider.

## How to obtain permission

| Owner | Contact | What to send |
| --- | --- | --- |
| OpenAI | partnercomms@openai.com (logo permission requests). Legal questions: legal@openai.com. | The project name and repository URL, and that it is an independent, open-source tray utility showing the user's own Codex quota. Explain that a 16 px unmodified mark would appear only inside the Codex card, never in AUM's name, icon or branding. Ask which official files to use on dark and light backgrounds, and whether redistribution inside the MIT repository is allowed or the files must stay user-supplied. |
| Anthropic | marketing@anthropic.com. The guidelines name this address for parties with an existing business relationship; press@anthropic.com is listed for media. | The same details for the Claude card. Anthropic says it supplies the approved images itself. |

Record any written permission next to the asset files, in a separate `third_party/` folder with its own notice, outside the MIT-licensed code. Do not treat the permission as covering anything beyond what it states.

## Local official artwork supplied by the user (supported now)

A user who is permitted to use the marks can place unmodified official SVG files in `%APPDATA%\Agent Usage Monitor\provider-icons\`:

| File | Used for |
| --- | --- |
| `codex-on-dark.svg`, `claude-on-dark.svg` | Graphite and Bluish Dark themes |
| `codex-on-light.svg`, `claude-on-light.svg` | Light theme |
| `codex.svg`, `claude.svg` | Fallback for a variant that is missing |

Recheck, 21 November 2026 — official sources for these files:

| Provider | Official download | Gate | Candidate file (unmodified) |
| --- | --- | --- | --- |
| Claude | https://www.anthropic.com/press-kit → ZIP on assets.claude.com | No click-through, but the trademark guidelines require Anthropic's prior approval of the material | `Anthropic media resources/Anthropic logos/Claude logos/3 Claude Spark/SVG/Claude Spark - Clay.svg` (one colour that reads on dark and light). Save it as `claude.svg`. |
| Codex (OpenAI) | https://openai.com/brand/ → "Download logos" (`https://cdn.openai.com/brand/openai-logos.zip`) | The page says "By using our logos, you agree to our Marks usage terms" | The Blossom mark: the white/light variant as `codex-on-dark.svg`, the black/dark variant as `codex-on-light.svg`. OpenAI publishes no separate Codex mark. |

AUM did not download the OpenAI kit or install either mark, because doing so means accepting the owners' terms on the user's behalf. Placing the files is the user's decision.

The variant switches automatically with the theme. It is shown at 16 px with its original proportions (contain fit), in the flyout, the compact preview and the Settings provider card. AUM never recolors, inverts or filters these files.

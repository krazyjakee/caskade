# Themes in the native app

The native app (Blitz, #87 onwards) applies themes exactly as the webview
does: the same compiled stylesheet from `ThemeService` in `cs.theme`, the
same tokens, layers, anatomy selectors and Appearance attributes, and the
user's CSS in `cs.user` (see [README](README.md)). A theme needs no changes.
What Blitz still renders differently from the webview is below; the BLITZ-6
issue (#85) decides each one, upstream or by design.

## How it's checked

- `cargo test -p caskade-native --features shots --test shots` renders every
  theme (Nexus, Paper, and the catalogue's Ember) on the reference screens
  with vello_cpu, through the app's own theme code, and compares them with
  the committed images in `apps/native/tests/shots/`.
- `caskade-native --shots <folder>` also writes each screen as an HTML page
  with the same markup, attributes, stylesheets, fonts and theme assets.
  `apps/native/scripts/webkit-shots.py` renders those pages in WebKitGTK on
  a private desktop, and `compare-shots.py` compares the two engines pixel by
  pixel, saving the differing pixels in red beside both renders.

The reference screen is the BLITZ-0 prototype's library: the shell's
sidebar, top bar with search, status bar and backdrop, and a grid of game
cards with placeholder art. The views' own screens join as they're ported
(#96 onwards).

## Blitz against WebKitGTK (10 Nov 2026)

Pixels differing by more than 24 in a channel, at 1280×800:

| Theme | Differing | Mean difference |
| --- | --- | --- |
| Nexus (`caskade.default`) | 3.6% | 4.6 |
| Ember (`dev.caskade.ember`) | 3.1% | 4.6 |
| Paper (`caskade.paper`) | 14.3% | 16.4 |

Layout, colours, fonts (Inter, Oxanium, JetBrains Mono and Paper's Source
Serif 4) and every token-driven style match. What differs:

| What | Where | In Blitz | Issue |
| --- | --- | --- | --- |
| `backdrop-filter` (the glass blur) | Nexus, Ember: cards, top bar, sidebar | Not drawn: the backdrop's grid lines and glows show through translucent surfaces | #85 (Skia only) |
| `text-overflow: ellipsis` | Every theme: long card titles | The text is clipped with no "…" | #80 |
| SVG filters in a background image | Paper's `grain.svg` (`feTurbulence`) | The filtered rectangle draws as solid colour, so the grain is a grey square | #85 |
| A background image repeated at `background-size` | Paper's grain (240 px tiles) | Only the first tile is drawn | #85 |
| Hairline `repeating-linear-gradient` stripes | Paper's placeholder art | The 1 px stripes draw heavier and darker | #85 |
| Scrollbars | Every theme: the library view | WebKit shows a scrollbar; Blitz shows none | #93 (the components') |
| Glyph positions | Every theme | Sub-pixel differences in spacing and anti-aliasing | none (expected) |

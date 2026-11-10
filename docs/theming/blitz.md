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

Pixels differing by more than 24 in a channel on the library screen, at
1280×800:

| Theme | Differing | Mean difference |
| --- | --- | --- |
| Nexus (`caskade.default`) | 1.6% | 3.1 |
| Ember (`dev.caskade.ember`) | 1.7% | 3.1 |
| Paper (`caskade.paper`) | 9.4% | 12.0 |

The gallery's pages differ by 0.6–3.5% in every theme, but for Paper's
home page (6.9%: the hero's halftone, below). Paper's 9.4% is the 1 px
line-height shift: with the view's heading at a whole-pixel height it's
1.2%.

Layout, colours, fonts (Inter, Oxanium, JetBrains Mono and Paper's Source
Serif 4) and every token-driven style match. SVG background images tile at
`background-size`, and SVGs that use filters (Paper's `feTurbulence` grain)
are drawn: Blitz rasterises them with resvg, once per size (series patch
`blitz/0025`, #124). What differs:

| What | Where | In Blitz | Issue |
| --- | --- | --- | --- |
| `backdrop-filter` (the glass blur) | Glass surfaces | Not drawn, by design (#85): glass over imagery is at least 80% opaque instead. WebKitGTK under Xvfb doesn't draw it either, so it adds nothing to the numbers above | #85 |
| A `position: fixed` box with `z-index: auto` inside a positioned box | A theme's backdrop (`[data-cs="backdrop"]`) at `position: fixed`, inside the positioned shell | Painted above the shell's positioned content, not under it (CSS 2.1 Appendix E puts it in tree order), so a fixed backdrop's grid, glows or grain cover the content. The built-in themes and the theme guide put the backdrop at `position: absolute; inset: 0`, which fills the same box in the shell and looks the same in WebKit (#124) | none (upstream) |
| Small gradient tiles | Paper's hero halftone (4 px radial-gradient dots) | A gradient layer of more than 500 tiles isn't drawn (a Blitz FIXME) | none |
| Fractional line heights | Text whose line height isn't a whole pixel (`text-2xl` is 31.999 px) | Rounded to the nearest pixel where WebKit truncates, so what follows sits 1 px lower; fine stripes below it (Paper's placeholder art) go out of phase | none (expected) |
| Scrollbars | Every theme: the library view | WebKit shows a scrollbar; Blitz shows none | #93 (the components') |
| Glyph positions | Every theme | Sub-pixel differences in spacing and anti-aliasing | none (expected) |

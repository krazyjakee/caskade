# Writing a theme

This guide builds **Paper**, a light theme, from an empty folder. Paper looks
nothing like the default Nexus theme: it has warm paper surfaces, ink-red
accents and square corners. It uses a serif display face and has no glow,
blur, grid or scanlines. It uses only the public contract:

- the declared `--cs-*` tokens ([tokens.md](tokens.md));
- the anatomy selectors ([anatomy.md](anatomy.md));
- fonts and images in its own folder.

If you need something the contract doesn't have, ask for it to be added to
the contract. Don't work around it with class names. The rules are in the
[README](README.md#theme-packages-139).

The finished theme ships as [`themes/paper`](../../themes/paper) with the id
`caskade.paper`. Give your copy your own reverse-DNS id. A built-in theme
wins over an installed folder with the same id. This guide uses
`dev.example.paper`. Apart from the id and the author, your folder ends up
identical to `themes/paper`.

## 1. The folder

```sh
mkdir -p paper/assets/fonts paper/previews
cd paper
```

```
paper/
├── theme.json              # step 2
├── tokens.json             # step 3
├── theme.css               # steps 4 and 5
├── assets/
│   ├── fonts/              # step 5: the woff2 files and their licence
│   └── grain.svg           # step 4
└── previews/home.svg       # step 6
```

A theme holds only tokens, CSS, fonts and images. The lint refuses scripts,
HTML and binaries anywhere in the folder.

## 2. `theme.json`

```json
{
  "id": "dev.example.paper",
  "name": "Paper",
  "version": "1.0.0",
  "description": "A light, printed look: warm paper, ink-red accents, square corners, ruled lines and a serif display face. No glows, blur or grids.",
  "author": "You",
  "modes": ["light"]
}
```

- **`modes: ["light"]`** marks Paper as a light theme. Settings use it too:
  a custom accent colour is darkened, not lightened, until it contrasts with
  the theme's surfaces.
- **No `extends`.** A theme that extends `caskade.default` inherits Nexus's
  `theme.css`: the backdrop grid, the HUD corner brackets, the hero
  scanlines and the card glow. Paper doesn't want any of them, so it extends
  nothing. Underneath it are only the contract's default token values and
  the plain components. Extend `caskade.default` when you only want to
  recolour Nexus.
- `previews` comes in step 6. The lint fails on a preview that doesn't
  exist yet.

## 3. `tokens.json`

Tokens do most of the work. Use the DTCG format, the same as
`packages/tokens`. A group's `$type` applies to the tokens inside it. Every
path must be a token listed in [tokens.md](tokens.md), and every value must
match that token's type. `"{color.accent.alt}"` is an alias, and it compiles
to `var(--cs-color-accent-alt)`.

```json
{
  "$description": "Paper overrides the contract's semantic and component tokens (docs/theming/tokens.md). It leaves alone the tokens that Appearance and console mode change through override sets (density.scale, type.scale, motion durations and stagger, focus.ring.width, card.width, hero.height): a theme's tokens sit in a later layer and would win over the user's settings.",
  "font": {
    "$type": "fontFamily",
    "display": {
      "$value": ["Source Serif 4 Variable", "Georgia", "Times New Roman", "serif"]
    }
  },
  "color": {
    "$type": "color",
    "surface": {
      "base": { "$value": "#f3eee3" },
      "raised": { "$value": "#fbf8f1" },
      "overlay": { "$value": "#fffdf8" },
      "hover": { "$value": "#e9e1cf" }
    },
    "border": {
      "$root": { "$value": "#2b25182b" },
      "strong": { "$value": "#2b251857" }
    },
    "fg": {
      "$root": { "$value": "#1f1b14" },
      "muted": { "$value": "#4d4537" },
      "subtle": { "$value": "#756b59" }
    },
    "accent": {
      "$root": { "$value": "#a33224" },
      "strong": { "$value": "#842518" },
      "alt": { "$value": "#1f4a73" },
      "fg": { "$value": "#fffdf8" }
    },
    "glow": { "$value": "#00000000" },
    "status": {
      "installed": { "$value": "#2e6f45" },
      "available": { "$value": "#a65a17" },
      "emulated": { "$value": "#55458f" },
      "missing": { "$value": "#6f6857" }
    },
    "warning": { "$value": "#94660c" },
    "danger": { "$value": "#a8241c" }
  },
  "radius": {
    "$type": "dimension",
    "card": { "$value": "0" },
    "control": { "$value": "0" },
    "panel": { "$value": "0" }
  },
  "effect": {
    "glass": { "blur": { "$value": "0px" } },
    "grid": { "opacity": { "$value": 0 } },
    "scanline": { "opacity": { "$value": 0 } },
    "glow": { "strength": { "$value": 0 } }
  },
  "motion": {
    "ease": { "out": { "$value": [0.25, 0.1, 0.25, 1] } }
  },
  "focus": {
    "ring": { "glow": { "$value": "{color.accent.alt}" } }
  },
  "sidebar": { "width": { "$value": "14rem" } },
  "topbar": { "height": { "$value": "4rem" } },
  "card": { "aspect": { "$value": "3 / 2" } },
  "chamfer": { "$value": "0px" }
}
```

What each group does:

- **Colours.** Paper and ink replace Nexus's navy. The surfaces are opaque:
  nothing is glass. Borders are ink at low alpha (8-digit hex), so they work
  on every surface. `glow` is fully transparent, so every glow the components
  draw from it disappears.
- **Radii.** `0` squares every card, control and panel. `chamfer: 0px` turns
  the cut-corner primary actions into rectangles.
- **Effects.** No blur, grid, scanlines or glow. The components already take
  these from the effect tokens, so these four values remove them everywhere.
- **Type.** `font.display` is a serif. Headings, numbers and labels follow it.
  Body text keeps the contract's sans and mono stacks. Step 5 bundles the
  font.
- **Density.** A narrower sidebar, a lower top bar and squarer card art.

Leave out the tokens that the user's Appearance settings and console mode
change through override sets. Those are `density.scale`, `type.scale`, the
motion durations and `stagger`, `focus.ring.width`, `card.width` and
`hero.height`. A theme's tokens compile into the `cs.theme` layer, which wins
over those sets. Setting one of them would turn off the user's setting. For
the same reason, Paper keeps the components' motion tokens and only changes
the easing curve.

## 4. `theme.css`

The tokens already restyle every component. `theme.css` adds what tokens
can't express. Its rules can only target:

- `[data-cs="…"]` for a component's root;
- `[data-part="…"]` for one of its parts;
- the other contract attributes, such as `data-status`, `data-variant`,
  `data-hovered` and `data-mode`;
- pseudo-classes, pseudo-elements, `*` and `:root`.

The lint rejects:

- class, id and element selectors;
- unknown roots and parts;
- undeclared `--cs-*` names;
- `!important`, `@import` and nested rules.

A part must belong to the root before it. For example,
`[data-cs="hero"] [data-part="fx"]` is fine, but
`[data-cs="hero"] [data-part="stores"]` is an error.

Write the file one area at a time. The order below matches
`themes/paper/theme.css`. Begin with a comment saying what the theme is:

```css
/*
 * Paper: a light, printed look written only against the public contract:
 * declared --cs-* tokens (docs/theming/tokens.md) and anatomy selectors
 * (docs/theming/anatomy.md). It doesn't extend caskade.default, so none of
 * Nexus's grid, HUD corners, scanlines or glows are underneath; every rule
 * here starts from the plain components.
 *
 * Square corners come from the radius tokens, and the hard offset shadows
 * stand in for glows: a sheet of paper lying on another.
 */
```

### Fonts

Step 5 adds the `@font-face` rules here, at the top of the file.

### The document

The app's base stylesheet is written for a dark scheme. A light theme sets
`color-scheme` itself, so scrollbars and native controls turn light too:

```css
/* ── Document: a light colour scheme for native controls and scrollbars ── */

* {
  color-scheme: light;
  scrollbar-color: var(--cs-color-border-strong) transparent;
}

::selection {
  background: color-mix(in oklab, var(--cs-color-accent) 22%, transparent);
  color: var(--cs-color-fg);
}
```

### The backdrop

`[data-cs="backdrop"]` is an empty layer behind the shell. Nexus draws its
grid there. Without Nexus, it has no size, so the theme positions it:
`position: absolute; inset: 0` fills the shell, which fills the window. Use
`absolute`, not `fixed`: the native app paints a fixed box above the shell's
content (a Blitz bug). Paper
fills it with paper grain, a small tiling SVG noise. Save this as
`assets/grain.svg`:

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240">
  <filter id="grain" x="0" y="0" width="100%" height="100%">
    <feTurbulence type="fractalNoise" baseFrequency="0.85" numOctaves="3" stitchTiles="stitch" />
    <feColorMatrix values="0 0 0 0 0.17  0 0 0 0 0.14  0 0 0 0 0.09  0 0 0 0.55 0" />
  </filter>
  <rect width="240" height="240" filter="url(#grain)" />
</svg>
```

```css
/* ── Backdrop: paper grain instead of a grid ── */

[data-cs="backdrop"] {
  position: absolute;
  inset: 0;
  pointer-events: none;
  background-image: url("assets/grain.svg");
  background-size: 240px 240px;
  opacity: 0.35;
  mix-blend-mode: multiply;
}
```

A `url()` must name an image or a font under `assets/` that exists in the
folder. The compiler rewrites it to the theme's own URL. Remote, absolute
and `../` URLs are refused.

### The shell

Paper replaces the glass with ruled edges. The sidebar is a slightly darker
tint, so the grain shows through it. The active page gets a raised slip with
an ink-red margin rule, in place of Nexus's glowing rail. The
`[data-part="nav-item"]` rules have no root in front of them, so they also
cover console mode's sidebar.

```css
/* ── Shell: ruled edges instead of glass ── */

:is([data-cs="sidebar"], [data-cs="console-sidebar"]) {
  background: color-mix(in oklab, var(--cs-color-surface-hover) 45%, transparent);
  border-right: 1px solid var(--cs-color-border-strong);
}

[data-cs="sidebar"] [data-part="brand"] {
  border-bottom: 1px solid var(--cs-color-border-strong);
}

[data-cs="sidebar"] [data-part="divider"] {
  border-top: 3px double var(--cs-color-border-strong);
}

:is([data-cs="topbar"], [data-cs="console-topbar"]) {
  border-bottom: 1px solid var(--cs-color-border-strong);
}

:is([data-cs="status-bar"], [data-cs="hint-bar"]) {
  background: var(--cs-color-surface-base);
  border-top: 1px solid var(--cs-color-border-strong);
}

/* The active page: a raised slip with an ink-red margin rule. */
[data-part="nav-item"][data-status="active"] {
  background: var(--cs-color-surface-raised);
  box-shadow:
    inset 3px 0 0 var(--cs-color-accent),
    0 0 0 1px var(--cs-color-border);
  font-weight: 600;
}

[data-part="nav-item"][data-status="active"] [data-part="icon"] {
  color: var(--cs-color-accent);
}

[data-cs="console-sidebar"] [data-part="count"] {
  border-radius: 0;
}
```

The brand's height is `--cs-topbar-height`, so its bottom rule lines up with
the top bar's.

### The hero

The hero paints its generated art as an inline background. A theme can't
change that, but it can style the layers above it:

- `glow`: hidden.
- `shade`: a paper fade, so the text sits on paper and the art shows faded
  on the right.
- `fx`: Nexus's scanlines go. In their place, a halftone screen over the art.

```css
/* ── Hero: a faded print with a halftone screen, text set on the paper ── */

[data-cs="hero"] {
  border-color: var(--cs-color-border-strong);
  box-shadow: 6px 6px 0 var(--cs-color-border);
}

[data-cs="hero"] [data-part="glow"] {
  display: none;
}

[data-cs="hero"] [data-part="shade"] {
  background: linear-gradient(
    90deg,
    var(--cs-color-surface-raised) 35%,
    color-mix(in oklab, var(--cs-color-surface-raised) 65%, transparent) 70%,
    color-mix(in oklab, var(--cs-color-surface-raised) 40%, transparent)
  );
}

[data-cs="hero"] [data-part="fx"] {
  position: absolute;
  inset: 0;
  pointer-events: none;
  background-image: radial-gradient(
    circle,
    color-mix(in oklab, var(--cs-color-fg) 22%, transparent) 0.7px,
    transparent 1.3px
  );
  background-size: 4px 4px;
  mask-image: linear-gradient(90deg, transparent 35%, #000 85%);
}

[data-cs="hero"] [data-part="eyebrow"] {
  color: var(--cs-color-accent);
}

[data-cs="hero"] [data-part="live-dot"] {
  border-radius: 0;
  box-shadow: none;
  background: var(--cs-color-accent);
}

[data-cs="hero"] [data-part="title"] {
  font-weight: 650;
  letter-spacing: -0.01em;
  line-height: 1.05;
  text-transform: none;
}

[data-cs="hero"] [data-part="description"] {
  font-family: var(--cs-font-display);
  font-size: 1rem;
}

[data-cs="hero"] [data-part="primary-action"] {
  box-shadow: 3px 3px 0 var(--cs-color-fg);
  transition-property: translate, box-shadow;
  transition-duration: var(--cs-motion-duration-fast);
  transition-timing-function: var(--cs-motion-ease-out);
}

[data-cs="hero"] [data-part="primary-action"]:hover {
  translate: -1px -1px;
  box-shadow: 4px 4px 0 var(--cs-color-fg);
}
```

The transitions take their timing from the motion tokens. When the user
asks for reduced motion, the durations drop to zero and the hover just snaps.

### Cards

A hard offset shadow takes the place of the glow, so a hovered card lifts
off the page. Placeholder art is dark, and on paper its title would be ink
on a dark background. Paper washes the art toward the paper colour with a
`::before` layer, then pastes the title on a label. The label is positioned,
so it paints above the `::before`.

```css
/* ── Cards: printed tiles that lift off the page ── */

:is([data-cs="game-card"], [data-cs="collection-card"]) {
  border-color: var(--cs-color-border-strong);
  transition-property: translate, box-shadow, background-color;
  transition-duration: var(--cs-motion-duration-base);
  transition-timing-function: var(--cs-motion-ease-out);
}

[data-cs="game-card"]:is(:hover, :focus-visible),
[data-cs="collection-card"]:is(:hover, :focus-visible) {
  translate: -2px -2px;
  box-shadow: 4px 4px 0 var(--cs-color-fg);
}

[data-cs="game-card"] [data-part="art"],
[data-cs="collection-card"] [data-part="art"] {
  border-bottom: 1px solid var(--cs-color-border-strong);
}

[data-cs="game-card"] [data-part="title"],
[data-cs="collection-card"] [data-part="title"] {
  font-family: var(--cs-font-display);
  font-size: 0.95rem;
  font-weight: 600;
}

[data-cs="collection-card"] [data-part="count"] {
  color: var(--cs-color-fg);
  filter: none;
}

/* Generated art is dark; wash it towards the paper. */
[data-cs="game-art"][data-source="placeholder"]::before {
  content: "";
  position: absolute;
  inset: 0;
  pointer-events: none;
  background: var(--cs-color-surface-base);
  mix-blend-mode: screen;
  opacity: 0.55;
}

[data-cs="status-badge"] {
  border-radius: 0;
  font-size: 0.625rem;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
```

`[data-source="placeholder"]` limits the wash to generated art. Real cover
images keep their colours.

### Shelves, panels and settings

Shelf headers become newspaper section heads. Panels and settings sections
become sheets on a desk, and setting rows become ledger lines.

```css
/* ── Shelves: newspaper section heads ── */

:is([data-cs="shelf"], [data-cs="shelf-row"]) [data-part="header"] {
  padding-bottom: 0.4rem;
  border-bottom: 2px solid var(--cs-color-fg);
}

:is([data-cs="shelf"], [data-cs="shelf-row"]) [data-part="title"] {
  font-size: 1.35rem;
  letter-spacing: 0;
}

:is([data-cs="shelf"], [data-cs="shelf-row"]) [data-part="view-all"] {
  color: var(--cs-color-accent);
}

/* ── Panels and settings: sheets on the desk, ledger rows ── */

:is([data-cs="panel"], [data-cs="settings-section"]) {
  border-color: var(--cs-color-border-strong);
  box-shadow: 4px 4px 0 var(--cs-color-border);
}

[data-cs="settings-section"] [data-part="header"] {
  padding-bottom: 0.75rem;
  border-bottom: 1px solid var(--cs-color-border-strong);
}

[data-cs="setting-row"] {
  border-top-style: dashed;
}
```

### Controls

The radius tokens square most controls. A few parts are always round, such
as switch thumbs and slider thumbs, and some glow, such as status dots. This
section squares those parts and turns off their glows:

```css
/* ── Controls: square, flat, inked ── */

[data-cs="button"][data-variant="primary"] {
  background: var(--cs-color-accent);
}

[data-cs="button"][data-variant="primary"][data-hovered] {
  background: var(--cs-color-accent-strong);
}

[data-cs="switch"] :is([data-part="track"], [data-part="thumb"]) {
  border-radius: 0;
  box-shadow: none;
}

[data-cs="checkbox"] [data-part="box"],
[data-cs="segmented"] [data-part="segment"],
[data-cs="select-popover"] [data-part="option"],
[data-cs="search-popover"] [data-part="suggestion"] {
  border-radius: 0;
}

[data-cs="slider"] [data-part="fill"] {
  border-radius: 0;
  background: var(--cs-color-accent);
}

[data-cs="slider"] [data-part="thumb"] {
  border-radius: 0;
  border-color: var(--cs-color-fg);
  background: var(--cs-color-surface-overlay);
  box-shadow: none;
}

[data-cs="status-dot"] [data-part="dot"] {
  border-radius: 0;
  box-shadow: none;
}

:is([data-cs="select-popover"], [data-cs="search-popover"]) {
  box-shadow: 4px 4px 0 var(--cs-color-border-strong);
}
```

Style interaction state through React Aria's attributes, such as
`[data-hovered]` above. A component's own `:hover` styles follow the same
attributes.

## 5. Fonts

A theme ships its fonts. It can't load them from the network. Paper's
display face is [Source Serif 4](https://github.com/adobe-fonts/source-serif),
licensed under the SIL Open Font License 1.1. Use the variable weight-axis
woff2 files that Fontsource publishes, the Latin and Latin Extended subsets:

```sh
base=https://cdn.jsdelivr.net/npm/@fontsource-variable/source-serif-4@5.3.1/files
curl -fLo assets/fonts/source-serif-4-latin-wght-normal.woff2 "$base/source-serif-4-latin-wght-normal.woff2"
curl -fLo assets/fonts/source-serif-4-latin-ext-wght-normal.woff2 "$base/source-serif-4-latin-ext-wght-normal.woff2"
curl -fLo assets/fonts/OFL.txt https://raw.githubusercontent.com/adobe-fonts/source-serif/release/LICENSE.md
```

The OFL requires the licence to travel with the font, so keep `OFL.txt` next
to the font files. The lint accepts it: only code is refused. The theme
scheme serves only fonts and images, so the licence is not served.

Declare the font at the top of `theme.css`, under the opening comment. The
`font-family` name must match the first name in the `font.display` token:

```css
/* ── Type: Source Serif 4 for --cs-font-display (OFL, assets/fonts/OFL.txt) ── */

@font-face {
  font-family: "Source Serif 4 Variable";
  font-style: normal;
  font-display: swap;
  font-weight: 200 900;
  src: url("assets/fonts/source-serif-4-latin-wght-normal.woff2") format("woff2");
  unicode-range:
    U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308,
    U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD;
}

@font-face {
  font-family: "Source Serif 4 Variable";
  font-style: normal;
  font-display: swap;
  font-weight: 200 900;
  src: url("assets/fonts/source-serif-4-latin-ext-wght-normal.woff2") format("woff2");
  unicode-range:
    U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, U+0329,
    U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C4, U+2113,
    U+2C60-2C7F, U+A720-A7FF;
}
```

The fallbacks in the token (`Georgia`, `Times New Roman`, `serif`) keep the
theme a serif if the font can't load.

## 6. Previews

The theme picker shows the images listed in `previews`. Any image type
works: PNG, JPEG, WebP, AVIF, GIF or SVG. A screenshot of the Home view is
the obvious choice. `themes/paper` ships a hand-drawn SVG of the Home view,
`previews/home.svg`; copy it or take your own screenshot. Then list it in
`theme.json`:

```json
  "modes": ["light"],
  "previews": ["previews/home.svg"]
}
```

## 7. Lint and build

From a Caskade checkout:

```sh
cargo run -p caskade-theme --features cli -- lint path/to/paper
```

`1 theme(s) ok` means the theme passes. Otherwise each problem names the
file and, where it has one, the line and column:

```
paper/theme.css:3:3: element selector h1: target data-cs and data-part (use :root for the document)
paper/tokens.json: radius.card: expected a dimension in px, rem, em, %, vh, vw, got "4pt"
```

The shipped copy is checked the same way, with
`cargo run -p caskade-theme --features cli -- lint themes/paper`. A test in
`crates/caskade-theme/tests/paper.rs` runs it with `cargo test`.

To see exactly what the app will load, print the compiled stylesheet. It
holds the tokens on `:root`, then the CSS with its URLs rewritten, all in
`@layer cs.theme`:

```sh
cargo run -p caskade-theme --features cli -- build path/to/paper
```

Paper extends nothing, so `build` needs no other folders. A theme that
extends `caskade.default` also passes `themes/default`.

## 8. Install

Copy the folder into the `themes` folder of Caskade's data directory:

| OS | Folder |
| --- | --- |
| Linux | `~/.local/share/caskade/themes/` (`$XDG_DATA_HOME/caskade/themes/`) |
| Windows | `%APPDATA%\Caskade\Caskade\data\themes\` |
| macOS | `~/Library/Application Support/dev.Caskade.Caskade/themes/` |

```sh
cp -r paper ~/.local/share/caskade/themes/
```

Caskade reads this folder at startup and whenever the themes are reloaded.
Choose Paper under Settings → Appearance. A theme with problems still shows
in the list, with each problem, and it never hides the other themes.

## 9. Check it in the app

Open every view in both shells: Home, Library (grid and list), a game's
page, Collections, Store, Emulation and each Settings tab. Then do the same
in console mode. Then try the user's settings against the theme:

- **Reduce effects** and **reduced motion** must still work. They do when
  blur, glow and motion come from the effect and motion tokens, as they do
  here.
- **Compact density** and **UI scale** must still change spacing and size.
  They do when the theme leaves `density.scale` and `type.scale` alone.
- **A custom accent** replaces `--cs-color-accent`. It must stay readable on
  `surface-base` and `surface-raised`.

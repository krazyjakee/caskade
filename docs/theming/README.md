# Theming contract

What themes (and plugins' styles) may rely on. How it should look is in the
[design guidelines](../DESIGN-GUIDELINES.md). Rule: **themes restyle, plugins restructure.** A theme is
tokens, CSS, fonts and images, never JavaScript.

## Design tokens (#137)

The tokens are the main way to restyle Caskade. They live as
[W3C Design Tokens (DTCG)](https://www.designtokens.org/) JSON in
[`packages/tokens/tokens/`](../../packages/tokens/tokens) and compile to CSS
custom properties named after their path: `color.accent.strong` becomes
`--cs-color-accent-strong`. The full list, with types and defaults, is
[tokens.md](tokens.md) (generated).

- **Three tiers.**
  - `primitive.json`: raw palette and font stacks.
  - `semantic.json`: what things mean, like `color.surface.*`, `color.accent`,
    `color.status.*`, `radius.*`, `effect.*`, `motion.*`, `density.scale`,
    `type.scale` and `focus.ring.*`. Most themes only change these.
  - `component.json`: sizes of particular components, like `sidebar.width`,
    `card.aspect` and `hero.height`.
- **Aliases** (`"$value": "{color.palette.indigo}"`) compile to `var()`, so a
  theme that changes a primitive restyles every token built on it. An alias
  must point at a declared token of the same type; cycles are errors.
- **`$root`** names a group's own value: `color.accent.$root` is
  `--cs-color-accent`, next to `color.accent.strong`.
- **Types:** `color` (hex), `dimension` (`px`, `rem`, `em`, `%`, `vh`, `vw`),
  `duration` (`ms`, `s`), `number`, `cubicBezier`, `fontFamily`, and
  Caskade's `keyword` for CSS keywords and ratios (`running`, `16 / 10`).
  Values are validated. Errors name the file and token path.
- **Override sets** (`tokens/sets/*.json`) restate declared tokens under a
  condition, given in `$extensions["dev.caskade.condition"]` as a `selector`
  (`[data-mode="console"]`, `[data-color-scheme="light"]`) or a `media`
  query, or both. They exist for reduced motion and reduced transparency
  and for the user's Appearance settings (below).

Only declared tokens are public. A test fails if the app or a package reads
a `--cs-*` the contract doesn't declare, and themes are checked against the
same list ([`manifest.json`](../../packages/tokens/manifest.json)).

After changing the JSON, run `pnpm --filter @caskade/tokens build`. It
rewrites `packages/tokens/tokens.css` (imported by the app into the
`cs.tokens` layer), `manifest.json` and `tokens.md`.
`pnpm --filter @caskade/tokens test` fails while they are stale.

## Anatomy selectors (#138)

CSS reaches components through attributes, never class names. Every
catalogued component renders:

- `data-cs="<name>"` on its root, such as `[data-cs="game-card"]`.
- `data-part="<part>"` on its parts, such as
  `[data-cs="game-card"] [data-part="title"]`. A part belongs to the nearest
  enclosing root.
- Domain state on the root, such as `data-status="installed"`,
  `data-variant` or `data-tone`.
- Interaction state, such as `data-hovered`, `data-pressed`,
  `data-focus-visible`, `data-selected` and `data-disabled`.

The full list of roots, parts and states is [anatomy.md](anatomy.md). It and
the contract themes are checked against
([`crates/caskade-theme/anatomy.json`](../../crates/caskade-theme/anatomy.json))
are generated from the native app's anatomy catalogue: each component
declares its entry beside its code, listed in `apps/native/src/anatomy.rs`
(#139). Anything not in the catalogue, including every class name, may
change without notice. What Blitz still renders differently is in
[blitz.md](blitz.md).

Tests check that:

- Every `data-cs` the native sources render is catalogued, each root once,
  and a root only contains catalogued roots.
- `anatomy.md` and `anatomy.json` are current.

After changing the catalogue, run
`CASKADE_WRITE_ANATOMY=1 cargo test -p caskade-native anatomy`.

### Compatibility with the webview's catalogue (#139)

The catalogue used to be the webview's (`packages/ui/src/anatomy.ts`), and
the native one had to match it word for word. Every root, part and state it
had is still in the contract, with these differences:

- `alphabet-bar`: a `letter` is shown only for initials some game starts
  with, so `data-disabled` is no longer set on one, and dragging along the
  bar no longer jumps letter by letter. The letter on screen still carries
  `data-selected`.
- `play-choice` gained the `play-starts` part (the Play starts select).
- Roots the native UI rendered without an entry are now catalogued:
  `activity-job`, `add-game`, `add-games`, `add-web-game`,
  `collection-dialog`, `collection-picker`, `discovery-review`,
  `discovery-settings`, `emulation`, `guide-menu`, `install-confirmation`,
  `install-root-picker`, `install-url`, `job-question`, `new-collection`,
  `pair-dialog`, `pair-request`, `playing-elsewhere`, `plugin-review`,
  `save-arriving`, `save-conflict`, `saves-panel`, `schema-settings`,
  `stop-host`, `storage-settings`, `stream-host`, `stream-pair-prompt`,
  `streaming-client`, `streaming-existing`, `streaming-program`,
  `streaming-settings`, `update-offer` and `virtual-library`.

## User settings (#141)

Settings → Appearance sets attributes and inline tokens on `<html>`. Each
attribute selects an override set, so themes keep working without knowing
about them:

| Attribute | Values | Set |
| --- | --- | --- |
| `data-effects` | `reduced`, or absent | `effects-reduced`: glass blur `0px`, grid and scanline opacity `0`, glow strength `0` |
| `data-motion` | `system`, `full`, `reduced` | `motion-reduced`: durations and stagger `0ms`, ambient loops `paused`. `system` follows `prefers-reduced-motion` (`reduced-motion` set), which `full` turns off |
| `data-density` | `comfortable`, `compact` | `density-compact`: `--cs-density-scale: 0.85`, which scales the spacing utilities |

Two values are inline styles on `<html>`:

- `--cs-ui-scale` (`0.8`–`1.5`): the root font size follows it, so `rem`
  sizes scale.
- A custom accent overrides `--cs-color-accent`, `-strong`, `-fg` and
  `--cs-color-glow`. It is checked against the theme's `surface-base` and
  `surface-raised`: below 3:1 it is lightened (dark themes) or darkened
  (light themes) in OKLCH until it passes, and refused with a message if
  it can't.

The app sets one more attribute itself: `data-paint="cpu"` when the window
paints on the CPU (WebKitGTK without GPU compositing, or the native app
without a usable GPU). Ambient loops repaint the whole screen there, so the
`paint-cpu` set pauses them (`--cs-motion-ambient: paused`) unless
`data-motion` is `full`. A theme can select on it too, e.g. to swap an
animated backdrop for a still one. The native app's backdrop is still there
whatever the motion setting ([Backdrops](#backdrops-149)).

Elements that take files dropped from the desktop (the library, path fields,
the plugin list) carry `data-drop-zone`. While files are dragged over one it
also carries `data-drop-active`; the base style outlines it with
`--cs-color-accent-strong`, and a theme can restyle it.

For these to work, theme CSS must take blur, grid, scanline, glow and
motion from the effect and motion tokens rather than literal values.

## Focus (#161)

Keyboard and pad focus share one look, built from the `focus.ring.*` tokens:

- a clear edge, `focus.ring.width` wide in `focus.ring.glow`. It is an
  outline, so focus never moves layout or replaces a component's shadows;
- `focus.ring.offset` places it: `2px` outside the element on the desktop,
  minus the width (inside) in console mode, where a scrolling shelf or a
  clipped card can't cut it;
- a glow fading inward from the edge (`focus.ring.inner`) and a soft halo
  outside it (`focus.ring.halo`), both blurs scaled by
  `effect.glow.strength`. They are `0px` on the desktop. Reduce effects
  turns the glows off and keeps the edge.

Cards whose artwork would cover an outline (game, collection and platform
cards, screenshots) draw the edge and the inward glow on an `::after` above
their content instead, cut to their rounding, and chamfered actions draw it
as a frame along the cut corners. Small controls whose own border would hide
an inner edge (the slider's thumb) keep it outside. Under forced colours the
edge is drawn in `Highlight`.

Focus shows on `:focus-visible` and the component's `data-focus-visible`. A
checkbox or switch carries the state on its root and draws the ring on a
part: `[data-cs="checkbox"][data-focus-visible] [data-part="box"]`.
A theme restyles focus through the tokens; a theme rule that moves a focused
element (the Paper theme lifts a hovered card) should leave console focus
in place, or a scrolling parent cuts its edge:
`[data-cs="game-card"]:not([data-mode="console"] *):focus-visible`.

## Backdrops (#149)

The native app draws the window's backdrop on the GPU, from data the theme
gives in `backdrop.json`: a base colour and up to eight layers (linear and
radial gradients, a grid, images, and the theme's own shaders), each with
an opacity, a blend, an elliptical mask and a motion. The fields, units and
limits are in [backdrop.md](backdrop.md) (generated). The built-in themes'
files are examples: `themes/default/backdrop.json` is Nexus's panning grid
and drifting glows.

While a layer moves, the backdrop gets frames of its own at the file's
`frameRate`: the window's last painted frame is composited over it, and
nothing is resolved, laid out or painted. The built-in themes ask for 6 a
second: their motion is a pixel or two a second, so each step is under a
pixel. Frames are also kept within 1% of a core, presenting them included:
where presenting is slow (a virtual display that copies each frame, a
remote X connection) they come further apart. A still backdrop costs
nothing after its first frame. Hidden in the tray or minimised, the window
draws nothing at all and releases its GPU resources until it is shown
again.

- **Tokens.** Colours, numbers, lengths and durations may be `{token}`
  references, read from `[data-cs="backdrop"]`'s computed style whenever the
  look, the window's size or the shown view changes. Theme tokens, the
  user's accent, colour scheme, UI scale (for `rem`) and the override sets
  all reach the GPU this way: Reduce effects hides Nexus's grid through
  `effect.grid.opacity`. `motion.ambient` set to `paused` (reduced motion,
  or `data-paint="cpu"`) keeps the backdrop still at the start of every
  motion.
- **The anchor.** `[data-cs="backdrop"]` stays in the document as the
  styling anchor. `display: none` or `visibility: hidden` on it hides the
  layers (the base colour stays) and stops their frames; its `opacity`
  multiplies them. While the GPU draws the backdrop, the document's own
  `[data-cs="backdrop"]` isn't painted: its `background`, borders, shadows,
  animations and `::before`/`::after` are ignored, as are `html` and
  `body` backgrounds (the base colour, `{color.surface.base}` by default,
  is the window's colour).
- **Custom CSS** restyles a GPU backdrop through the same contract: tokens
  (globally, or on `[data-cs="backdrop"]` alone) and the anchor's
  `display`, `visibility` and `opacity`. A CSS `background` can't become a
  GPU layer.
- **Inheritance.** A chain shows the `backdrop.json` nearest its leaf, so a
  theme that only changes tokens (Ember) shows Nexus's composition in its
  colours. A theme whose `theme.css` restyles `[data-cs="backdrop"]` and
  has no `backdrop.json` of its own stops the search: its CSS backdrop is
  drawn, as below. To change an inherited composition, ship a whole
  `backdrop.json`; layers aren't merged.
- **CSS backdrops.** Without a usable GPU (the window draws on the CPU), on
  macOS for now (the renderer doesn't keep the scene there), for a theme
  with no composition (Paper), and in the webview, the theme's CSS backdrop
  is drawn by the document, with its animations paused: the theme's look
  at the start of its motion, with no ambient repaint loop. Keep a
  `theme.css` backdrop that matches the composition's first frame: it is
  the still form of the theme. Existing themes keep working unchanged; an
  animated CSS backdrop is shown still until the theme adds a
  `backdrop.json`.
- **Shaders.** A `"type": "shader"` layer runs the theme's own WGSL
  function from `assets/`, given the pixel, the backdrop's size, a looping
  time and up to four colours and numbers (tokens, so the shader follows
  the accent and Reduce effects). It is a pure function: no bindings or
  textures, so it can't read the window or anything else. `caskade-theme
  lint` validates it. It runs only when the backdrop moves, and one that
  costs more than its GPU budget stops moving. Details in
  [backdrop.md](backdrop.md#shaders).

`CASKADE_BACKDROP=still` draws the CSS backdrop, still, on a GPU too, and
`CASKADE_BACKDROP=css` draws it animated as before #149: both for
comparing and measuring.

## Theme packages (#139)

A theme is a folder: tokens, CSS, fonts and images, never code.

```
themes/neon/
├── theme.json   # required
├── tokens.json  # optional: DTCG overrides of declared tokens
├── theme.css    # optional: rules on anatomy selectors
├── backdrop.json # optional: the backdrop the native app draws on the GPU (#149)
├── assets/      # images and fonts that theme.css and the previews use, and backdrop shaders (.wgsl)
└── locales/     # optional: <lang>.po translations of the name and description
```

`theme.json`:

| Field | | |
| --- | --- | --- |
| `id` | required | Reverse-DNS, like plugin ids: `dev.example.neon`. |
| `name` | required | 1 to 64 characters. |
| `version` | required | `major.minor.patch`. |
| `extends` | optional | The theme this one restyles on top of, usually `caskade.default`. Without it only the contract's default tokens lie underneath. |
| `modes` | optional | Colour schemes it provides, its default first: `dark`, `light`. Default `["dark"]`. |
| `previews` | optional | Images in the folder, shown by the theme picker. |
| `description`, `author` | optional | Text for the picker. |

The core reads themes from these places:

- the built-in themes: `caskade.default` (Nexus, `themes/default` in the
  repository) first, then Cobalt, Emerald and Engage
  (`caskade.cobalt`, `caskade.emerald`, `caskade.engage`, #125). A
  choice of a built-in id that's gone (Paper's, or the ids these three
  had before their rename) moves to its replacement without a notice;
- the folders under `<app data>/themes/`;
- folders bundled by plugins;
- folders loaded in place in [dev mode](#dev-mode).

`crates/caskade-theme` checks every theme. A broken theme is listed with its
problems, each naming the file and, where it has one, the line and column. A
broken theme never hides the others. The rules:

- `tokens.json` may only override tokens the contract declares
  ([tokens.md](tokens.md)), with values of their type. An alias
  (`"{color.palette.cyan}"`) becomes `var()`.
- `theme.css` selectors use `data-cs`, `data-part` and the other contract
  attributes ([anatomy.md](anatomy.md)), plus pseudo-classes and
  pseudo-elements. These are errors:
  - class, id and element selectors;
  - unknown roots, parts or attributes;
  - the consent dialogs, `[data-cs="plugin-review"]` and
    `[data-cs="install-confirmation"]`, anywhere in a selector (`:has()`
    and `:is()` included): a theme could hide what the user agrees to, add
    false reassurance or swap the buttons;
  - undeclared `--cs-*` names;
  - `!important`;
  - `@import`;
  - at-rules other than `@media`, `@supports`, `@container`, `@keyframes`
    and `@font-face`.
- A `url()` must name an image or font under `assets/`. Remote, absolute
  and escaping URLs are refused.
- `backdrop.json` must match its schema ([backdrop.md](backdrop.md)): known
  fields, values of their type, declared tokens of the right type, images
  and shaders that exist under `assets/`, and shaders that compile and
  declare nothing but functions, structs and constants.
- Scripts, HTML and binaries anywhere in the folder are refused.

A theme compiles, together with everything it `extends`, into one
stylesheet: the tokens on `:root`, then each theme's CSS, base first, all
inside `@layer cs.theme`. The layer beats component and plugin styles and
loses to the user's CSS. A theme's `@font-face` rules come first, outside
the layer: a layer orders the cascade, which fonts aren't part of, and the
native app only loads the fonts of a stylesheet's top-level rules. The app loads the stylesheet from
`caskade-theme://localhost/<id>/theme.css`
(`http://caskade-theme.localhost/…` on Windows), which also serves the
theme's `assets/` and previews.

Check a theme while writing it:

```sh
cargo run -p caskade-theme --features cli -- lint path/to/theme
cargo run -p caskade-theme --features cli -- build path/to/theme themes/default
```

`build` prints the compiled stylesheet and resolves `extends` among the
folders given. The default theme is itself written only against this
contract: Nexus's backdrop grid, HUD corners, scanlines and card glow live
in `themes/default/theme.css`.

### Translations (#219)

A theme's `name` and `description` can show in the user's language. Put a
gettext catalog per language in `locales/<lang>.po`, named by its BCP 47
tag (`sv.po`, `pt-BR.po`), in a domain named after the theme's id. The
English text in `theme.json` is the message id:

```po
msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Neon"
msgstr "Neonljus"
```

The app looks the texts up in the language Caskade is in (`pt-BR` falls
back to `pt`), and shows the English for anything missing, empty or
fuzzy. A theme without `locales/` is English and has no problems. A
catalog that can't be read, isn't named after a language, or has a
translation with a placeholder its English lacks, is one of the theme's
`warnings` (`ThemeInfo.warnings`): the theme still applies, in English.
`caskade-theme lint` reports those files and fails on them.
`locales/<id>.pot`, if you keep a template, is ignored. A theme has no
other text to translate: everything it shows comes from the app.

[Writing a theme](writing-a-theme.md) builds the light Paper theme (`themes/paper`) from an empty folder, step by step.

## Switching themes (#140)

Settings → Appearance lists every installed theme with its name, author,
description and previews. A theme that can't be used shows its problems and
can't be picked. Picking one applies it at once: the app swaps the one
`<link>` to the compiled stylesheet, without a reload.

- The choice is `core.appearance.theme`, a theme id. Older settings that
  named `nexus` or `terminal` become `caskade.default`.
- On start the core resolves the choice (`theme_resolve`) and the app applies
  it before the first render, so the default never flashes in.
- A chosen theme that was removed, or no longer compiles, falls back to
  `caskade.default`. Appearance shows a notice saying why. The setting is
  kept, so the theme comes back once it is reinstalled or fixed.
- `<html data-color-scheme>` is `dark` or `light`: the OS preference when
  the theme's `modes` include it, otherwise the theme's first mode. Next to
  it, `data-mode` is `desktop` or `console`. Themes restate tokens for
  either with override selectors such as `[data-color-scheme="light"]`.

### Dev mode

In a development build, `caskade dev path/to/theme` (`dev` as the first
argument), Appearance → "Load theme folder" or a theme folder dropped on
the theme list loads a theme folder in place and watches it, like a dev
plugin. Release builds ignore `caskade dev`. Each save recompiles the theme within a second and restyles the
app if it is the active one. A broken save is logged, and the app falls back
to the default theme until the theme compiles again. A dev theme wins its id
over an installed copy, so a checkout of an installed theme can be worked on
in place.

## Custom CSS (#142)

Settings → Appearance → Custom CSS holds your own rules
(`core.appearance.customCss`, up to 32 Ki characters). Saving applies them
without a reload, as one `<style>` in `@layer cs.user`. That layer comes
after `cs.theme` and `cs.components`, so a rule there beats the same
property set by the theme or by a component without `!important`.

- Style through the same contract as themes: the
  [anatomy selectors](anatomy.md) and the [tokens](tokens.md). Class names
  aren't stable.
- Nothing may load from other origins. A remote `@import` or `url()` refuses
  the whole sheet, with a message naming the line. Relative, `data:` and
  Caskade's own URLs are allowed. `@import` can't work inside the layer, so
  it is refused too, and so is a stray `}` that would end the layer early.
- "Disable custom CSS" keeps the rules but stops applying them.
- **Safe mode** starts Caskade with the default theme and without custom
  CSS, to recover from a broken look: `caskade --safe-mode`, or
  `CASKADE_SAFE_MODE=1`. Nothing is changed in the settings.

## M3 exit test (#145)

The native screenshots (`cargo test -p caskade-native --features shots --test shots`,
[docs/native-testing.md](../native-testing.md)) render the reference screens
and the component gallery under every built-in theme and a catalogue theme,
and compare them with committed images, so a theme that stops restyling a
view through the contract shows up as a changed screenshot. The webview's
Chromium script that did this before #143 was retired with the webview app.

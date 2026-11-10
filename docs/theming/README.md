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
[`@caskade/ui`](../../packages/ui) and shell component renders:

- `data-cs="<name>"` on its root, such as `[data-cs="game-card"]`.
- `data-part="<part>"` on its parts, such as
  `[data-cs="game-card"] [data-part="title"]`. A part belongs to the nearest
  enclosing root.
- Domain state on the root, such as `data-status="installed"`,
  `data-variant` or `data-tone`.
- React Aria's interaction state, such as `data-hovered`, `data-pressed`,
  `data-focus-visible`, `data-selected` and `data-disabled`.

The full list of roots, parts and states is [anatomy.md](anatomy.md). It is
generated from the catalogue in `packages/ui/src/anatomy.ts`, which plugins
can also import from `@caskade/ui`. Anything not in the catalogue, including
every class name, may change without notice. The native app (#90) renders the
same roots, parts and states, catalogued again in `apps/native/src/anatomy.rs`
and checked against `anatomy.ts`; themes apply to both unchanged. What
Blitz still renders differently is in [blitz.md](blitz.md).

Tests check both directions:

- Each component in each documented state renders exactly the parts and
  states its catalogue entry lists.
- The stylesheets of the app and `packages/ui` may only use catalogued
  `data-cs` and `data-part` selectors, and no class or id selectors.

After changing the catalogue, run `pnpm --filter @caskade/ui docs`.

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

The app sets one more attribute itself: `data-paint="cpu"` when the webview
paints on the CPU (WebKitGTK without GPU compositing). Ambient loops repaint
the whole screen there, so the `paint-cpu` set pauses them
(`--cs-motion-ambient: paused`) unless `data-motion` is `full`. A theme can
select on it too, e.g. to swap an animated backdrop for a still one.

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

Focus shows on `:focus-visible` and React Aria's `data-focus-visible`. A
checkbox or switch carries the state on its root and draws the ring on a
part: `[data-cs="checkbox"][data-focus-visible] [data-part="box"]`.
A theme restyles focus through the tokens; a theme rule that moves a focused
element (the Paper theme lifts a hovered card) should leave console focus
in place, or a scrolling parent cuts its edge:
`[data-cs="game-card"]:not([data-mode="console"] *):focus-visible`.

## Theme packages (#139)

A theme is a folder: tokens, CSS, fonts and images, never code.

```
themes/neon/
├── theme.json   # required
├── tokens.json  # optional: DTCG overrides of declared tokens
├── theme.css    # optional: rules on anatomy selectors
├── assets/      # images and fonts that theme.css and the previews use
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

- the built-in themes, `caskade.default` (`themes/default` in the
  repository) first;
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
- Scripts, HTML and binaries anywhere in the folder are refused.

A theme compiles, together with everything it `extends`, into one
stylesheet: the tokens on `:root`, then each theme's CSS, base first, all
inside `@layer cs.theme`. The layer beats component and plugin styles and
loses to the user's CSS. The app loads the stylesheet from
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

`pnpm --filter @caskade/desktop test:themes` checks that a second theme
restyles every core view through the contract alone. It starts the fixture
dev server (`VITE_CASKADE_FIXTURES=1`) on a free port, compiles
`themes/default` and `themes/paper` with `caskade-theme build` (under the
heavy lock: don't wrap the command in `flock`), and opens Home, Library grid
and list, a game, Store, Collections, a collection, Activity, the plugin
route and every Settings tab in Chromium (`playwright-core`, no browser
download: `--chrome <path>` or `CHROME_PATH`). Each theme is applied the way
the app applies one, as the single `[data-cs-theme]` stylesheet, with its
assets served on `caskade-theme.localhost`.

For every view it records the computed colours, fonts, radii, borders and
effects of each `data-cs` root and its parts. It fails when a view shows no
colour, typography or radius change, when a root looks the same under both
themes and isn't in the script's allowlist, when a token the theme declares
doesn't compute on `<html>`, when an element built on a radius or font token
doesn't take the theme's value, or when the theme's font doesn't load.
Screenshots (`<view>-<theme>.png`) and the coverage report
(`coverage.json`, `coverage.md`) are written to `target/theming/`. Other
themes: `--base <dir> --theme <dir>`; precompiled sheets:
`--css <dir>=<file>`; a running fixture server: `--url <origin>`. The fixture
build has no plugins, so the plugin route shows only the shell. The runs
cover Chromium on Linux; WebView2 and WKWebView are deferred, to W9 and M2 in
`docs/manual-checks.md` (#8 lets them wait).

# BLITZ-6: sticky, container queries, backdrop blur, fixed and transitions (2026-11-10)

Issue #85. Five things today's UI uses that Blitz, at the BLITZ-0 pin, lacked
or got wrong, each resolved before the views are ported (#96 onwards): by a
patch in the series in [`vendor/blitz/`](../../vendor/blitz/README.md), by a
fix already in the pin, or by a design rule for the native views.

## Resolutions

| Feature | Used in today's UI | Resolution | Validation |
| --- | --- | --- | --- |
| `position: sticky` | the library list's header row (`VirtualLibrary`), the settings nav (`SettingsView`), the selection bar (`SelectionBar`) | **Fixed in Blitz:** series patch `blitz/0024`, blitz#887 (by LinuxBoy-96, still open upstream) ported to the pin. Sticky boxes are laid out as relative; after layout their offsets come from the nearest scroller (or the viewport) and the containing block, and every scroll and paint applies them in paint, hit testing and the geometry APIs (so screen readers get the stuck place too). Use it as in a browser. One limit, from the PR: a sticky `<thead>` in a real `display: table` table paints under later rows; give a sticky header row its own stacking (`z-index`, as the library list's block `thead` has) | `tests/blitz-tests/tests/position_sticky.rs` (7 tests, in the patch); `apps/native/tests/design_resolutions.rs` |
| Container queries (`@container`, `@xl:` …) | 10 settings files: two-column grids from `@xl`, a side column from `@3xl`/`@4xl`, `StackedTable`'s table/cards switch at `@6xl` | **Design:** native views don't use container queries (Blitz/Stylo don't run them: they need a restyle after layout). Two columns from a width: an intrinsic grid, `grid-cols-[repeat(auto-fit,minmax(17rem,1fr))]` (two 17rem tracks and the gap make the `@xl` 36rem). A side column: a wrapping flex row (`flex flex-wrap`, the main column `flex-[1_1_30rem]`, the side `flex-[0_1_18rem]`). A layout switch (`StackedTable`): a viewport media query at the container breakpoint plus the width of the chrome around the panel, which is fixed in the shell | `design_resolutions.rs`: the grid switches at the same width, the side column wraps under, the media query switches the table |
| `backdrop-filter` (`backdrop-blur`, 59 files) | glass surfaces: the top bar, the selection bar, overlays, badges and hero buttons over art | **Design:** the native UI draws no backdrop blur (`vello_gpu` 0.3 applies `filter` but not `backdrop-filter`; only the Skia backend draws it). Glass keeps its colour and opacity; **glass over imagery is at least 80% opaque** (`/80` and up). Measured against today's blurred look (blur simulated at the 8 px our tokens cap it to): at 80% opacity an unblurred surface differs from the blurred one by more than 24 levels in 0.8% of the glass area over the library screen and 0.1% over detailed art; at 70% over art, 37.6% (75–80% unblurred is the closest to today's 70% blurred: 23–26%). The seven `bg-surface-overlay/70` glass uses go to `/80` when ported | the simulation below; `effect.glass-blur` already turns blur off for Reduce effects and reduced transparency, which the guidelines require to read the same |
| `position: fixed` against the viewport (Blitz #690) | toasts, dialogs, the status bar | **Fixed in the pin:** blitz#922 (out-of-flow hoisting) resolves fixed boxes against the viewport, inside positioned ancestors, and keeps them there while the page scrolls | Blitz's `oof_dynamic_cb.rs`; `design_resolutions.rs` (a fixed bar inside a positioned ancestor stays on the window's bottom while the page scrolls) |
| Transitions on properties a stylesheet sets (Blitz #689) | every `transition-*` utility | **Fixed in the pin:** blitz#695 doesn't resolve styles while render-blocking stylesheets load, so no transition starts from pre-stylesheet values | Blitz's `render_blocking_stylesheet.rs` |

Options weighed for backdrop blur, and why not:

- **Switch to Skia** (draws `backdrop-filter`): gives up vello_hybrid, which
  BLITZ-0 measured and BLITZ-3 made cheap with damage-limited repaint
  (#82); every glass surface would cost a blur each frame it repaints.
- **Draw it in Blitz**: paint what lies behind a glass element again into a
  `vello_gpu` filter layer, clipped to the element. Feasible, but it
  re-encodes the backdrop for every glass surface on every frame that
  touches it, and the measurement below shows the look barely needs it.

## Measurement

**Against eng3's theme screenshots** (#90, posted on #85: Nexus 3.6%,
Ember 3.1%, Paper 14.3% of pixels differing by more than 24 levels from
WebKitGTK at 1280×800): none of these resolutions moves them. Rerun with
the series including 0024 on 10 Nov: 3.6%, 14.3%, 3.1%. The reference
screen has no sticky, container-query or fixed element, and no element
on it uses `backdrop-blur`. WebKitGTK under Xvfb (no accelerated
compositing) and headless Firefox don't draw `backdrop-filter` either:
their renders with and without it are identical, so the deviations
attributed to glass in `docs/theming/blitz.md` come from elsewhere.
Paper's 14% is SVG filters in its grain image, a background image
repeated at `background-size` (only the first tile is drawn) and hairline
gradient stripes, none in #85's list.

**Backdrop blur, simulated:** the library screen and a detailed image
(standing in for cover art scrolling under the top bar), with three glass
surfaces (top bar, a centred panel, a bottom bar) composited at the
surface colours. Today's look is the surface over a Gaussian blur (σ 8 px)
of what's behind; the native look is the same surface without the blur.

| Glass | Over | Differs by > 24 levels (share of glass) | Mean difference (screen) |
| --- | --- | ---: | ---: |
| `surface-base`/80 | library screen | 0.8% | 0.33 |
| `surface-base`/80 | detailed art | 0.1% | 4.24 |
| `surface-base`/70 | library screen | 1.0% | 0.47 |
| `surface-base`/70 | detailed art | 37.6% | 6.34 |
| today's /70 blurred against /75 unblurred | detailed art | 22.7% | 5.53 |
| today's /70 blurred against /80 unblurred | detailed art | 25.8% | 5.67 |

CPU: no blur means no cost; the resolution adds nothing to a frame.

## Upstream

Submitted on 10 Nov 2026 where upstream didn't already have the bug or a fix;
each item's upstream issue or PR and its state is in [#127](https://github.com/krazyjakee/caskade/issues/127). The
drafts below are kept as written.

The drafts:

- **DioxusLabs/blitz#887 "Implement position: sticky"** is LinuxBoy-96's
  open PR; `blitz/0024` is its port to our pin, with seven tests. Offer
  them on the PR, with the stacking limit of sticky `<thead>` cells in
  real tables.
- **DioxusLabs/blitz: "Treat an image with an empty alt as decorative"**
  (`blitz/0023`, from #93): `<img alt="">` maps to `role="none"`, as
  HTML-AAM has it.
- Backdrop blur in `vello_gpu`/anyrender: an issue, not a patch, if the
  user wants blur back later.

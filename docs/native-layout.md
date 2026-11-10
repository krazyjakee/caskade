# Laying out the native UI (RSX on Blitz)

How to build views for the native app (Dioxus RSX rendered by Blitz) so they
lay out as the webview does and stay fast with large libraries. The look
(tokens, components, anatomy) is in [DESIGN-GUIDELINES.md](DESIGN-GUIDELINES.md);
input and focus are in the [tech spec](TECH-SPEC.md) (*The native UI's input*).

## Structure and look

- **Layout that makes a component work goes in inline `style`; how it looks
  goes in classes.** A scroll container, absolutely placed rows, a grid's
  `grid-template-columns`: inline, so the component works without a
  stylesheet (headless tests have none) and nothing in a theme can break it.
  Colours, type, spacing, radii and borders: the `--cs-*` token utilities, as
  in the webview.
- New classes need `just native-css` (`apps/native/scripts/build-css.mjs`),
  run from what you commit: CI checks `assets/app.css` and `shots.css` are
  current.

## Scroll areas in flex columns

- The webview's "fill the view with a scroll area" pattern works as it is:
  an auto-height `flex min-h-full flex-col` holding a `flex min-h-96 grow
  basis-0` row with an `overflow-auto` child. Taffy used to size that column
  from the row's content (#81; `docs/spikes/blitz-2.md`); the vendored
  Taffy patch fixes it, and `apps/native/tests/layout.rs` holds Chromium's
  numbers. If a scroll area ever grows to its whole content again, check that
  test first; the old workaround was `height: 0` on the growing row.

## Long lists: virtualise

- **Anything that can list a library's worth of items is virtualised**: the
  library's grid and list use `library::VirtualLibrary` (#94). It mounts only
  the rows on screen, one grid row or six list rows each side, and the focused
  item's row, and loads only the 100-game pages those need, keeping a few each
  side. Scrolling 10,000 games keeps a few dozen cards mounted.
- Rows have **fixed heights** (312 px cards, 44 px list rows), so the scroll
  position says which rows show without measuring them; columns follow the
  width (`floor((width + 16) / 192)`, as the webview's card layout).
- **Thumbnails, never originals**: cards show the 320 px variant and rows the
  96 px one (`library::art::url`), served from the artwork cache on
  `caskade-asset://`.

## Scrolling and measuring

- **Blitz sends no scroll event for a scroll made from code**
  (`BaseDocument::scroll_to`, `scroll_into_view`). After scrolling a box
  yourself, call `Input::scrolled_by_code()`; components that follow scrolling
  read `Input::scrolled()` (it changes after the input layer's scrolls too) and
  the box's offset from the document (`node.scroll_offset()`).
- **Layout comes a frame after mounting**: an element's box is known once the
  document has been laid out, not in `onmounted`. Read
  `get_client_bounding_rect` through the element's `NodeHandle` at render
  time, and re-render after the first frame (the grid retries for up to
  half a second).
- Focus must not vanish when its element scrolls out: keep the focused item's
  row mounted, as the grid does. A jump (LT/RT, a letter) scrolls first and
  focuses the target once it's mounted (`Input::focus`).

## Measuring

- `apps/native/scripts/library-scroll.sh` runs the library's scroll benchmark
  on a private desktop: the release binary in fixtures mode with 10,000 games
  scrolls its grid and list top to bottom a step per frame, and reports frame
  times and the most games mounted. On Xvfb the frames are the CPU renderer's;
  whether a real display's refresh rate holds is a manual check (#120),
  which `--here` runs on the current display (a person at the machine only).

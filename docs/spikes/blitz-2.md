# BLITZ-2: a column flexbox's auto height ignored `flex-basis: 0` children (#81)

## What went wrong

The Blitsen spike ([blitz.md](blitz.md#blitsen-the-current-react-ui-on-blitz))
found Blitz laying the library out much taller than Chromium. The view's
content column is `flex min-h-full flex-col` (auto height). The library's
growing row is `flex min-h-96 grow basis-0`, and holds the `overflow-auto`
scroller of the virtual grid. Chromium sizes the column from the row's flex
basis and its min-height: 558 px, the view's height. Taffy sized it from the
row's content, 55,224 px. The scroller grew to its whole content, so TanStack
Virtual saw a 55k-px viewport and mounted all 8,429 cards (218 s, 1.1 GB).

## Reduced case

```html
<div id="view" style="height: 600px; overflow-y: auto">
  <div id="column" style="display: flex; flex-direction: column; min-height: 100%">
    <div id="row" style="display: flex; flex: 1 1 0px; min-height: 384px">
      <div id="scroller" style="flex-grow: 1; overflow: auto">
        <div style="height: 55000px"></div>
      </div>
    </div>
  </div>
</div>
```

| | `#column` | `#row` | `#scroller` |
| --- | --- | --- | --- |
| Chromium (headless shell 1248) | 600 | 600 | 600 |
| Blitz at 9f9c8e3 (Taffy 4aca4f4) | 55,000 | 55,000 | 55,000 |
| Blitz with the Taffy patch | 600 | 600 | 600 |

`apps/native/tests/layout.rs` asserts the patched numbers, through Dioxus
Native and Blitz.

## Cause

Taffy works out an auto-height column container's intrinsic main size with the
spec's flex-fraction algorithm
([§9.9.1](https://drafts.csswg.org/css-flexbox-1/#intrinsic-main-sizes)), from
each item's max-content contribution: the row's content, 55,000 px. Chrome
instead sums the items' hypothetical main sizes: the flex base size clamped by
max-height and floored by min-height. Here that is max(0, 384) = 384, then
the column's own `min-height: 100%` makes it 600. Where `min-height` is
`auto`, the automatic minimum is the item's content size, so the content
still counts (the second fixture below checks that).

## The patch

`vendor/blitz/patches/taffy/0001-*.patch` (Taffy at 4aca4f4). For a column
container sized under min- or max-content, an item contributes its
hypothetical main size: `flex_basis.min(max).max(resolved_minimum_main_size)`
plus margins, without measuring its content.

Taffy's suite passes with it, all 6,689 Chrome-generated fixtures included.
The patch adds two fixtures, generated from Chrome with Taffy's
`test_helper.js` (headless shell, scrollbars hidden):

- `flex_column_auto_height_from_basis_zero_with_min`: the reduced case
  (`overflow: scroll`, since Taffy has no `auto`). It fails before the
  change and passes after.
- `flex_column_auto_height_from_basis_zero_auto_min`: the same item without
  a min-height, where the automatic minimum keeps the content's 200 px. It
  passes either way.

## Upstream (not filed: waiting for the user's go)

A Taffy issue, with the patch as its pull request:

> **Column flex container with auto height sizes from items' content instead of their hypothetical main sizes**
>
> A column flex container with `height: auto` gets its intrinsic main size
> from the flex-fraction algorithm over its items' content. Chrome sums the
> items' hypothetical main sizes (flex base size clamped by min/max) instead.
> The two disagree when an item has `flex: 1 1 0px`, an explicit
> `min-height`, and taller content.
>
> ```html
> <div style="display: block; width: 400px; height: 600px; overflow: scroll">
>   <div style="display: flex; flex-direction: column; min-height: 100%">
>     <div style="display: flex; flex: 1 1 0px; min-height: 384px">
>       <div style="flex-grow: 1; overflow: scroll"><div style="width: 10px; height: 55000px"></div></div>
>     </div>
>   </div>
> </div>
> ```
>
> Chrome: the column, the item and the scroller are 600 px tall. Taffy: 55,000 px.
> A scroll area that fills a view this way grows to its whole content, so a
> virtualised list inside it mounts every row.
>
> The fix uses the hypothetical main size for column containers under min-/max-content
> sizing. Taffy's suite passes, including every Chrome-generated fixture.
> Two Chrome-generated fixtures are added.

WPT-style test (`testharness.js` with `check-layout-th.js`):

```html
<!DOCTYPE html>
<title>An auto-height column flex container sizes from its items' hypothetical main sizes</title>
<link rel="help" href="https://drafts.csswg.org/css-flexbox-1/#algo-main-container">
<meta name="assert" content="A flex: 1 1 0px item with an explicit min-height doesn't size an auto-height column container from its content.">
<script src="/resources/testharness.js"></script>
<script src="/resources/testharnessreport.js"></script>
<script src="/resources/check-layout-th.js"></script>
<body onload="checkLayout('#view')">
<div id="view" style="height: 600px; overflow-y: auto" data-expected-height="600">
  <div style="display: flex; flex-direction: column; min-height: 100%" data-expected-height="600">
    <div style="display: flex; flex: 1 1 0px; min-height: 384px" data-expected-height="600">
      <div style="flex-grow: 1; overflow: auto" data-expected-height="600">
        <div style="height: 55000px"></div>
      </div>
    </div>
  </div>
</div>
</body>
```

## Workaround (not needed with the patch)

Before the patch, `height: 0` on the growing row gave Chromium's geometry, as
the spike found (the prototype still applies it unless
`CASKADE_PROTO_FLEX_BUG=1`). With the patch in the vendored series
(`vendor/blitz/patches/taffy`), RSX can use the webview's classes as they are.

# BLITZ-1: `text-overflow` ellipses and input placeholders (#80)

## What was missing

Blitz clipped text that didn't fit with no "…" (36 webview files use
Tailwind's `truncate`), and showed nothing in an empty field with a
`placeholder`. Both are in the vendored series now (`vendor/blitz`):

| Patch | What |
| --- | --- |
| `blitz/0021` Implement text-overflow (ellipsis and string markers) | blitz#893 by Hugo (`LinuxBoy-96`), head `51fcffbe` (10 Nov 2026: open, changes requested by nicoburns on 27 Sep, answered by later pushes), ported to our pin `9f9c8e3`. |
| `blitz/0022` Show text inputs' placeholders in the `::placeholder` style | Ours. |

## blitz#893 on our pin

The PR's last push follows the review: the marker is computed after the
Parley layout (`layout/inline/layout.rs`) and stored on the `TextLayout`
(`overflow: Option<Box<TextOverflowLayout>>`), the non-paint code is its
own module (`layout/text_overflow.rs`), the marker is shaped in the block
container's style and painted in its colour on the line's baseline, and
overflow is measured without trailing whitespace. Its base (`7931e67`) is
older than our pin, so it didn't apply as it is. The port, kept in Hugo's
name:

- `layout/inline.rs` is `layout/inline/layout.rs` at our pin; the
  `overflow` block goes after `align`, and it locks a clone of the font
  context's `Arc` (the PR borrowed `self.font_ctx` while `self` was
  borrowed mutably).
- `stroke_text` takes `TextPaintOptions` at our pin; the PR's
  `text_overflow` argument goes after it, and every caller passes it
  (`None` for the clip pass, text inputs and list markers).
- `MarkerRun` clones the run's `FontData` (`run.run().font().clone()`);
  `CopyStyleGetters` comes from `stylo_taffy`.

## Placeholders

Stylo's servo build computes `::placeholder` lazily (it's not among the
five eager pseudo-elements), so Blitz asks for it when it builds an
input's editor (`Stylist::lazily_compute_pseudo_element_style`) and lays
the placeholder out in a second `PlainEditor` with that style, sized and
scaled with the input's own. Paint draws it, under the caret, while the
raw text is empty and no composition is in progress: an IME's preedit in
an empty field hides it, as in Chromium. Its colour is the pseudo-element's
`color` with its `opacity`. A single-line input's placeholder loses its
line breaks; a `placeholder` attribute change rebuilds the input's box, as
`value` does. The UA sheet gives `::placeholder` Chromium's light grey
(`#757575`). Not done: `:placeholder-shown` (Stylo has the state; Blitz
doesn't set it).

## Validation

- `apps/native/tests/text_pixels.rs` (`--features shots`): one page, a
  title cut with an ellipsis, the same clipped, a field with a themed
  placeholder, one with the default and one with a value, rendered by
  Blitz with vello_cpu and by headless Chromium 156 (Chrome for Testing
  156.0.8078.4, `scripts/text-oracle.sh`, committed as
  `tests/text/chromium.png`), both in the bundled Inter. Measured the same
  way: three dots at the cut title's end, within 3 px of Chromium's; none
  on the clipped one; the placeholders' colours within 30 of (220, 0, 0)
  and (117, 117, 117) and their extents within a few pixels; black text
  when there's a value. The two images differ in 247 of 104,000 pixels
  (anti-aliasing; at most 58 in a channel). A second page does it with
  the app's stylesheet and classes (`truncate text-sm font-semibold`, the
  text field's `placeholder:text-fg-subtle`): the title's ellipsis lands
  where Chromium's does and the placeholder is the default theme's
  `--cs-color-fg-subtle` in both (`tests/text/chromium-app.png`).
- `apps/native/tests/text_display.rs`: headless, a cut title's layout has
  its ellipsis and a short one has none; a placeholder shows when empty
  and focused, not while composing or with a value, comes back when
  emptied, and follows a new `placeholder` attribute.
- The theme screenshots (`tests/shots`) now show the library card's cut
  title ending in "…" (the only pixels that changed).

## Upstream

Submitted on 10 Nov 2026 where upstream didn't already have the bug or a fix;
each item's upstream issue or PR and its state is in [#127](https://github.com/krazyjakee/caskade/issues/127). The
drafts below are kept as written.

### A comment on blitz#893

> We've been carrying this PR (at `51fcffbe`) on top of `9f9c8e3` in an app
> and it does what we need: Tailwind's `truncate` ends titles in "…" in
> cards and list rows, matching Chromium within a few pixels
> (we compared vello_cpu renders with headless Chromium's). Rebasing it
> needed three small changes, in case they help: `inline.rs` is now
> `inline/layout.rs` (the overflow block goes after `align`, and locks a
> clone of the font context's `Arc` to avoid borrowing `self` twice);
> `stroke_text` now takes `TextPaintOptions`, so the new argument goes
> after it; and `MarkerRun` needs `run.run().font().clone()`.

### Pull request: "Show text inputs' placeholders in the `::placeholder` style"

> An `<input>` or `<textarea>` with a `placeholder` showed nothing while
> empty. This shows it while the value is empty and no IME composition is
> in progress (an empty value with a preedit shows the preedit), under the
> caret, in the `::placeholder` style.
>
> - Stylo computes `::placeholder` lazily, so it's resolved with
>   `Stylist::lazily_compute_pseudo_element_style` when the input's editor
>   is built, and the placeholder is laid out in a second `PlainEditor`
>   (`TextInputData::placeholder`), refreshed with the input's width and
>   scale.
> - Paint draws it in the pseudo-element's colour (times its opacity),
>   centred as the input's text is.
> - HTML strips line breaks from a single-line input's placeholder.
> - The UA sheet gives `::placeholder` Chromium's light-scheme grey,
>   `#757575` (Firefox uses `opacity: 0.54` instead).
> - A `placeholder` attribute change rebuilds the input's box, as `value`
>   does.
>
> Not included: matching `:placeholder-shown`.
>
> Tested in an app against headless Chromium (colour and extents within a
> few pixels) and headlessly for the IME case; a `blitz-tests` test can come
> with it.

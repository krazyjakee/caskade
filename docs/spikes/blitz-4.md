# BLITZ-4: what screen readers get from Blitz (2026-11-10)

Issue #83. Caskade's native UI reaches screen readers through Blitz's
AccessKit bridge: AccessKit builds the platform's tree (AT-SPI on Linux, UI
Automation on Windows, NSAccessibility on macOS) from the nodes Blitz gives
it. At the BLITZ-0 pin, Blitz gave roles and a few attributes (label,
description, disabled, expanded, hidden, level, selected), no bounds, and
dropped every action a screen reader asked for. Patches 0006–0010 of the
series in [`vendor/blitz/`](../../vendor/blitz/README.md) close the gaps
Caskade's components need.

## What changes

| Patch | Effect |
| --- | --- |
| `blitz/0006` Expose ARIA states and properties to AccessKit | checked and pressed (`aria-checked`, `aria-pressed`, native checkboxes and radios) as toggled; `aria-current`, `-modal`, `-busy`, `-invalid`, `-required`, `-readonly`, `-multiselectable`, `-haspopup`, `-orientation`, `-posinset`, `-setsize`; live regions (`aria-live`, `aria-atomic`, the implicit values of `alert`, `status`, `log`); values (`aria-value*`, `<progress>`, `<meter>`, range and number inputs, a text input's value, masked for passwords, and its placeholder); `aria-activedescendant` and `aria-controls`; native `disabled`, `required`; a link's URL; `role` as a token list and 13 more roles. A checked state or a text value changing marks the tree changed. |
| `blitz/0007` Give AccessKit nodes their bounds | each element's border box (CSS px, viewport-relative; the window node scales to physical px); scrolling and resizing mark the tree changed |
| `blitz/0008` Route screen-reader actions to DOM events | nodes declare focus, click, scroll into view, expand/collapse and increment/decrement; the shell turns AccessKit's requests into `UiEvent::Accessibility`, and the event driver does them as the user's input would (focus and blur events; focus then a click event with its default action; arrow keys for range controls). A click with no default action keeps the focus when it lands in the focused element, rather than clearing it. |
| `blitz/0009` Name form controls by their labels | a control without `aria-label`/`-labelledby` takes its `<label>`'s text (around it or with `for`), as HTML-AAM says |
| `blitz/0010` Mount a Dioxus app in a div, not a main element | `DioxusDocument` wrapped every app in `<main>`, so screen readers announced a second main landmark around the app's own |

Upstream Blitz already refreshes the tree after DOM and focus changes (#1456,
in the pin). Animation frames don't mark the tree changed, so a running
spinner doesn't rebuild it.

## Verified here

- `tests/blitz-tests` in the Blitz checkout (`accessibility_states.rs`,
  `accessibility_actions.rs`, in the patches): the whole suite, 357 tests,
  passes with the series.
- [`apps/native/tests/accessibility.rs`](../../apps/native/tests/accessibility.rs)
  in Caskade's CI: the AccessKit tree of demo elements built as Caskade's
  components build theirs (navigation with the current page, toggle and
  menu buttons, a labelled checkbox, a slider, a required invalid search
  field, tabs, a modal dialog, a busy status with a progress bar, an alert)
  as a text snapshot; and a screen reader's click, expand, increment and
  focus reaching the Dioxus handlers, with the tree following.

## Upstream pull requests

Not submitted: they go out under the user's name once approved. Each is one
patch file in `vendor/blitz/patches/blitz/`, with its tests.

**DioxusLabs/blitz: "Expose ARIA states and properties to AccessKit"**
(`0006`). The bridge mapped roles and a handful of attributes; screen
readers also need the state and value of controls and how regions relate.
Adds the states and properties above, resolving `aria-activedescendant` and
`aria-controls` after every node is built (as `aria-labelledby` is), and
marks the tree changed when a checked state or a text input's value changes.
Tests in `accessibility_states.rs`.

**DioxusLabs/blitz: "Give AccessKit nodes their bounds"** (`0007`). Screen
readers place their focus highlight and flat review by node bounds, and
AT-SPI hit-tests with them. Element border boxes relative to the viewport in
CSS px, scaled by the window node's transform; scrolling and viewport
changes mark the tree changed.

**DioxusLabs/blitz: "Route screen-reader actions to DOM events"** (`0008`).
`ActionRequested` was a TODO. Adds `UiEvent::Accessibility` (with
`BlitzAccessibilityAction`), declares each node's actions, and does them
through the event driver so Dioxus handlers and default actions run. Also: a
click with no default action no longer clears the focus when it lands in
the focused element. Tests in `accessibility_actions.rs`.

**DioxusLabs/blitz: "Name form controls by their labels"** (`0009`).
HTML-AAM's name from `<label>`, for labelable elements without ARIA naming.

**DioxusLabs/blitz: "Mount a Dioxus app in a div, not a main element"**
(`0010`). The code made `<main id="main">` where its comment says
`<div id="main">`.

# BLITZ-4: what screen readers get from Blitz (2026-11-10)

Issue #83. Caskade's native UI reaches screen readers through Blitz's
AccessKit bridge: AccessKit builds the platform's tree (AT-SPI on Linux, UI
Automation on Windows, NSAccessibility on macOS) from the nodes Blitz gives
it. At the BLITZ-0 pin, Blitz gave roles and a few attributes (label,
description, disabled, expanded, hidden, level, selected), no bounds, and
dropped every action a screen reader asked for; on Linux it built no AT-SPI
adapter at all, so Orca saw no window. Patches 0006–0013 of the series in
[`vendor/blitz/`](../../vendor/blitz/README.md) close the gaps Caskade's
components need.

## What changes

| Patch | Effect |
| --- | --- |
| `blitz/0006` Expose ARIA states and properties to AccessKit | checked and pressed (`aria-checked`, `aria-pressed`, native checkboxes and radios) as toggled; `aria-current`, `-modal`, `-busy`, `-invalid`, `-required`, `-readonly`, `-multiselectable`, `-haspopup`, `-orientation`, `-posinset`, `-setsize`; live regions (`aria-live`, `aria-atomic`, the implicit values of `alert`, `status`, `log`); values (`aria-value*`, `<progress>`, `<meter>`, range and number inputs, a text input's value, masked for passwords, and its placeholder); `aria-activedescendant` and `aria-controls`; native `disabled`, `required`; a link's URL; `role` as a token list and 13 more roles. A checked state or a text value changing marks the tree changed. |
| `blitz/0007` Give AccessKit nodes their bounds | each element's border box (CSS px, viewport-relative; the window node scales to physical px); scrolling and resizing mark the tree changed |
| `blitz/0008` Route screen-reader actions to DOM events | nodes declare focus, click, scroll into view, expand/collapse and increment/decrement; the shell turns AccessKit's requests into `UiEvent::Accessibility`, and the event driver does them as the user's input would (focus and blur events; focus then a click event with its default action; arrow keys for range controls). A click with no default action keeps the focus when it lands in the focused element, rather than clearing it. |
| `blitz/0009` Name form controls by their labels | a control without `aria-label`/`-labelledby` takes its `<label>`'s text (around it or with `for`), as HTML-AAM says |
| `blitz/0010` Mount a Dioxus app in a div, not a main element | `DioxusDocument` wrapped every app in `<main>`, so screen readers announced a second main landmark around the app's own |
| `blitz/0011` Build the AT-SPI backend with blitz-shell's accessibility feature | the workspace takes `accesskit_xplat` without default features and nothing asked for `accesskit_unix`: Linux and the BSDs had no adapter. Now built with async-io, AccessKit's default (its D-Bus connection runs on a thread of AccessKit's own; idle CPU unchanged, 0.27% of a core on Xvfb) |
| `blitz/0012` Expose text as AccessKit labels | text was a `TextRun`, which AccessKit's name computation and AT-SPI ignore: every name from content (buttons, links, headings) was empty and Orca couldn't read text. Text is now a `Label` with its text as value, and names its parent only for roles named from content (ARIA's list), so a live region isn't announced twice |
| `blitz/0013` Name the AccessKit window after the window's title | the document's `<title>`, else the title the window was created with (winit can't read it back on X11 or Wayland) |

`blitz/0008` also takes AT-SPI's way of stepping a slider: AccessKit turns
a value set through AT-SPI's Value interface into `SetValue`, which Blitz
does as one step towards it.

Upstream Blitz already refreshes the tree after DOM and focus changes (#1456,
in the pin). Animation frames don't mark the tree changed, so a running
spinner doesn't rebuild it.

## Verified here

- `tests/blitz-tests` in the Blitz checkout (`accessibility_states.rs`,
  `accessibility_actions.rs`, `accessibility_label.rs`, in the patches): the
  whole suite, 358 tests, passes with the series.
- [`apps/native/tests/accessibility.rs`](../../apps/native/tests/accessibility.rs)
  in Caskade's CI: the AccessKit tree of demo elements built as Caskade's
  components build theirs (navigation with the current page, toggle and
  menu buttons, a labelled checkbox, a slider, a required invalid search
  field, tabs, a modal dialog, a busy status with a progress bar, an alert)
  as a text snapshot; and a screen reader's click, expand, increment and
  focus reaching the Dioxus handlers, with the tree following.

- **Linux, Orca 46.1 on AT-SPI 2.52**, under Xvfb with a private session bus,
  AT-SPI bus and runtime dir: [`apps/native/scripts/a11y-atspi.py`](../../apps/native/scripts/a11y-atspi.py)
  opens [`examples/accessibility_demo.rs`](../../apps/native/examples/accessibility_demo.rs),
  checks the AT-SPI tree (roles, names, checked, pressed, modal, required,
  values, bounds), focuses, activates and steps controls through AT-SPI,
  and reads what Orca said (speech-dispatcher on libao's null driver). All
  23 checks pass; Orca said: "Caskade accessibility demo frame." /
  "Favourite toggle button not pressed." / "pressed" / "Volume slider 50." /
  "Uninstall Hollow Knight? dialog" / "Uninstall push button." /
  "Celeste is installed" (the status's live update, once).
- **Windows 10 (the test VM), UI Automation**, read from PowerShell's
  UIAutomationClient in the desktop session: the window named after its
  title; hyperlinks with their URL; buttons, a toggle button (Off, then On
  after `Toggle`), the expand button (Collapsed, then Expanded with its
  options after `Expand`), a checkbox (On), a slider (`RangeValue` 40 in
  0–100, 50 after `SetValue(100)`), a required edit, tab items with their
  selection, a dialog, a status bar with a progress bar; `SetFocus` focuses;
  `Invoke` runs the handlers, and the live text updates.

## Gaps left

- AccessKit's AT-SPI mapping has no state for `invalid`
  (`accesskit_atspi_common` 0.21), so Orca doesn't hear "invalid entry";
  UI Automation and macOS get it. Upstream AccessKit.
- The document's `<html>` and `<body>` show as `unknown` on AT-SPI (`Custom`
  on UIA); screen readers treat them as layout.
- What each OS's screen reader says for each component of the library
  (BLITZ-14, #93) is checked there: its acceptance has "every interactive
  component announces its role, name, state and value". Listening passes
  with Narrator, Orca on a real session and VoiceOver are user checks in
  #120.

## Upstream pull requests

Submitted on 10 Nov 2026 where upstream didn't already have the bug or a fix;
each item's upstream issue or PR and its state is in [#127](https://github.com/krazyjakee/caskade/issues/127). The
drafts below are kept as written. Each is one
patch file in `vendor/blitz/patches/blitz/`, with its tests (Blitz's suite,
358 tests, passes with the series).

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

**DioxusLabs/blitz: "Build the AT-SPI backend with blitz-shell's
accessibility feature"** (`0011`). The workspace takes `accesskit_xplat`
with `default-features = false`, and nothing enabled `accesskit_unix`, so
on Linux and the BSDs the accessibility feature built no adapter.

**DioxusLabs/blitz: "Expose text as AccessKit labels, so names from content
reach screen readers"** (`0012`). AccessKit takes a name from a
`labelled_by` target's value only for `Label`s; `TextRun` is for text-field
runs with character data. Text names its parent only for ARIA's
name-from-content roles. Test in `accessibility_label.rs`.

**DioxusLabs/blitz: "Name the AccessKit window after the window's title"**
(`0013`).

**AccessKit: map `invalid` to AT-SPI's `invalid-entry` state** (an issue,
with the one-line mapping).

# BLITZ-5: input methods, clipboard and selection in Blitz text fields (2026-11-10)

Issue #84. Caskade's text fields in the native UI are Blitz's own `<input>`
and `<textarea>`: Blitz edits them with parley's `PlainEditor` and talks to
the platform's input method and clipboard through winit. At the BLITZ-0 pin
the pieces were there (IME on focus, preedit and commit into the editor, the
clipboard on all four winit backends, mouse and keyboard selection), but an
input method's candidate window was never placed, script never heard a
composition, and fields had no read-only, password or touch handling.
Patches 0014–0020 of the series in [`vendor/blitz/`](../../vendor/blitz/README.md)
close the gaps.

## What changes

| Patch | Effect |
| --- | --- |
| `blitz/0014` Fire composition events and say when a key comes mid-composition | `compositionstart`, `compositionupdate` (the preedit) and `compositionend` (the committed text, empty when cancelled) around the field's edits, in a browser's order, through to Dioxus's composition events (`convert_composition_data` was unimplemented); `KeyboardEvent.is_composing` while the field composes, and the field's own shortcuts leave a composition alone; `input` carries the value without the preedit; `DeleteSurrounding` works; the IME's raw events no longer reach script under the non-DOM name "composition" |
| `blitz/0015` Place the IME at the caret, tell it the field's purpose, commit on blur | Blitz asked winit for no IME capabilities, so every caret area was dropped (and the one it sent was offset twice by the input's position): candidate windows opened wherever the platform put them. It now asks for the cursor area and hint-and-purpose, sends the purpose from the input's `type` (a password is hidden and sensitive), and each resolve sends the caret's area when it moved (after the field's and the page's scrolling and the zoom). A field losing focus mid-composition commits it, with its events |
| `blitz/0016` Underline an IME's composition | the preedit was drawn like committed text |
| `blitz/0017` Text fields: read-only, passwords, copying, single-line paste, commands | `readonly` fields take no edits but select and copy (they were editable); passwords don't copy or cut; copying doesn't fire `input`; shortcuts match with Shift or Caps Lock; single-line fields drop pasted line breaks; `BaseDocument::text_field_command` (select all, copy, cut, paste) for a context menu or a pad |
| `blitz/0018` Keep selecting past a text input's edge, select a word on long press | a mouse drag stopped selecting once it left the input; touch had no way to select (a finger pans): a half-second press selects the word |
| `blitz/0019` Paint selections with the `::selection` background | the highlight was a fixed light blue whatever the theme said |
| `blitz/0020` Setting a text input's value ends a composition and puts the caret last | a controlled field's same value changes nothing; a new one replaced the buffer under a composition and kept the caret's byte offset |

In Caskade: `Input::field_command` (`apps/native/src/input/field.rs`) runs
those commands for #93's text field menu, firing the `input` a cut makes
through the input layer, and the input layer's key map leaves keys alone
while an IME composes. The on-screen keyboard (#92) keeps writing the value
and pasting as before.

## Verified here

- Blitz's suite with the series, 369 tests: `text_input_ime.rs` drives a
  composition through the event driver (its events, the commit when focus
  moves, the IME caret area following typing), `text_input_clipboard.rs`,
  `text_input_selection.rs` (both fail without their fixes),
  `selection_pseudo.rs`; unit tests for the composition events, surrounding
  text and setting the value mid-composition.
- [`apps/native/tests/text_field.rs`](../../apps/native/tests/text_field.rs)
  (Caskade's CI): a controlled search field hears `compositionstart`,
  `compositionupdate`, `keydown` with `is_composing`, `input` and
  `compositionend` in order, and a pasted query without its line break; the
  value goes round through the signal intact. `input::field` tests a cut
  from the menu heard as typing; the input layer's and library's tests pass.
- **IBus 1.5.29 over XIM**, on a private display and session bus with a
  private HOME ([`apps/native/scripts/ime-ibus.py`](../../apps/native/scripts/ime-ibus.py),
  [`examples/text_field_demo.rs`](../../apps/native/examples/text_field_demo.rs)):
  IBus's Unicode entry (Ctrl+Shift+U 4f60 Space, a real preedit and commit)
  arrives as `compositionstart`, `compositionupdate u` … `u4f60`,
  `input 你`, `compositionend 你`; plain typing follows; Ctrl+C puts the
  field's text on that display's clipboard and Ctrl+V pastes from it,
  dropping the line break.

- **Microsoft IME (Japanese) on Windows 10**, the test VM's desktop
  session ([`apps/native/scripts/ime-windows.ps1`](../../apps/native/scripts/ime-windows.ps1),
  the same demo built for Windows): on the Japanese layout with the IME
  off, letters type as usual; with it on, "nihon" composes (the preedit
  にほｎ underlined in the field, the candidate window opened under the
  caret), Space converts and Enter commits: `compositionstart`,
  `compositionupdate ｎ` … `にほｎ`, `日本`, `input abxy日本`,
  `compositionend 日本`.

The scripts start a session bus of their own after switching to a private
HOME and runtime dir: services it activates (a portal, a keyring) get the
private dirs. Under `dbus-run-session` alone they got the user's real ones
(reported for #122).

## Gaps left

- Real-session input methods are user checks in #120: fcitx5 and IBus with
  CJK engines on an X11 and a Wayland desktop (winit's Wayland IME is
  untested here), Microsoft IME on Windows 11 hardware, macOS input sources.
- No `beforeinput`, `paste`/`copy`/`cut` events (not needed by #93), no undo
  (parley's editor has none), no middle-click paste of the primary selection,
  `::selection` colours only the highlight (not the selected text).
- macOS: the agent's reading of winit-appkit suggests arrows and Enter may
  act twice (Cocoa's command and the key); checked on a Mac in #120.

## Upstream pull requests

Submitted on 10 Nov 2026 where upstream didn't already have the bug or a fix;
each item's upstream issue or PR and its state is in [#127](https://github.com/krazyjakee/caskade/issues/127). The
drafts below are kept as written. Each is one
patch file in `vendor/blitz/patches/blitz/`, with its tests.

**DioxusLabs/blitz: "Fire composition events and say when a key comes
mid-composition"** (`0014`). Text fields applied IME events but script never
heard them: the internal event went out under the name "composition" and
Dioxus Native's `convert_composition_data` was `unimplemented!()`. Adds
`DomEventData::Composition{Start,Update,End}` and Dioxus Native's
conversion; sets `is_composing` on key events while the focused field
composes; implements `DeleteSurrounding`.

**DioxusLabs/blitz: "Place the IME at the caret, tell it the field's
purpose, commit on blur"** (`0015`). `ShellProvider::enable_ime`;
blitz-shell requests winit's cursor-area and hint-and-purpose capabilities
(without them winit drops every area); the caret area follows from parley's
`ime_cursor_area` each resolve; fixes the double-counted content-box offset;
commits a composition when focus moves away.

**DioxusLabs/blitz: "Underline an IME's composition in text inputs"**
(`0016`).

**DioxusLabs/blitz: "Text fields: read-only, passwords, copying, single-line
paste, commands"** (`0017`).

**DioxusLabs/blitz: "Keep selecting past a text input's edge, and select a
word on long press"** (`0018`).

**DioxusLabs/blitz: "Paint selections with the `::selection` background"**
(`0019`).

**DioxusLabs/blitz: "Setting a text input's value ends a composition and
puts the caret last"** (`0020`).

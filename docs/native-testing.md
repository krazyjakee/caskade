# Testing the native UI

How the native app (`apps/native`, Dioxus RSX on Blitz) is tested, what CI
runs, and how to run the same locally. Layout advice is in
[native-layout.md](native-layout.md); the architecture is in the
[tech spec](TECH-SPEC.md).

## The layers

| Layer | Where | What it covers |
| --- | --- | --- |
| Headless documents | `#[cfg(test)]` modules under `apps/native/src` (components in `ui/tests.rs`, the frame and its navigation in `frame/tests.rs` and `router/tests.rs`, the input layer, the library's virtual grid, theme, anatomy), `apps/native/tests/{layout,text_display,text_field,accessibility}.rs` | A `DioxusDocument` with no window: render, `poll` and `resolve`, then query the DOM, send `UiEvent`s (keys, pointer, wheel, IME) and read Blitz's AccessKit tree. Like Vitest for the webview. The library's tests open the real core and app layer on a 10,000-game fixture library, in memory. |
| Crates below the UI | `crates/caskade-ui-data`, `-ui-input`, `-ui-i18n`, `-ui-theme`, `caskade-app`, `caskade-player` | The logic views use (data and paging, pad and keyboard rules, translations, appearance), tested as plain Rust. |
| Screenshots | `apps/native/tests/shots.rs` (`--features shots`) | Every reference screen in every valid theme, rendered with vello_cpu, against `tests/shots/*.png` (1% of pixels may differ); `shell` and `home` are the real frame over the fixture library, on the library's grid and on Home. `scripts/webkit-shots.py` and `compare-shots.py` set them beside WebKitGTK; the gaps are in [theming/blitz.md](theming/blitz.md). |
| Against Chromium | `apps/native/tests/text_pixels.rs` (`--features shots`) | One page rendered by Blitz and, committed, by headless Chromium (`scripts/text-oracle.sh`), measured the same way: cut titles and placeholders (#80). Taffy's layout numbers from Chromium are in `tests/layout.rs`. |
| The window | `apps/native/tests/smoke.rs`, `console_e2e.rs` (`#[ignore]`d) | The real binary on an Xvfb display and session bus of its own (`caskade-test-desktop`): the window restored, a web game and a video, a clean quit, crash restarts, a second start, and a scripted pad through the e2e harness (into the sidebar, a page and Back). Controller fixtures disable online metadata matching. |
| Measurements | `apps/native/scripts/library-scroll.sh` | Frame times scrolling 10,000 games, on a private desktop. |
| Real machines | #120 | Real displays, GPUs, screen readers, input methods and other OSes: manual rows, not CI. |

Nothing here reaches the internet or the developer's desktop: documents are
headless, screenshots are CPU-rendered in memory, and window tests start
their own display and bus.

## In CI

`.github/workflows/ci.yml`, on master and on every pull request whose
changes reach `caskade-native` (`scripts/ci-scope.py`'s `native`: its own
files, any crate it builds on, the vendored Blitz, themes, translations or
the token contracts; the `run-e2e` label too):

- **rust** (Linux): `cargo test -p caskade-native` with the other app
  suites (the headless layers), Clippy, formatting, and the stylesheet
  check (`build-css.mjs --check`).
- **native** (Linux): the window tests under Xvfb with Mesa's lavapipe,
  then the screenshots and the Chromium comparison.
- **native-macos**: `cargo check -p caskade-native --all-targets`. Running
  it on a Mac is a #120 check.
- **windows-cross**: Clippy for `x86_64-pc-windows-gnu`, caskade-native
  included. A debug windows-gnu build doesn't link (iroh-relay's DLL
  exports), so Windows runs are release builds on a real machine (#120).

## Locally

| Task | Command |
| --- | --- |
| Headless tests and screenshots | `just test-native` |
| The window tests | `just test-native-window` |
| Rewrite the screenshots after a deliberate change | `just native-shots` |
| Chromium's reference for `text_pixels` | `apps/native/scripts/text-oracle.sh` |
| Run the app | `just native` (your display) or `scripts/private-desktop.sh -- <binary>` |

A new test that starts the app uses `caskade_test_desktop::PrivateDesktop`;
one that only renders uses a headless document.

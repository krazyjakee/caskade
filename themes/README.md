# Themes

- `default/` (Nexus, `caskade.default`), `cobalt/`, `emerald/` and `engage/` (`caskade.cobalt`, `caskade.emerald`, `caskade.engage`) are built in: `crates/caskade-core/src/themes.rs` embeds them, so they ship with every copy and the catalogue can't replace them.
- `ember/` (`dev.caskade.ember`) and `paper/` (`dev.caskade.paper`) are not built in. They are the starter catalogue's theme packages (`scripts/addons/starter.json`, `docs/plugins/publishing.md`), installed from the catalogue like any other. Paper was built in until #125 (as `caskade.paper`); someone who had it chosen moves to Nexus without a notice (`caskade_core::themes::RETIRED`). Cobalt, Emerald and Engage were Cobalt Five, Emerald One and Toggle Two (`caskade.cobalt-five`, `caskade.emerald-one`, `caskade.toggle-two`) before any release had them; a choice of an old id moves to the new one the same way.

The theme format is in `docs/theming/README.md`. The built-in themes' `backdrop.json` files are the backdrops the native app draws on the GPU (#149); their `theme.css` backdrops are the same look at the start of its motion, which the webview and a window without a GPU draw. Lint a folder with `cargo run -p caskade-theme --features cli -- lint themes/<folder>`.

## The console-inspired themes (#125)

Cobalt, Emerald and Engage are in the spirit of today's console home screens, never copies of them:

- Their names are parodies that can't be mistaken for a product, and no console's or maker's mark appears in a name, id, description, preview or CSS. `no_built_in_theme_names_a_console_or_its_maker` in `crates/caskade-core/src/themes_tests.rs` scans every built-in theme's text for them.
- No logos, symbols, controller-face glyphs, console fonts, sounds, icons or wallpapers. The fonts are openly licensed and bundled (Manrope, Barlow Semi Condensed, Nunito: SIL OFL, each folder's `assets/fonts/OFL.txt`).
- Their palettes are their own values near each look's spirit, not the official colours, and they use Caskade's own layout, tokens and anatomy.
- Their previews are the native app's `preview` reference screen (`apps/native/src/shots/screens.rs`), whose fixture games have genres for subtitles rather than platform names.

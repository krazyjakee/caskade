# Themes

- `default/` (Nexus, `caskade.default`) is built in: `crates/caskade-core/src/themes.rs` embeds it, so it ships with every copy and the catalogue can't replace it.
- `ember/` (`dev.caskade.ember`) and `paper/` (`dev.caskade.paper`) are not built in. They are the starter catalogue's theme packages (`scripts/addons/starter.json`, `docs/plugins/publishing.md`), installed from the catalogue like any other. Paper was built in until #125 (as `caskade.paper`); someone who had it chosen moves to Nexus without a notice (`caskade_core::themes::RETIRED`).

The theme format is in `docs/theming/README.md`. Lint a folder with `cargo run -p caskade-theme --features cli -- lint themes/<folder>`.

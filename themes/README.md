# Themes

- `default/` (Nexus, `caskade.default`) and `paper/` (`caskade.paper`) are built in: `crates/caskade-core/src/themes.rs` embeds them, so they ship with every copy and the catalogue can't replace them.
- `ember/` (`dev.caskade.ember`) is not built in. It is the starter catalogue's theme package (`scripts/addons/starter.json`, `docs/plugins/publishing.md`), installed from the catalogue like any other.

The theme format is in `docs/theming/README.md`. Lint a folder with `cargo run -p caskade-theme --features cli -- lint themes/<folder>`.

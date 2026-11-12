# Caskade Pack (example plugin)

An example plugin for the installer hooks (#55), translated (#219). It
teaches Caskade two things:

- **A format.** `.cpk` game packs: `CPAK`, then one entry after another. An
  entry is the name length (u16 LE), the UTF-8 relative path, flags (u8; 1
  means executable) and the size (u64 LE), then the bytes. The plugin reads
  the headers through `host.install-input-read` and answers with
  `copy_range` and `set_executable` steps. The installer writes the files.
- **A URL host.** A game page on `games.example.com` links its pack with
  `rel="caskade-download"`. The plugin fetches the page and returns the
  pack's URL; the installer downloads it.

Its only permission is `network`. It can't read or write any file.

It also shows how a plugin is translated (#219), into Swedish:

- `locales/sv.po` translates its name, description, its **Prefer packs**
  setting (whether its plan ranks above the built-in installers) and its
  **About game packs** game action, by their English text. The host reads
  it; `locales/dev.caskade.example.pack.pot` is the template.
- The backend answers the action (`ui.act` with `run`, plugin API 0.2,
  #109) with a notice, and its error messages come from the same catalog
  through `host.message`.

After changing a text, run
`node ../../../packages/create-plugin/templates/extract-messages.mjs`
here to update the template and the catalog.

Build it with `node plugins/examples/build.mjs caskade-pack`, then install
the folder from Settings → Plugins.

`crates/caskade-plugin-host/tests/installer.rs` installs a pack from a local
page with it, end to end.

# Publishing to the add-on catalogue

How a plugin or theme gets into Caskade's catalogue, and how it is updated, deprecated and reported. The format and what the app does with it are in [catalogue.md](catalogue.md). Plugins follow the [plugin contract](README.md), themes the [theming contract](../theming/README.md).

The catalogue lists Caskade plugins and themes only. It doesn't take Playnite extensions or other launchers' packages, and it doesn't take custom metadata sources: a plugin that provides the reserved `metadata-provider` interface, or lists a `metadata` category, is refused.

## The tools

`caskade-catalogue` (`apps/desktop/src-tauri/src/bin/catalogue.rs`) builds packages and checks catalogues with the code the app installs with:

```sh
cargo run -p caskade-desktop --bin caskade-catalogue -- build <source.json> --out <dir> [--previous <published catalogue.json>]
cargo run -p caskade-desktop --bin caskade-catalogue -- check <dir>/catalogue.json | <address> [--key <public key or .pub file> | --release-key]
```

`just starter-catalogue [<dir>]` does both for the starter collection. It builds the example plugins first (`plugins/examples/build.mjs`).

### `build`

Reads a source file and writes `<dir>/catalogue.json`, `<dir>/packages/<id>-<version>.zip` and `<dir>/media/<id>/<n>.<ext>`.

What a package holds is decided by its kind. A plugin package holds `plugin.json`, `dist/`, `locales/`, `assets/`, a README, a LICENSE and a CHANGELOG. A theme package holds `theme.json`, `tokens.json`, `theme.css`, `assets/`, `previews/`, `locales/`, a README and a LICENSE. Sources, tests, `node_modules` and hidden files stay out.

The package is refused if:

- `plugin.json` names a `ui` or `backend` file that hasn't been built
- it holds a link
- it holds something that looks like a credential: `.env`, `.npmrc`, key and certificate files, private keys, minisign or Tauri signing keys, or common token prefixes

Archives are reproducible: entries are in name order under one top folder named for the id, with fixed times and modes. The same files give the same SHA-256. `generated` comes from the source, else `SOURCE_DATE_EPOCH`, else the time of the build.

The manifest supplies the id, name, version, author, plugin API range and permissions; the source supplies the rest. The catalogue is then read with the app's own validation, and any record the app would leave out fails the build: a package listed twice, a version that isn't semantic, an unknown OS or mode, a manifest without an author, or a license that isn't an SPDX expression.

With `--previous`, each package keeps the releases already published, newest first, so copies installed from them still match. A version already published with other contents is refused; raise the version.

### `check`

Reads a staged folder or a published address the way the app does, then checks everything an install would. It fails on any problem:

- **Signature:** checked against `--key`, or Caskade's release key with `--release-key`. Without either it only warns.
- **Records:** every record must be valid ([the rules](catalogue.md#limits)). The app leaves an invalid record out; here it is a problem. Ids must be unique, versions semantic and unique within a package, API ranges readable, OSes and modes known, and every package needs an author and an SPDX license.
- **Artifacts:** every release's archive is downloaded. It must be reachable (a missing file or an HTTP error such as 404 is a problem) and match its size and SHA-256.
  - It must unpack safely and hold the manifest of its id and version.
  - A plugin must pass the plugin host's checks and start in a scratch host. Its API range and permissions must be the manifest's.
  - A theme must pass the theme service's checks and not take a built-in theme's id.
- **Listing:** every screenshot is downloaded and must be a reachable PNG, JPEG or WebP image. Screenshots are optional: a package without them gets a warning.

### Licenses and attribution

Plugin and theme manifests have no license or attribution fields, so the catalogue's `author` and `license` are a package's attribution. The license is an SPDX expression that covers **everything the package ships**, its code and any bundled fonts, images or sounds:

- When everything is under one license, name it: `MIT`.
- When bundled assets come under other terms, combine them: `MIT AND OFL-1.1`, `MIT AND CC-BY-4.0`. Credit their authors and sources in the package's README or a LICENSE file, which a package may ship, as their licenses ask.
- Terms SPDX doesn't list are `LicenseRef-<name>`, with the text in the package's LICENSE file.

`build` and `check` refuse a package without an author or with a license that isn't an SPDX expression. Whether the license is true and allows redistribution is for the [review](#review).

## The source file

```json
{
  "format": 1,
  "packages": [
    {
      "path": "../../plugins/examples/howlongtobeat",
      "kind": "plugin",
      "summary": "How long a game takes to beat, on its page.",
      "description": "Optional; the manifest's description otherwise. Plain text.",
      "license": "MIT",
      "homepage": "https://…",
      "categories": ["game pages"],
      "screenshots": [{ "file": "media/hltb.png", "alt": "The tab on a game page" }],
      "os": [],
      "modes": [],
      "notes": "What changed in this release.",
      "published": "2026-10-30",
      "deprecated": null
    }
  ]
}
```

- **Paths** are relative to the source file.
- **`os`:** empty means every OS. List only the ones it works on: GameMode is `["linux"]`. An install on another OS is refused with the reason shown.
- **`modes`:** empty means both. If its pages don't work in console mode, say `["desktop"]`, and the app tells console users.
- **`description`:** say what the add-on needs and where it falls short. Examples are a service it depends on, an unofficial API that can break, offline behaviour, or a program that must be installed. Users read it before they install, together with the permissions.

The starter collection's source is `scripts/addons/starter.json`.

## API compatibility

A plugin release declares the plugin API range it was built for (`api` in `plugin.json`, e.g. `^0.1`), and the catalogue repeats it.

- An app whose API is outside the range shows the release as not compatible and won't install it. It offers the newest release that fits instead.
- Publishing a release for a newer API therefore never breaks users on an older Caskade. Keep the older release in the catalogue (`--previous` does) for as long as such users exist.
- Raise the range only when the plugin uses something new.
- Themes have no API range; the token contract and the anatomy catalogue are checked when the theme loads.

## Permissions

The catalogue shows what a plugin asks for, and `check` holds it to the manifest. The manifest is what the plugin host enforces ([Permissions](README.md#permissions)).

- **Library access:** `library.read`, `library.write`
- **Network:** `network:<host>`, each host it may reach
- **Folders:** `fs:<folder>`, each folder it may see, `:ro` for read-only
- **Processes:** `process`, may change what a launch runs

Ask for the least that works:

- **A first install** shows the user the review dialog, and only then grants the permissions.
- **An update** keeps the grant. If it asks for more, it is installed but stays off until the user allows it again. Say why in the release notes.
- **Explain each permission** in the description: why it needs the host, the folder or `process`.

Themes have no permissions: they are tokens, CSS and assets, never code.

## Publish, update, deprecate

Every step after building is the maintainer's. Nothing in the repository uploads or signs on its own.

**Publish a package:**

1. Add it to the source file, build the plugin, and build the catalogue with `--previous` set to the published `catalogue.json`.
2. Check the folder with `check <dir>/catalogue.json`.
3. Try it in a clean profile from a local server:
   - Serve the folder: `python3 -m http.server -d <dir> 8765`.
   - Start the app with `CASKADE_CATALOGUE_URL=http://127.0.0.1:8765/catalogue.json` and `CASKADE_CATALOGUE_PUBKEY` set to a test key's public half. Sign `catalogue.json` with that key first, using `pnpm tauri signer sign -f <test key> catalogue.json`.
   - Install it from Settings → Plugins.
   - Or run the ignored test `starter_collection_installs_in_a_clean_profile` with `CASKADE_STARTER_DIR=<dir>`.
4. Sign `catalogue.json` with the release key: `pnpm tauri signer sign -f ~/.tauri/caskade-updater.key catalogue.json`.
5. Upload `catalogue.json`, `catalogue.json.sig`, `packages/` and `media/` as assets of the `catalogue` release on `krazyjakee/caskade` (the [origin](catalogue.md#origin-and-trust)).
6. Check the published copy: `check https://github.com/krazyjakee/caskade/releases/download/catalogue/catalogue.json --release-key`.

**Update a package:**

1. Raise its version in the manifest, update `notes` and `published`, and rebuild with `--previous`.
2. Users who have it installed are offered the update when Settings opens, if their copy of the catalogue is a day old and they haven't turned the check off, or when they press Check for updates. Updating keeps their settings and data.

**Deprecate a package:** set `deprecated` to the reason, e.g. "Replaced by X" or "The service closed". It stays listed for those who have it, with the reason shown, and nobody is pushed to remove it.

**Remove a release:** removing one from the catalogue stops new installs of it. Users who have it installed keep it.

**Remove a package:** to remove a package entirely, deprecate it first, then drop it from the source.

## Reporting a broken package

Report it as an issue on [krazyjakee/caskade](https://github.com/krazyjakee/caskade/issues) with:

- the package id and version, from the details dialog
- your OS and app mode
- what the dialog said, such as "It didn't install: …" or "the previous version is back"

If the problem can damage something or leak data, mark the issue as a security report.

A maintainer can pull a broken release by publishing the catalogue without it, or deprecate the package. Caskade already refuses a release that doesn't match its checksum. It also rolls back an update that doesn't start and remembers the refused release, so a broken update never replaces a working version.

## Review

Before a package goes into the curated catalogue, a maintainer checks:

- **`check` passes** for the staged catalogue.
- **Sources:** the package's sources are public, and its license allows redistribution. Bundled third-party assets are covered by the license expression and credited in the README or LICENSE.
- **Listing:** the description and screenshots match what it does.
- **Permissions:** they are explained, and no broader than needed.
- **Behaviour:** it installs, works and uninstalls in a clean profile on each OS it lists.
- **Not test plugins:** test plugins, such as `crashy`, aren't offered.

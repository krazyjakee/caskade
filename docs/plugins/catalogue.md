# The add-on catalogue

A curated list of plugins and themes that Caskade can browse and install (#181–#184). The client is `caskade_core::addons`; the adapter commands are `addons_*` in `commands/addons.rs`.

The catalogue offers plugins and themes only. A plugin whose release says it `provides` the reserved `metadata-provider` interface is left out, and so is any entry in a `metadata` category: custom metadata sources aren't part of it.

## Origin and trust

| | |
|---|---|
| Origin | `https://github.com/krazyjakee/caskade/releases/download/catalogue/catalogue.json`: the `catalogue.json` asset of a release tagged `catalogue` on Caskade's public repo. Reading it needs no account or token. |
| Signature | `catalogue.json.sig` beside it: a minisign signature made with **Caskade's release key**, the key that signs updates (`~/.tauri/caskade-updater.key`; public half in `tauri.conf.json` → `plugins.updater.pubkey`). The app refuses a catalogue that doesn't verify, and keeps the last one that did. |
| Artifacts | Each release names its archive by URL, **SHA-256** and **size**. An install (#182) refuses an archive that doesn't match, whoever served it. |
| Addresses | Archive, image and homepage addresses must be HTTPS. Relative addresses resolve against the catalogue's own address, so artifacts can sit beside it as release assets. Plain HTTP is accepted only on the catalogue's own scheme and host, which means a local server while developing. Addresses with credentials are refused. |
| Privacy | A refresh is two GETs: the catalogue and its signature. Nothing about the user, their library or their installed add-ons is sent. Compatibility is worked out on the device. |
| Overrides | A build can point at another origin with `CASKADE_CATALOGUE_URL` at build time. A run can point at one with `CASKADE_CATALOGUE_URL` and `CASKADE_CATALOGUE_PUBKEY` (a minisign public key, as is or base64) in its environment. That is for local catalogue servers and smoke tests (#184), not for users. |

Publishing the curated origin is a release step for the maintainer; see [Publishing](#publishing).

## Format (version 1)

```json
{
  "format": 1,
  "generated": "2026-10-30T00:00:00Z",
  "packages": [
    {
      "id": "dev.caskade.playtime-stats",
      "kind": "plugin",
      "name": "Playtime stats",
      "author": "Caskade",
      "summary": "Charts of your play time.",
      "description": "Plain text. Shown as text; markup is never interpreted.",
      "license": "MIT",
      "homepage": "https://…",
      "categories": ["stats"],
      "screenshots": [{ "url": "media/stats.png", "alt": "The stats page" }],
      "deprecated": null,
      "releases": [
        {
          "version": "0.1.0",
          "api": "^0.1",
          "os": ["linux", "windows", "macos"],
          "modes": ["desktop", "console"],
          "permissions": ["library.read"],
          "provides": [],
          "notes": "First release.",
          "published": "2026-10-01",
          "artifact": { "url": "dev.caskade.playtime-stats-0.1.0.zip", "sha256": "…", "size": 12345 }
        }
      ]
    }
  ]
}
```

### Fields

- **`kind`:** `plugin` or `theme`.
- **`id`:** the manifest id (`plugin.json` or `theme.json`). Lowercase letters, digits, `-` and `_`, in dot-separated parts. Unique in the catalogue, across plugins and themes.
- **`author`:** required.
- **`license`:** required, an [SPDX license expression](https://spdx.github.io/spdx-spec/v2.3/SPDX-license-expressions/) covering everything the package ships: `MIT`, `MIT OR Apache-2.0`, `GPL-2.0-or-later WITH Classpath-exception-2.0`, `LicenseRef-<name>` for terms SPDX doesn't list. Ids aren't checked against SPDX's list, but free text, `NONE` and `NOASSERTION` are refused.
- **`version`:** a semantic version, unique within the package. Versions that differ only in build metadata (`1.0.0+a`, `1.0.0+b`) are the same version.
- **`api`:** the plugin API range the release needs, as its `plugin.json` has it. Required for plugins and absent for themes.
- **`os`:** the OSes the release runs on: `linux`, `windows`, `macos`. Empty or missing means all of them.
- **`modes`:** the app modes the release's UI supports: `desktop`, `console`. Empty or missing means both. A missing mode is a limit the app shows, not a refusal.
- **`artifact`:** the archive's address, its lowercase hex SHA-256 (64 digits) and its size in bytes (1 byte to 512 MiB).
- **`permissions`:** what the manifest asks for, shown in the review. The manifest in the archive is what decides.
- **`deprecated`:** a reason. The entry stays listed for those who have it installed.

### Limits

A catalogue over the limits below is refused whole, and so is anything that isn't format 1 or isn't JSON:

- the catalogue: 4 MiB, at most 1,000 packages
- each package: 50 releases, 8 screenshots, 8 categories, 32 permissions
- names: 80 characters
- summaries: 200 characters
- descriptions and notes: 8,000 characters
- artifacts: 512 MiB

Within a valid catalogue, a record that breaks a rule is **left out with the reason**, and the others stay:

- a bad id
- a duplicate id (the first record stays)
- an unknown kind
- a missing name, author or summary
- a missing license, or one that isn't an SPDX license expression
- control characters in text
- a non-HTTPS address

A release that breaks a rule is left out of its entry. An entry left with no releases is left out. Releases are left out for:

- a version that isn't semantic, or one listed twice (the first stays)
- a plugin release without a readable API range, or a theme release with one
- an unknown OS or mode
- an artifact without an address, a 64-digit SHA-256 or a size within the limit

The catalogue itself can't show that its artifacts exist or are what they claim; `caskade-catalogue check` fetches every one before publishing ([publishing.md](publishing.md#check)), and every install checks the archive again.

## In the app

`CatalogueService` keeps the last catalogue that verified and validated in `<cache>/addons/`. It shows that catalogue offline and after a restart, and sets a damaged or other-origin cache aside without stopping startup.

`addons_catalogue(mode)` reads the catalogue, and `addons_catalogue_refresh(mode)` fetches it again. Each entry comes with its `compatibility`, worked out by `caskade_core::addons::evaluate` against what is installed and what this build accepts:

- **`state`:**
  - `available`
  - `installed`: at the newest release that fits, or newer
  - `updateAvailable`
  - `incompatible`: no release fits
- **`release`:** the newest release that fits, which is the one an install gets.
- **`reasons`:**
  - `api`: needs a plugin API this build lacks
  - `os`: doesn't run on this OS
  - `mode`: not blocking; its pages lack the current app mode
  - `deprecated`
  - `kind`: something of the other kind is installed under the id

Screenshots load through `addons_media(id, index)`. The artwork cache downloads only the catalogue's own image addresses, and only from public hosts unless the origin is local. It decodes them as PNG, JPEG or WebP and re-encodes them as WebP thumbnails. Nothing from the catalogue runs in the webview.

## Installing and updating (#182)

`addons_install(id, mode, accept, retry)` (`commands/addon_packages.rs`) installs or updates an add-on to the newest release that fits. It extends the plugin host and the theme service rather than adding another package manager.

### The steps

1. **Download** (`caskade_core::addons::package::download`): to `<data>/addons/downloads/<sha256>.zip`. The download stops at the catalogue's size and must match its SHA-256; an archive already there that checks out is reused. Cancel with `addons_install_cancel(id)`. Progress arrives as `AddonProgress` events (downloading, checking, installing, starting).
2. **Unpack** (`extract`): into `<data>/addons/staging/<job>`. Archives are refused for any of:
   - absolute paths, drive paths or `..` paths
   - links
   - more than 20,000 files or 512 MiB
   - unreadable zips, or sizes that don't match their contents

   One top folder is the package; otherwise the archive's root is.
3. **Check** (`check_identity`): the manifest (`plugin.json` or `theme.json`) must be the entry's id at the release's version.
   - A plugin must pass the host's `preview_folder`.
   - A theme must pass `ThemeService::check_folder`, which checks its tokens, CSS and that every asset it names exists.
   - A built-in or plugin-bundled theme's id can't be replaced.
4. **Swap** (`swap_in`):
   - It writes `<data>/addons/journal.json`, moves the installed version to `<data>/addons/previous/<id>`, then moves the staged folder into `<data>/plugins/<id>` or `<data>/themes/<id>`.
   - A plugin is rediscovered and, when enabled with a backend, started; a theme is reloaded.
   - If it doesn't start or apply, the swap rolls back and the previous version is back.
   - When the app stops between the swap and its confirmation, `package::recover` rolls it back at the next start, before plugins and themes load, and clears staging.

### Permissions

**A first plugin install** comes back as `needsReview` with the plugin's preview: its permissions, contributions and whether it has a UI half. The UI shows the existing review, and only an install with `accept` enables the plugin and grants what it asks.

**An update** keeps the user's grant. If the new version asks for more, the plugin host's own rule leaves it **off and waiting for consent** (`needsConsent`, Settings → Plugins); nothing new is granted silently.

### Updates keep data

An update keeps the plugin's key-value store, settings and data folder. A plugin whose pages were already loaded reports `restartRequired`.

### Failures

Each failure has a `kind`. Integrity, unsafe, identity, incompatible and activation failures won't pass by retrying, so they are recorded as the release's `rejected` state and refused again without a download unless the user retries (`retry`). Network and storage failures are transient.

### Uninstalling

`addons_uninstall(id, keepData)` removes a plugin, keeping its key-value store, settings and data folder only when asked, or a theme Caskade installed. Removing the active theme makes the default theme the choice. Built-in themes and dev folders aren't removed.

### Records and the update preference

`addons.records` (the library's settings table, backed up with it) keeps, per add-on: the version installed from the catalogue, a skipped update (`addons_skip_update`) and a rejected release. It also keeps the update preference (`addons_set_update_preference`): `notify` (check when Settings opens) or `manual`.

## Browsing and managing (#183)

The catalogue sits in Settings → Plugins, above the installed plugins (`components/addons/AddonCatalogue.tsx`). Settings → Appearance links to it for more themes. The webview reaches the commands through `AddonsApi` (`src/api/addons.ts`); `VITE_CASKADE_FIXTURES=1` browses a fixture catalogue (`addonsFixtures.ts`).

- **The list:** each add-on with its kind, author, summary and what it means here: available, installed, an update or not compatible. It is searchable by name, summary, author and category, and filtered by kind. The status line says when the catalogue was fetched, that it is offline or why the last check failed, and how many updates wait. Check for updates fetches again. When the update preference is `notify`, opening Settings fetches a catalogue older than a day.
- **Details** (`AddonDetails.tsx`):
  - screenshots through `addons_media`, with a placeholder when one can't load, and the description as text
  - why it doesn't fit, and what it lacks in the current mode
  - the release an install gets: version, date, size, OSes, modes and what it asks for
- **Installing:**
  - Install, Update or Try again (only for a refused release) with progress and Cancel.
  - A new plugin first opens the same permission review as a dev folder install.
  - The outcome follows: installed, off until allowed, restart for its pages, or why it failed and whether the previous version is back.
  - Offline, installing says it needs a connection.
- **Managing:** Skip this version (or offer it again), Use this theme for an installed theme, and Uninstall, which asks first and, for a plugin, whether to keep its settings and data.

In console mode the list and the details are pad-driven: A on a row opens its details, the details take the whole screen with the install button focused, and B closes them.

## Publishing

The maintainer's steps; nothing in the repo does these on its own. [publishing.md](publishing.md) has the details: the tools, the source file, API compatibility, permissions, updates, deprecation and reporting.

1. Build and check the catalogue and its packages: `just starter-catalogue <dir>`, or `caskade-catalogue build` and `check`.
2. Sign `catalogue.json` with the release key: `pnpm tauri signer sign -f ~/.tauri/caskade-updater.key catalogue.json`.
3. Upload every file in the folder, `catalogue.json.sig` included, as assets of the `catalogue` release on `krazyjakee/caskade`.
4. Check the published copy: `caskade-catalogue check <origin> --release-key`.

## The starter collection (#184)

`scripts/addons/starter.json` lists:

- **Playtime stats:** UI only, no permissions.
- **GameMode:** Linux only. It needs Feral GameMode installed and asks for `process` and three read-only folders.
- **Ember:** a theme. Default (Nexus) and Paper are built into the app, so they aren't catalogue packages; Ember extends the default with warm accents.

Each package has two screenshots (`scripts/addons/media/`). They were taken in the app itself, under Xvfb, with a clean profile: a fake Steam library in an isolated HOME and the packages installed from the starter catalogue on a local server.

Not offered:

- **HowLongToBeat:** howlongtobeat.com's firewall refuses the plugin's search requests (`403 Unavailable (IFW-E01)`, checked 2026-10-30), with or without a browser's headers. The plugin can't work until the site offers a way in, and getting around a site's bot protection isn't something a catalogue package should do.
- **`crashy`:** the test plugin.
- **`caskade-pack`:** the example, whose network host is a placeholder.

`commands/addon_smoke_tests.rs` builds a plugin and a theme with the same tooling, signs the catalogue and serves it from a local server. It then installs both into a clean profile, checks that they start or compile, updates them, and checks a broken release: the tooling flags it and the install rolls back. `starter_collection_installs_in_a_clean_profile` (ignored; set `CASKADE_STARTER_DIR`) installs every starter package the same way.

# Legal documents and notices

Settings → About → Licenses, Terms and conditions and Privacy policy show these documents in one Legal view (`/settings?tab=about&section=<licenses|packages|terms|privacy>`). The app bundles them, so they read offline.

| In the app            | Source                                                                  |
| --------------------- | ----------------------------------------------------------------------- |
| Licenses              | [`LICENSING.md`](LICENSING.md), then the 7-Zip licence texts            |
| Open-source packages  | `apps/desktop/src/views/settings/about/notices.generated.json`          |
| Terms and conditions  | [`TERMS.md`](TERMS.md)                                                  |
| Privacy policy        | [`PRIVACY.md`](PRIVACY.md)                                              |

The view renders the Markdown files as they are (`views/settings/about/Markdown.tsx`: headings, paragraphs, lists, quotes, links, bold and code). A link to one of these files opens its section in the view. Start a quote with `**Missing:**` or `**Draft.**` to mark content that isn't complete yet; never present placeholder wording as a finished policy.

## Updating the policies

Edit `TERMS.md` or `PRIVACY.md` and change its **Effective** date in the same commit. `PRIVACY.md` must describe what the code does: check it whenever a change adds or removes a network connection, a stored kind of data, or a setting that controls either. The connections it lists are in `caskade_core::igdb` (`BASE_URL`), `hasheous`, `artwork`, `romm`, `updates`, `emulation::setup` and the emulator definitions' `[install.*]` sources, `apps/desktop/src-tauri/src/commands/app_update.rs` (`ENDPOINT`), `caskade_installer::runtime::umu` and `fetch::resolvers`.

## Updating the notices

Run the generator after changing Rust or JavaScript dependencies (`Cargo.lock`, `pnpm-lock.yaml`), the 7-Zip pin (`scripts/7zz-pins.json`) or the fonts, and commit its output with the change:

```sh
python3 scripts/legal-notices.py
```

It needs `cargo`, `node`, `pnpm` and installed `node_modules`, and reads the crates' sources from Cargo's registry, so build or `cargo fetch` first. It lists:

- the Rust crates in the normal dependency graphs of both binaries the bundles ship, `caskade-desktop` and `caskade-native`, for every target (`cargo metadata`), which covers what any platform's build links. The vendored Blitz crates (`vendor/blitz/crates`) carry no licence files; their texts come from other crates under the same licence;
- the JavaScript packages in the UI's production bundle, fonts included: `apps/desktop/scripts/shipped-packages.mjs` builds it in memory and lists the package of every module and asset in it. `pnpm licenses list --prod` isn't used: it follows workspace packages' build tools and auto-installed peers, so it lists hundreds of packages that never ship;
- the upstream `License.txt` of each 7-Zip package the release builds bundle, through `scripts/7zz-download.py` (the first run downloads and verifies the pinned packages into `target/sidecars/`).

Each package keeps its licence expression, repository and the copyright lines of its licence files (its authors when it has none). Each licence's text is kept once, from the first package that ships it. The script prints the licences it found no text for (`missingTexts` in the output); a missing text that is the only choice for a package needs adding by hand before a release.

Components the generator can't see are described by hand in `LICENSING.md`: bundled data (the emulator definitions), platform-specific parts of each bundle (the AppImage's libraries, the Windows installer's WebView2 bootstrapper) and software Caskade downloads on request. Update it when `tauri.conf.json`'s `bundle` (or a platform's override), `.github/workflows/release.yml`'s targets, or `crates/*/THIRD-PARTY.md` change.

## Missing

- The legal entity behind Caskade and its metadata server, a contact address, and the law that governs the terms.
- The notices of the libraries the AppImage bundles (GTK, WebKitGTK, libayatana-appindicator and their dependencies).

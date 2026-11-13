# Caskade website

The public landing page at <https://krazyjakee.github.io/caskade/>. This is a
static website hosted by GitHub Pages, separate from the desktop webview.
It needs Node 22 and Python 3 for building and checking; no package installation,
network access, backend or external font service is required to build or check it.
The page has no first-party runtime JavaScript; its embedded YouTube player loads
third-party content when viewed.

From the repository root:

```sh
node apps/website/build.mjs
python3 apps/website/check.py
python3 -m http.server 1431 --directory apps/website/dist
```

Open <http://localhost:1431/> for English and <http://localhost:1431/sv/> for
Swedish. The generated `dist/` directory is ignored by Git.

## Editing

- `index.html` is the shared template. All player-facing text is in
  `locales/en.json` and `locales/sv.json`, including metadata and accessibility
  labels. The build rejects missing, blank and unused translations. Brands and
  language autonyms are proper names. The native language popover shows flags
  alongside language names, marks the current language, and supports Escape
  and clicking outside without runtime JavaScript.
- `site.css` uses the app's generated `--cs-*` tokens. The build copies those
  tokens and the Caskade logo (`apps/native/icons/caskade.svg`). The website's `data-cs="website"` root
  and parts are in the anatomy catalogue (`WEBSITE` in
  `apps/native/src/anatomy.rs`); after changing them, regenerate the contract
  `check.py` reads with `CASKADE_WRITE_ANATOMY=1 cargo test -p caskade-native anatomy`.
- `assets/` bundles the Latin variable Inter and Oxanium fonts and their OFL
  licences, the same families the app bundles (`apps/native/assets/fonts`). Keep the
  licences when replacing font files.
- `desktop-en.jpg`, `desktop-sv.jpg`, `console-en.jpg` and `console-sv.jpg` are
  captures of the app's Home view (taken from the retired webview app's
  browser preview before #143), using a sample library. The page crops the images with CSS and links to the complete captures.
  The sample artwork comes from Steam's public artwork CDN; game artwork belongs
  to its respective owners. No personal library, account or device data is used.
- Download buttons link directly to the published AppImage, Debian package,
  Windows setup and Apple silicon / Intel disk images. `downloads.mjs` selects
  exactly one asset per format from GitHub release metadata and rejects missing,
  ambiguous or unexpected URLs. Keep platform caveats aligned with
  `docs/PRODUCT-SPEC.md`.
- `releases.json` is a snapshot for offline builds. To preview newer metadata,
  run `gh api repos/krazyjakee/caskade/releases/latest > /tmp/caskade-release.json`,
  then `node apps/website/build.mjs /tmp/caskade-release.json`. Refresh the snapshot
  with the five installer assets when updating it; filenames are read from the
  metadata rather than constructed from the app's version. Run
  `node --test apps/website/downloads.test.mjs` to check asset selection.
- The video below the introduction uses a responsive, lazy-loaded YouTube
  privacy-enhanced embed (`youtube-nocookie.com`) with no autoplay and a direct
  YouTube link. Both languages share the video, with translated headings and
  accessibility labels. Video playback requires an internet connection.

The offline checker verifies both languages, headings, image alternatives,
local assets, section anchors, CSS asset URLs and the website anatomy. All local
URLs are relative, including Swedish's parent-directory asset links. The output
works both below `/caskade/` on project Pages and at the root of a local server.
If moving to a custom domain, update the canonical and alternate URL base in
`build.mjs` too.

## Refreshing screenshots

The captures came from the retired webview app's fixture preview, through a
Playwright script removed with it (#143). Until a native capture script
exists (#150), take new ones from the native app on a private desktop with fixtures:

```sh
cargo build -p caskade-native
scripts/private-desktop.sh --size 1600x900 -- env CASKADE_FIXTURES=1 \
  "$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')/debug/caskade-native"
```

Use a fabricated sample library only: no personal library, account or device
data. Review all four images, then rebuild the website and commit them.

## Deployment

`.github/workflows/pages.yml` builds and checks pull requests without deployment.
Changes to the site, shared logo, token CSS or the workflow on `master` publish
the checked artifact. Publishing a release also rebuilds and publishes the site
from `master`. Deployment reads the latest stable release metadata through the
GitHub API before the offline build, so download links follow new releases without
runtime JavaScript or a browser API request. A manual workflow dispatch also
publishes `master`.
Deployment uses the `github-pages` environment with `pages: write` and
`id-token: write`; the build only has read access. Actions are pinned to commit
SHAs. No `gh-pages` branch or generated files in `master` are needed.

The repository's **Settings → Pages → Build and deployment → Source** must be
**GitHub Actions**. A maintainer can also initialise this once with:

```sh
gh api --method POST repos/krazyjakee/caskade/pages -f build_type=workflow
```

Build and deployment results appear in the **GitHub Pages** Actions workflow.

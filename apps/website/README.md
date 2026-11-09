# Caskade website

The public landing page at <https://krazyjakee.github.io/caskade/>. This is a
static website hosted by GitHub Pages, separate from the desktop webview.
It needs Node 22 and Python 3 for building and checking; no package installation,
network access, runtime JavaScript, backend or external font service is required.

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
  tokens and the existing Caskade logo. The website's `data-cs="website"` root
  and parts are in `packages/ui/src/anatomy.ts`; regenerate the catalogue with
  `pnpm --filter @caskade/ui docs` after changing its anatomy.
- `assets/` bundles the Latin variable Inter and Oxanium fonts and their OFL
  licences, from the same Fontsource families the desktop app uses. Keep the
  licences when replacing font files.
- `desktop-en.jpg`, `desktop-sv.jpg`, `console-en.jpg` and `console-sv.jpg` are
  captures of the actual app's Home view in its browser preview, using a sample
  library. The page crops the images with CSS and links to the complete captures.
  The sample artwork comes from Steam's public artwork CDN; game artwork belongs
  to its respective owners. No personal library, account or device data is used.
- Download links go to GitHub's latest release page, where visitors choose a
  platform asset. There are no pinned version numbers or guessed filenames.
  Keep the platform caveats aligned with `docs/PRODUCT-SPEC.md`.

The offline checker verifies both languages, headings, image alternatives,
local assets, section anchors, CSS asset URLs and the website anatomy. All local
URLs are relative, including Swedish's parent-directory asset links. The output
works both below `/caskade/` on project Pages and at the root of a local server.
If moving to a custom domain, update the canonical and alternate URL base in
`build.mjs` too.

## Refreshing screenshots

Install the desktop workspace's dependencies and a Playwright-compatible
Chromium, then start a local fixture preview:

```sh
VITE_CASKADE_FIXTURES=1 pnpm --filter @caskade/desktop dev --host 127.0.0.1 --port 1430 --strictPort
```

In another terminal, run the manual capture under Xvfb on Linux:

```sh
xvfb-run -a node apps/website/capture.mjs http://127.0.0.1:1430
```

Set `CHROME_PATH` if Chromium is installed outside Playwright's default location.
The capture downloads public cover artwork, intercepts the fixture bootstrap in
an isolated browser context, and renders the actual application components in
both modes and languages. It does not edit the app's source or open a native
profile. Only visible images are awaited; off-screen shelves load lazily. This
is a manual asset-authoring step, separate from offline builds and CI checks.
Review all four images, then rebuild the website and commit the captures.

## Deployment

`.github/workflows/pages.yml` builds and checks pull requests without deployment.
Changes to the site, shared logo, token CSS or the workflow on `master` publish
the checked artifact. A manual workflow dispatch also publishes `master`.
Deployment uses the `github-pages` environment with `pages: write` and
`id-token: write`; the build only has read access. Actions are pinned to commit
SHAs. No `gh-pages` branch or generated files in `master` are needed.

The repository's **Settings → Pages → Build and deployment → Source** must be
**GitHub Actions**. A maintainer can also initialise this once with:

```sh
gh api --method POST repos/krazyjakee/caskade/pages -f build_type=workflow
```

Build and deployment results appear in the **GitHub Pages** Actions workflow.

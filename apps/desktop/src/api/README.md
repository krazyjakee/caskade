# Library data boundary (#107, #111)

`LibraryApi` is an injectable view projection, not a second persistent domain model. Native transport maps the generated `bindings.ts` commands/DTOs; do not hand-edit that generated file. Core owns filtering, sorting, FTS5, session totals and managed-path resolution. Views never issue filesystem or process commands.

`LibraryProvider` shares a TanStack Query client across routes and invalidates the `library` namespace on library and game lifecycle events. List keys include the full normalized filter/sort/search tuple; pages are bounded to 100 summaries. Topbar search requests six results from the same backend. Detail/stat/session queries refresh after launch and exit; no page reload is required. Asynchronous subscription cleanup handles unmount/React StrictMode races.

The browser fixture transport is selected **only** by `import.meta.env.DEV` plus `VITE_CASKADE_FIXTURES=1`. It is never a fallback after failed native IPC. Example isolated development preview (leave an existing app server alone):

```sh
VITE_CASKADE_FIXTURES=1 VITE_CASKADE_FIXTURE_COUNT=1203 pnpm --filter @caskade/desktop exec vite --port 1421
```

The same switch gives settings (`settingsFixtures.ts`), discovery, storage and the plugin manager in-memory fixtures, so every Settings tab renders in the browser; their exported APIs stay synchronous through `lazyApi`. Production builds eliminate the fixture import. Fixture records are original synthetic metadata; no contributor library is copied into frontend tests.

Grid/list use TanStack Virtual rows, memoized cards and exact 320/96 pixel cache thumbnails. Details use the 960 variant; missing variants retain a themed placeholder, never an original-image fallback. `convertFileSrc(path, 'caskade-asset')` handles the platform-specific custom scheme. Artwork cache confinement is enforced again in Rust.

## Validation

Run `pnpm typecheck`, `pnpm lint` and `pnpm --filter @caskade/desktop test` (heavy lane for build/test). Vitest checks paging, separate query keys, bounded backend search, event invalidation, late listener cleanup, explicit errors, virtualized DOM bounds, thumbnail use and fake launch/session/dashboard refresh.

For the browser scroll benchmark, start a separate Chromium instance with a **disposable** profile and `--remote-debugging-port=9223`; never connect the script to a user's normal profile. Run:

```sh
flock /tmp/caskade-heavy.lock python3 apps/desktop/tests/library-scroll.py --url http://localhost:1421 --output target/app-core/library-scroll.json
```

It opens/closes its own pages, traverses each layout over 240 animation frames, counts missed refresh intervals against the measured idle cadence, and rejects zero/unbounded mounted cards or missed frames. Python `websockets` is required. Artifacts record the actual browser and metrics; jsdom tests establish DOM bounds, not rendering performance. Chromium fixture evidence does not claim native WebKitGTK/WebView2/WKWebView parity.

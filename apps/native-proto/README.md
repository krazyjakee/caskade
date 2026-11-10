# caskade-native-proto (BLITZ-0, #79)

A throwaway prototype of Caskade's library on Dioxus Native (Blitz), built to
measure Blitz against today's webview UI. Results and the decision are in
[`docs/spikes/blitz-0.md`](../../docs/spikes/blitz-0.md). Nothing ships from here.

It shows the shell (sidebar, top bar with one text field, status bar with the
spinner) and the library grid, virtualised as the webview's
(`VirtualLibrary.tsx`: 312 px rows, 176 px minimum cards, one row of
overscan), for a real library read straight from `caskade-core`
(`list_games_for_account`, 100 games a page around what is on screen). Covers
are the 320 px thumbnails the webview shows. The styles are the app's own:
`scripts/build-css.mjs` compiles `apps/desktop/src/styles/index.css` (tokens,
base, Tailwind utilities) and `themes/default/theme.css` against the classes
in `src/`. Arrow keys, Page Up/Down and a pad (through `caskade-input`) move
the focus between cards.

It has its own Cargo workspace and lockfile: the alpha Dioxus and beta Blitz
dependencies never reach the main workspace. Blitz is pinned to `9f9c8e3`
(0.3.0-beta.4, the PR #78 spike's commit) and anyrender to `812644a`; Dioxus
is 0.8.0-alpha.3.

## Patches (BLITZ-3, #82)

The prototype builds Blitz, anyrender and Stylo from
[`vendor/blitz/crates`](../../vendor/blitz/README.md), the same patched
crates `caskade-native` uses: `[patch]` entries in `Cargo.toml` point there
for every crate in `vendor/blitz/PINS`. Results in
[`docs/spikes/blitz-3.md`](../../docs/spikes/blitz-3.md).

## Build

```sh
cd apps/native-proto
node scripts/build-css.mjs                       # after changing classes in src/
flock /tmp/caskade-heavy.lock cargo build --profile proto                    # vello_hybrid (Blitz's default)
flock /tmp/caskade-heavy.lock cargo build --profile proto --features log-frame-times,log-phase-times
flock /tmp/caskade-heavy.lock cargo build --profile proto --no-default-features --features cpu   # vello_cpu
cargo build --profile proto --target x86_64-pc-windows-gnu                   # Windows
```

Renderer features: `hybrid` (default), `gpu` (vello), `cpu` (vello_cpu with
softbuffer), `skia`.

## Run

It needs a **copy** of a library, never the live one (opening it runs
migrations): `CASKADE_PROTO_HOME` holds `data/caskade/library.sqlite` and
`cache/caskade/artwork/` (or set `CASKADE_PROTO_DB` and
`CASKADE_PROTO_ARTWORK`).

| Variable                                                          | Effect                                                                                                                                        |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `CASKADE_PROTO_SPINNER=1`                                         | the status bar's spinner runs (the top bar's button toggles it too)                                                                           |
| `CASKADE_PROTO_AMBIENT=1`                                         | the default theme's ambient background animates (paused otherwise)                                                                            |
| `CASKADE_PROTO_FLEX_BUG=1`                                        | leaves out the BLITZ-2 workaround (`height: 0` on the grid's growing row)                                                                     |
| `CASKADE_PROTO_WIDTH`, `_HEIGHT`                                  | window size (1600×900)                                                                                                                        |
| `CASKADE_PROTO_ON_TOP=1`, `CASKADE_PROTO_POSITION=x,y`            | always on top without taking the focus, at a place: for real-display runs                                                                     |
| `CASKADE_PROTO_BENCH=idle\|spinner\|scroll`                       | after the first page and `_BENCH_SETTLE` s (5), idles or scrolls the grid at `_BENCH_SPEED` px/s (3000) for `_BENCH_SECONDS` (30), then exits |
| `CASKADE_PROTO_DEBUG=1`                                           | logs each render's scroll position and rows                                                                                                   |
| `CASKADE_PROTO_SPINNER_STEPS=8`                                   | the spinner turns in that many steps a turn (`steps(n)`)                                                                                      |
| `CASKADE_PROTO_GALLERY=1`                                         | shows `assets/gallery.html` (every animation in `themes/`) instead of the library                                                             |
| `CASKADE_PROTO_NO_BACKDROP=1`                                     | leaves out the theme's backdrop                                                                                                               |
| `CASKADE_PROTO_PROFILE=<file>`                                    | with `--features profile`: samples the bench window and writes self and inclusive time per function                                           |
| `BLITZ_DAMAGE=0`                                                  | (patched) repaints every frame in full, for comparisons                                                                                       |
| `BLITZ_PREFERS_REDUCED_MOTION=1`                                  | (patched) `prefers-reduced-motion: reduce`                                                                                                    |
| `ANYRENDER_FRAME_LIMIT_HZ=60`, `ANYRENDER_PRESENT_MODE=immediate` | (patched, measurement only) unthrottled presents paced like a 60 Hz vsync, where no display paces them                                        |
| `--core-only`                                                     | no window: opens the library and reads the first page (the baseline without UI)                                                               |

## Measure

- `scripts/run.sh`: starts an app (this prototype or a webview build) on a
  fresh copy of a library, in its own user and network namespace with a
  private Xvfb, D-Bus and keyring, and samples every process it runs
  (`scripts/sample.py`). For the webview, `--exe 'caskade|WebKit\w*Process'`
  and `--click '74,144;81,373'` (Library, then All Games, at 1440×900).
- `scripts/frames.py`: runs a `CASKADE_PROTO_BENCH` and reports frame pacing
  (from Blitz's `log-frame-times`), phase times and CPU between the bench's
  marks.
- `scripts/run.sh … --startup 20`: also grabs the display from the start and
  reports when the screen is visually complete (`startup.json`).
- `scripts/latency.sh`: pointer event to the end of the frame that answers it,
  on a private Xvfb.
- `scripts/gallery.sh`: every animation in `themes/` in Blitz and in
  WebKitGTK, each on a private Xvfb: which ones move, and a contact sheet.
- `scripts/display-suite.sh`: the real-display runs (idle, spinner, ambient,
  scroll), one window at a time, each driving itself: no synthetic input.
- `scripts/windows/`: the same on Windows (`setup.ps1` places a library copy,
  `launch.ps1` starts a command in the desktop session, `sample.ps1` samples).

### Platform runs still to do

On each machine, with a library copy in `~/blitz0-home` (as `run.sh` makes
one: `data/caskade/library.sqlite` and `cache/caskade/artwork/`), a display
that is on, and the prototype built there (or copied) into `bin/` as
`proto-hybrid` and `proto-hybrid-timed`:

```sh
apps/native-proto/scripts/display-suite.sh --bin bin --home ~/blitz0-home \
  --out blitz0-$(hostname)-$(date +%F) --refresh <the display's Hz>
```

Leave the machine alone while it runs (about 6 minutes); the window opens on
top without taking the focus. Wayland: unset `DISPLAY` so winit uses Wayland.
Steam Deck: run it inside gamescope (`gamescope -W 1280 -H 800 -r 60 -- …`).
Then send the `frames.json` files.

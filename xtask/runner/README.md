# Linux native / nightly runner contract (#50)

The general `/srv/caskade-cache` volume remains read-only in test jobs. Provision
these directories on the trusted runner before enabling the native workflows:

```
/srv/caskade-cache/native/share/caskade/umu/1.4.4/umu/umu-run
/srv/caskade-cache/native/share/umu/steamrt3/
/srv/caskade-cache/native/share/Steam/compatibilitytools.d/UMU-Proton-10.0-4/
/srv/caskade-fuzz-corpora/<target>/
```

Warm the first three directories under an unprivileged dedicated runner account
with `python3 xtask/runner/warm-proton.py --cache /srv/caskade-cache` (grant that
account write access to the native cache during this explicit provisioning step).
The tool downloads SHA-256-pinned umu-launcher 1.4.4, then asks umu to cache its
Steam Runtime and UMU-Proton using a disposable wineboot prefix. It never changes
HOME or a personal prefix. After warming, remount the cache read-only in jobs and
record versions/hashes in the runner receipt. Alternatively copy already-warmed
`~/.local/share/caskade/umu`, `~/.local/share/umu` and
`~/.local/share/Steam/compatibilitytools.d` from a dedicated runner account into
the paths above. UMU-Proton is required; GE-Proton10-34
crashes 32-bit fixtures on the development host. Never warm a personal library
or persist a game prefix as a shared cache.

`prepare-proton.py` checks for the warmed cache and copies it to a private
writable job-local XDG tree; HOME is never overwritten. Missing cache is a hard,
actionable setup error. A JSON receipt records the source/build and environment.
Tests use fresh per-game prefixes. The container's unprivileged `caskade` account
runs Proton under Xvfb (Steam Runtime refuses UID 0; the quiet Windows installers still need a display). The image includes Xvfb/xauth and Mesa/Vulkan loader libraries. Docker/Flatpak delegate tests keep the
existing private-daemon native job contract. General tool caches are never
written by a test job. Only trusted master/manual cache refresh may replace them.

`fuzz-corpus.py` is the separate corpus writer. The trusted master-only nightly
fuzz job gets `/srv/caskade-fuzz-corpora:/caskade-corpus:rw`; add that exact path
to act_runner `container.valid_volumes`. It never grants access to fork jobs.
Each target has its own directory. Before/after fuzzing, merge only regular
flat SHA-named input files, at most 1 MiB each / 100,000 files / 256 MiB per
target. Contents are rehashed; writes replace atomically; links/special files
and unsafe names are ignored. All targets come from the actual fuzz manifest,
so newly added parsers automatically receive a 600-second nightly run.

Native profile evidence includes Proton install/launch/Z-drive isolation, legacy
registry self-check, and actual own-uninstaller cleanup under Proton. The
performance profile generates its sparse 20 GiB probe and 10 GiB extraction
workloads under `/tmp` (minimum 12 GiB free), never commits them, and retains JSON
measurements + Criterion reports. The scoreboard consumes the native JUnit for
isolated system delegates and runs real CLI probe/plan/install/verify/uninstall
for other available minis. Foreign host rows are labelled plan-only/not-run;
missing runner-only fixture families are explicitly pending, never counted as
passed. Linux launch acceptance is separately proven by `linux_mini`, Flatpak,
Debian/Fedora and Proton native tests.

Validate tooling without touching any real installation/cache:

```sh
python3 -m unittest discover -s xtask/runner -p 'test_*.py' -v
```

The native and scoreboard containers provide a private 8 GiB executable tmpfs at
`/caskade-move`, passed as `CASKADE_TEST_MOVE_ROOT`, for #62 second-filesystem
Proton moving tests. It is separate from the checkout filesystem and vanishes
with the job. `moving_proton` runs in the same unprivileged Proton group; the
other native tests run separately. No host mount or sudo action is needed.

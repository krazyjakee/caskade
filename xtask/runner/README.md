# Linux native runner contract (#50)

The `installer-linux-native` job in `.github/workflows/ci.yml` runs on a
GitHub-hosted `ubuntu-latest` runner (see `crates/caskade-installer/docs/ci.md`).
Its cached tools are one GitHub Actions cache entry, `installer-proton-<hash of
warm-proton.py>`, holding `~/caskade-cache/native/share`:

```
native/share/caskade/umu/1.4.4/umu/umu-run
native/share/umu/steamrt3/
native/share/Steam/compatibilitytools.d/UMU-Proton-<version>/
```

On a cache miss the job runs
`python3 xtask/runner/warm-proton.py --cache ~/caskade-cache` as the runner's
unprivileged account, and master's runs save the result (GitHub evicts entries
unused for 7 days; changing `warm-proton.py` starts a new key). The tool downloads SHA-256-pinned umu-launcher 1.4.4,
then asks umu to cache its Steam Runtime and UMU-Proton using a disposable
wineboot prefix. It never changes HOME or a personal prefix. Only master runs
write the cache master's and pull requests' runs read. To warm a local cache,
run the same command with `--cache` pointing at a private directory. UMU-Proton is required;
GE-Proton10-34 crashes 32-bit fixtures on the development host. Never warm a
personal library or persist a game prefix as a shared cache.

Ubuntu's AppArmor refuses unprivileged user namespaces by default; the job sets
`kernel.apparmor_restrict_unprivileged_userns=0`, because Steam Runtime's
pressure-vessel (bwrap) and Flatpak need them.

`prepare-proton.py` checks for the warmed cache and copies it to a private
writable job-local XDG tree; HOME is never overwritten. Missing cache is a hard,
actionable setup error. A JSON receipt records the source/build and environment.
It replaces the trees it copies, so anything already at the destination is safe;
keep the destination out of `target/`, which CI caches.
Tests use fresh per-game prefixes. `native-profile.py` runs Proton under Xvfb
as the runner's unprivileged user (Steam Runtime refuses UID 0; the quiet Windows
installers still need a display). Run as root, it only proceeds inside a fresh
Actions job container, where it hands the workspace to the `caskade` user.

`fuzz-corpus.py` keeps a fuzz target's corpus between local runs (fuzzing has no
workflow; see the docs' Local-only runs): merge a retained corpus into
`crates/caskade-installer/fuzz/corpus/<target>` before `just fuzz <target>`, and
the result back out afterwards. Merges keep only regular flat SHA-named input files, at most 1 MiB each /
100,000 files / 256 MiB per target. Contents are rehashed; writes replace
atomically; links/special files and unsafe names are ignored. All targets come
from the actual fuzz manifest.

Native profile evidence includes Proton install/launch/Z-drive isolation, legacy
registry self-check, and actual own-uninstaller cleanup under Proton. The local
performance gates generate their sparse 20 GiB probe and 10 GiB extraction
workloads under `/tmp` (minimum 12 GiB free), never commit them, and write JSON
measurements + Criterion reports. The local scoreboard (`just corpus`) can
consume the native JUnit for isolated system delegates and runs real CLI
probe/plan/install/verify/uninstall for other available minis. Foreign host rows are labelled plan-only/not-run; missing
runner-only fixture families are explicitly pending, never counted as passed.
Linux launch acceptance is separately proven by `linux_mini`, Flatpak,
Debian/Fedora and Proton native tests.

Validate tooling without touching any real installation/cache:

```sh
python3 -m unittest discover -s xtask/runner -p 'test_*.py' -v
```

The #62 second-filesystem Proton moves need `CASKADE_TEST_MOVE_ROOT` on another
filesystem with 4 GiB free. The jobs use the runner's separate `/mnt` disk, or
an 8 GiB executable tmpfs mounted there when `/mnt` shares the workspace's
filesystem. `moving_proton` runs in the same unprivileged Proton group; the
other native tests run separately.

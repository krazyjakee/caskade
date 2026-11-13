# Pinned 7-Zip sidecar (I0 preparation for I5)

The pin is **26.05**. Archive SHA-256 values in `7zz-pins.json` come from the
[official release assets](https://github.com/ip7z/7zip/releases/tag/26.05).
`7zz-download.py` verifies packages before extracting known members. It never
runs an installer. Downloads and unpacked binaries live under ignored
`target/sidecars/7zip/26.05/`; no binary belongs in Git.

```sh
python3 scripts/7zz-download.py
python3 scripts/7zz-download.py --all
```

Python 3's standard library is sufficient. Preparing Windows on Windows uses the
SHA-pinned official `7zr.exe` solely to unpack the verified installer as an
archive. Cross-packaging Windows on Linux/macOS uses the verified native `7zz`
from the same cache, or an explicit `--bootstrap` extractor. The Windows console
is upstream **7z.exe plus 7z.dll**, renamed to `7zz.exe` for a common sidecar
name. `7za.exe` is unsuitable for the RAR gate. System p7zip is not the pinned
sidecar and is never a substitute for acceptance evidence.

Populate `HostCaps.seven_zip` with the downloader's absolute `program` path.
`SevenZip::from_host`/`SevenZip::pinned` verify the program hash and the Windows
DLL hash. `SevenZip::new` is explicit utility injection for tests. The host
capability detector may discover an unpinned system tool; production sidecar
construction rejects it until configured with a verified package.

Native fixture check (wrap every Cargo invocation in the shared heavy lane):

```sh
CASKADE_7ZZ=/absolute/path/to/7zz flock /tmp/caskade-heavy.lock \
  cargo test -p caskade-installer --test it sevenzip:: -- --ignored
```

Normal tests run the fake utility, including process-tree cancellation. Pinned
RAR tests are explicitly gated so CI cannot silently pass by skipping a missing
binary: #22 must download the pin, set `CASKADE_7ZZ`, and run the ignored tests on
Linux, Windows and macOS. Windows tests also require Python for fake utilities.

## In the app bundles

`scripts/7zz-sidecar.py` stages the verified program for a target as ignored
`apps/native/binaries/7zz-<target-triple>[.exe]`, with the upstream
License.txt as `7zip-License.txt` and, for Windows, the matching `7z.dll`.
`apps/native/Tauri.toml` packages it (`bundle.externalBin = ["binaries/7zz"]`)
next to the `caskade` executable: `/usr/bin/7zz` from the deb, `7zz.exe` with
`7z.dll` beside it in the Windows install folder, and
`Caskade.app/Contents/MacOS/7zz`. Every bundle carries the license under
`licenses/`. The DLL is a resource, never in externalBin or launched.
`scripts/stage-bundle.py` runs the script for its target, so `just build-app`
and release CI stage it with the app; `--dev` (`just dev`) puts it beside the
host's debug native executable instead.

```sh
python3 scripts/7zz-sidecar.py
# For a Linux Windows-cross check:
python3 scripts/7zz-sidecar.py --target x86_64-pc-windows-gnu
```

The triples are `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` and
`aarch64-pc-windows-msvc` (the x64 Windows payload also serves GNU test
builds). The macOS payload is universal; each Windows bundle gets only its own
architecture's DLL. The app finds the program beside its executable
(`caskade_app::commands::install::sidecar`), and `InstallService::with_sidecar`
verifies the program and Windows DLL hashes with the installer's pinned
loader. Keep installer library code independent of the packaging. App
signing/notarization runs in the packaging workflow without changing the
pinned executable's bytes before verification; a signing pipeline that changes
them must explicitly update its verified artifact manifest. Windows
MSVC/ARM64 and macOS signing/notarization remain native verification items on
#22.

## Execution and fallback

`SevenZipHandler` claims RAR/CAB/WIM/ISO, `pe.sfx_format`, and low-confidence
unknown archive fallback inputs when the host has a sidecar. `SevenZipExtractor`
implements the executor seam. `StepContext::create_scratch` supplies a temporary
child inside the actual active staging/final directory; it is deleted before
return. All listing paths, sizes, collisions and link graphs are checked before
launching `x`. No absolute-path extraction switches or link-safety bypasses are
enabled. Pipe output, duration and observed scratch entries/bytes are capped.
The external utility is trusted code; scratch is quarantine, not an OS sandbox.
Every imported file/link goes through `SafeDir`, then regular files are re-probed.

Apps can retain an `Arc<SevenZipExtractor>` and consume `take_artifacts` after a
successful executor result for another planning pass. Utility `-bsp1` percentages
arrive through `with_progress`; actual byte events represent validated imports.
A native-parser fallback must start in a fresh/rolled-back staging attempt;
never retry a safety, cancellation or disk-space rejection through a looser path.

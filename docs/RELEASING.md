# Releasing

Releases are built on GitHub Actions by `.github/workflows/release.yml`, from a
`vX.Y.Z` tag pushed to the GitHub repo `krazyjakee/caskade`, where all work now
happens. (Gitea is archived; GitHub's history before 3 Nov 2026 is one summary
commit per publish.)

The workflow builds Linux x86_64 (deb and AppImage, on Ubuntu 22.04 so the
AppImage runs on older glibc), Windows x86_64 (NSIS installer) and macOS on
Apple silicon and Intel (app and dmg). It signs the updater bundles, uploads
everything to a draft release with a `latest.json` for the in-app updater, and
checks that `latest.json` lists all four platforms. Nothing reaches users until
you publish the draft: the updater reads
`https://github.com/krazyjakee/caskade/releases/latest/download/latest.json`,
and `releases/latest` skips drafts and prereleases.

Every bundle's executable is the native app, `caskade-native` built and
installed under the name `caskade`: `/usr/bin/caskade` from the deb,
`caskade.exe` in the install folder on Windows and
`Caskade.app/Contents/MacOS/caskade` on macOS. That is the executable earlier
releases' shortcuts, `caskade://` handler, login item and launch caller
started, so they keep working after an update. The pinned 7-Zip (`7zz`) is
beside it (`scripts/7zz-packaging.md`). There is no other UI in the bundles:
the Tauri/React app and `--legacy` are gone (#143).

The jobs, in order:

- `prepare` checks the tag, the versions and the signing secrets, and makes
  the draft.
- `build` (one leg per target, `.github/workflows/build-bundles.yml`) builds
  caskade-native for the target, stages it and 7-Zip
  (`scripts/stage-bundle.py`), then makes the bundles (`tauri bundle
  --no-sign`), with a read-only token and no signing key, and uploads them as
  workflow artifacts. Build scripts, proc-macros and npm packages run here,
  so the key never does.
- `sign` downloads the artifacts and signs the updater bundles with
  `tauri signer sign` from the Tauri CLI version in `pnpm-lock.yaml`. It runs
  no project code. It writes `latest.json` and uploads everything to the
  draft. Only this job and `prepare` see the key.
- `verify` reads the draft back and checks `latest.json` against it: every
  platform's file, and that its signature is the committed key's and carries
  the release's version.

To check the bundles of any commit before tagging, run the `bundles`
workflow (Actions → bundles → Run workflow, or
`gh workflow run bundles.yml --repo krazyjakee/caskade --ref master`). It
runs the same build legs and leaves the bundles as the run's artifacts
(`bundles-<target>`, kept 7 days): no tag, no draft, no signatures, no
secrets.

## The bundler

The bundles are made by the Tauri CLI's bundler, `tauri bundle` from
`@tauri-apps/cli` (pinned in `apps/native/package.json`), and signed by its
`tauri signer sign`. Both are build tools: Caskade has no Tauri runtime, and
the CLI bundles the already built native executable. Its configuration is
`apps/native/Tauri.toml`, with `Tauri.windows.toml` and `Tauri.macos.toml`
beside it; the Windows installer's hooks are
`apps/native/packaging/windows/hooks.nsh`. The version comes from
`apps/native/Cargo.toml`.

`cargo-packager` (#143) was considered as a standalone replacement and not
taken, because installed copies would notice:

- Its signer writes no version into a signature's trusted comment;
  `tauri signer sign --app-version` does, and the updater refuses a signature
  for another version than the one announced.
- Its NSIS installer is an older fork of Tauri's: no installer hooks, no
  update mode (an update brings back shortcuts the user removed), no
  restart as the signed-in user, and no WebView2 install, which the game and
  video windows need. The install folder, uninstall key and `uninstall.exe`
  matched; the rest would change under existing installs.
- Its AppImage leaves out WebKitGTK's helper processes and the GTK plugin,
  so game and video windows wouldn't start from the AppImage.
- It makes no macOS updater tarball.

Reconsider if the Tauri CLI stops bundling a crate with no Tauri dependency,
or if another bundler covers the points above.

Keep `productName`, `identifier`, `mainBinaryName` and the `caskade` scheme
in `Tauri.toml` as they are: the install folder
(`%LOCALAPPDATA%\Caskade`), uninstall and publisher registry keys, the deb
package, the macOS bundle, shortcuts and the link handler follow them.
`scripts/check-release-metadata.py` fails if they change, and CI runs it.
`plugins.updater.pubkey` there is the path of
`crates/caskade-app/updater.pub`, so the key has one copy; the bundler needs
it only to make updater bundles.

## One-time setup

1. Create the GitHub repo. It has to be public: installed copies download
   updates without logging in.

   ```sh
   gh repo create krazyjakee/caskade --public
   ```

2. Make it the checkout's `origin`:

   ```sh
   git remote add origin https://github.com/krazyjakee/caskade.git
   ```

3. Give the workflow the updater's signing key. `gh` drops the files' trailing
   newline.

   ```sh
   gh secret set TAURI_SIGNING_PRIVATE_KEY --repo krazyjakee/caskade < ~/.tauri/caskade-updater.key
   gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo krazyjakee/caskade < ~/.tauri/caskade-updater.password
   ```

   The workflow stops before building when either secret is missing, when the
   password is wrong, or when the key isn't the pair of
   `crates/caskade-app/updater.pub`. The app embeds this file. Keep this
   trust key; installed copies rely on it.

4. Back up `~/.tauri/caskade-updater.key` and `caskade-updater.password`
   somewhere offline, such as a password manager. Installed copies only accept
   updates signed with this key. If it is lost, they can never update again:
   a new key only reaches users through a build they download and install by
   hand.

## Cutting a release

1. Set the new version in `apps/native/Cargo.toml` (`[package] version`),
   then run
   `flock /tmp/caskade-heavy.lock cargo metadata --format-version 1 > /dev/null`
   to update its entry in `Cargo.lock` (the release build uses `--locked`).
   The bundles take their version from there.

   Run `python3 scripts/check-release-metadata.py <version>` to check both
   version entries, the bundle identity and the updater key, then commit them
   to `master`.

2. Push `master`, then tag it:

   ```sh
   git push origin master
   git tag -a v0.2.0 -m "Caskade 0.2.0"
   git push origin v0.2.0
   ```

   A version with a suffix (`v0.2.0-beta.1`) becomes a prerelease, which the
   updater never offers.

3. Watch the `release` run on GitHub. The `prepare` job fails within a minute
   or two on a tag that doesn't match the version, on missing secrets or on
   a wrong key. When it's done, the run's summary links the draft.

4. Review the draft: the files (`.deb`, `.AppImage`, `-setup.exe`, two
   `.dmg`, two `.app.tar.gz`, their `.sig` files and `latest.json`), and the
   release notes. Test the update if the release changes anything about
   installing or starting (below).

5. Publish it on the release page, or:

   ```sh
   gh release edit v0.2.0 --repo krazyjakee/caskade --draft=false
   ```

   Installed copies find it at their next check: shortly after they start,
   then every six hours.

If a build job fails, use Re-run failed jobs on the run's page: `sign` and
`verify` run once every leg has passed. To build an existing tag
again, run the workflow by hand (Actions → release → Run workflow) with the
tag. It uploads to the same draft and replaces files of the same name. A
published release is never rebuilt: fix the problem, bump the version and tag
again.

`latest.json` also has a `linux-x86_64-deb` entry, because the `sign` job signs
the deb too. Copies installed from the deb update from it (see below). Each
signature is bound to the version (`signer sign --app-version`), and the
updater refuses one for another version
(`crates/caskade-app/src/self_update.rs`).

## What updates itself

| Installed from | Updates in the app |
| --- | --- |
| AppImage | Yes |
| Windows installer (NSIS) | Yes |
| macOS app (from the dmg) | Yes |
| `.deb` | Yes, from 0.1.2: Caskade installs the new `.deb` with `pkexec apt-get install` (dpkg without apt), so the system's password prompt appears once. It never falls back to asking for the password itself or to `sudo`. 0.1.1 and earlier say to use the package manager: install the next `.deb` over them once (`sudo apt install ./Caskade_0.1.2_amd64.deb`). |
| Development build (`just dev`, `cargo run`) | No |

An update downloads only when the user asks for it, and the new version runs
after a restart.

Updates from 0.1.x, whose bundles carried the Tauri app as `caskade` and the
native app as `caskade-native`, replace `caskade` with the native app. The
deb removes `/usr/bin/caskade-native` as a file of the old package; the
AppImage and macOS app are replaced whole. The Windows installer's hooks
close a running `caskade-native.exe` and delete it, so it can't keep the old
version running or keep the folder after an uninstall.

The bundles aren't code-signed or notarized yet. Windows SmartScreen warns
about an unknown publisher (More info → Run anyway). On macOS, Gatekeeper
blocks the first start: allow it under System Settings → Privacy & Security,
or run `xattr -dr com.apple.quarantine /Applications/Caskade.app`. The updater
signature is separate from OS code signing and is always checked.

## What the bundles contain

Every bundle carries `caskade` (the native app) and `7zz`, and Windows's
`7z.dll`, with 7-Zip's license under `licenses/`. No developer tool is
packaged: the bundler takes only the staged executable and what
`Tauri.toml` lists. Release builds strip debuginfo but keep symbol names, so
panic backtraces in users' logs still name functions. That saves about 8 MB
(`caskade-native` 222.6 → 214.6 MB on Linux); a full strip would save about
64 MB but lose those names.

## Testing an update before publishing

A draft's files only download when logged in, so serve them locally and build
an older Caskade that reads from there. `CASKADE_UPDATER_ENDPOINT` replaces
the feed URL at build time. On Linux, with the draft for `v0.2.0`:

```sh
mkdir -p /tmp/caskade-update && cd /tmp/caskade-update
gh release download v0.2.0 --repo krazyjakee/caskade -p latest.json -p '*.AppImage'
sed -i 's#https://github.com/krazyjakee/caskade/releases/download/v0.2.0/#http://127.0.0.1:8000/#' latest.json
python3 -m http.server 8000 --bind 127.0.0.1
```

In another terminal, from the repo, build an AppImage that calls itself 0.0.1:

```sh
# In a checkout whose apps/native/Cargo.toml version is 0.0.1:
CASKADE_UPDATER_ENDPOINT=http://127.0.0.1:8000/latest.json just build-app
```

Run `Caskade_0.0.1_amd64.AppImage` from
`${CARGO_TARGET_DIR:-target}/debug/bundle/appimage/`, press Check now under
Settings → General → Caskade updates, then Download and install. After the
restart, the app should report 0.2.0.

Check that the installed shortcut opens Caskade, and verify a second start,
`--console`, a `caskade://` link and `--launch` as well.

The other bundles test the same way on their own OS: download the
`-setup.exe` or the right `.app.tar.gz`, point `latest.json` at it, build the
app and stage it (the first two lines of `just build-app`), then bundle with
`pnpm --dir apps/native tauri bundle --debug --no-sign --no-binary-patching
--bundles nsis` (or `--bundles app`). To check an update from 0.1.x, install
that release's bundle first: on Windows, `caskade-native.exe` must be gone
from the install folder afterwards.

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

The jobs, in order:

- `prepare` checks the tag, the versions and the signing secrets, and makes
  the draft.
- `build` (one leg per target) builds with a read-only token and no signing
  key (`tauri build --no-sign`), and uploads its bundles as workflow
  artifacts. Build scripts, proc-macros and npm packages run here, so the key
  never does.
- `sign` downloads the artifacts and signs the updater bundles with
  `tauri signer sign` from the Tauri CLI version in `pnpm-lock.yaml`. It runs
  no project code. It writes `latest.json` and uploads everything to the
  draft. Only this job and `prepare` see the key.
- `verify` reads the draft back and checks `latest.json` against it.

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
   `plugins.updater.pubkey` in `apps/desktop/src-tauri/tauri.conf.json`.

4. Back up `~/.tauri/caskade-updater.key` and `caskade-updater.password`
   somewhere offline, such as a password manager. Installed copies only accept
   updates signed with this key. If it is lost, they can never update again:
   a new key only reaches users through a build they download and install by
   hand.

## Cutting a release

1. Set the new version in each place the workflow checks:
   - `apps/desktop/src-tauri/tauri.conf.json` (`version`)
   - `apps/desktop/src-tauri/Cargo.toml` (`[package] version`), then run
     `cargo metadata --format-version 1 > /dev/null` to update its entry in
     `Cargo.lock` (the release build uses `--locked`)
   - `apps/desktop/package.json` (`version`)

   Commit them to `master`.

2. Push `master`, then tag it:

   ```sh
   git push origin master
   git tag -a v0.2.0 -m "Caskade 0.2.0"
   git push origin v0.2.0
   ```

   A version with a suffix (`v0.2.0-beta.1`) becomes a prerelease, which the
   updater never offers.

3. Watch the `release` run on GitHub. The `prepare` job fails within a minute
   or two on a tag that doesn't match the versions, on missing secrets or on
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
the deb too, as `tauri build` would. Copies installed from the deb update from
it (see below). Each
signature is bound to the version (`signer sign --app-version`), because the
app's updater config has `requireSignedVersion` on.

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

The bundles aren't code-signed or notarized yet. Windows SmartScreen warns
about an unknown publisher (More info → Run anyway). On macOS, Gatekeeper
blocks the first start: allow it under System Settings → Privacy & Security,
or run `xattr -dr com.apple.quarantine /Applications/Caskade.app`. The updater
signature is separate from OS code signing and is always checked.

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
cd apps/desktop
CASKADE_UPDATER_ENDPOINT=http://127.0.0.1:8000/latest.json \
  flock /tmp/caskade-heavy.lock pnpm tauri build --debug --bundles appimage --config '{"version":"0.0.1"}'
```

Run `Caskade_0.0.1_amd64.AppImage` from
`${CARGO_TARGET_DIR:-target}/debug/bundle/appimage/`, press Check now under
Settings → General → Caskade updates, then Download and install. After the restart, the app should report 0.2.0. A debug build accepts
the plain `http` feed with a warning; a release build refuses it.

The other bundles test the same way on their own OS: download the
`-setup.exe` or the right `.app.tar.gz`, point `latest.json` at it, and build
with `--bundles nsis` or `--bundles app`.

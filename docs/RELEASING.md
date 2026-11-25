# Releasing

Releases are built on GitHub Actions by `.github/workflows/release.yml`, from a
`vX.Y.Z` tag pushed to the GitHub repo `krazyjakee/caskade`, where all work now
happens. (Gitea is archived; GitHub's history before 3 Nov 2026 is one summary
commit per publish.)

The workflow builds Linux x86_64 (deb and AppImage, on Ubuntu 22.04 so the
AppImage runs on older glibc), Windows x86_64 (NSIS installer) and macOS on
Apple silicon and Intel (app and dmg). It signs and notarizes the macOS app
and dmg with Caskade's Developer ID, code-signs the Windows installer and the
Caskade it installs with a Certum certificate, signs the updater bundles, uploads
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
- `notarize` (one leg per macOS target, on a macOS runner) signs the leg's
  app, and the 7zz in it, with the Developer ID and the hardened runtime,
  has Apple notarize the app and then the dmg it puts it in, staples the
  tickets, and checks them with macOS's `codesign`, `spctl` and `stapler`
  (`scripts/macos/sign.sh`, below). Only this job and `prepare` see the Apple
  secrets, and it installs no project dependencies.
- `windows-sign` (`.github/workflows/sign-windows.yml`, on a Windows runner)
  logs in to Certum's cloud signing, bundles the Windows leg's `caskade.exe`
  again with the pinned Tauri CLI, signing as it goes, and checks the result
  with a silent install (below). Only this job and `prepare` see the
  SimplySign secrets; it builds nothing and installs no project dependencies.
- `sign` downloads the artifacts, takes the notarized macOS files and the
  signed Windows installer in place of the build legs' unsigned ones, signs the
  updater bundles with `tauri signer sign` from the Tauri CLI version in
  `pnpm-lock.yaml`, generates `SHA256SUMS` over all downloadable bundles,
  writes `latest.json` (populating `notes` from `CHANGELOG.md` via
  `scripts/changelog-notes.py`), and uploads everything to the draft. It runs
  no project code. Only this job and `prepare` see the key.
- `verify` reads the draft back and checks `latest.json` and `SHA256SUMS`
  against it: every platform's file, its checksum entry, updater notes, and
  that each signature is the committed key's and carries the release's version.

To check the bundles of any commit before tagging, run the `bundles`
workflow (Actions → bundles → Run workflow, or
`gh workflow run bundles.yml --repo krazyjakee/caskade --ref master`). It
runs the same build legs and leaves the bundles as the run's artifacts
(`bundles-<target>`, kept 7 days): no tag, no draft, no signatures, no
secrets. Its macOS app is only ad-hoc signed, so a downloaded copy needs
`xattr -dr com.apple.quarantine` before macOS opens it, and its Windows
installer isn't code-signed (the `windows signing smoke` workflow can sign
it, below).

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

5. Give the workflow the macOS signing identity and notary key (made below,
   in `~/.apple-signing/caskade`):

   ```sh
   base64 -w0 ~/.apple-signing/caskade/identity.pem | gh secret set MACOS_SIGNING_PEM_B64 --repo krazyjakee/caskade
   base64 -w0 ~/.apple-signing/caskade/notary.json | gh secret set MACOS_NOTARY_KEY_B64 --repo krazyjakee/caskade
   ```

   `prepare` stops the release when either is missing: a macOS build that
   isn't notarized is one macOS calls damaged after a download.

6. Give the workflow the Windows signing login, `SIMPLYSIGN_USER` and
   `SIMPLYSIGN_TOTP_SECRET`, and the variable `WINDOWS_TIMESTAMP_URL`
   (`http://time.certum.pl`). See Windows code signing, below. `prepare`
   stops the release when either secret is missing.

## macOS signing and notarization

`scripts/macos/sign.sh` signs with [rcodesign] (apple-codesign 0.29.0; the
release job downloads it SHA-256-checked), which needs no Mac and no
Keychain: everything but the dmg works on Linux too. With
`--for-notarization` it turns on the hardened runtime and requires a
timestamp, then the script reads every signature back rather than trusting
rcodesign's exit code (it exits 0 on an ad-hoc result). The hardened runtime
gets one entitlement back, for the plugins' wasmtime
(`scripts/macos/entitlements.plist` says why; CI's `native-macos` job checks
a hardened build still compiles wasm, and warns if it no longer needs it).

Re-signing changes the bundled 7zz's bytes, and its timestamp changes them on
every release, so on macOS the app also accepts a 7zz that meets
`MACOS_RELEASE_REQUIREMENT` (`SevenZip::pinned`, `scripts/7zz-packaging.md`).
That names the team, `8MVM4FMRRC`: signing with another team's identity
needs that constant changed too, or the release's 7-Zip is refused.

To sign a bundle by hand (add `--notarize` to send it to Apple too):

```sh
MACOS_SIGNING_PEM=~/.apple-signing/caskade/identity.pem \
MACOS_NOTARY_KEY=~/.apple-signing/caskade/notary.json \
  scripts/macos/sign.sh Caskade_0.2.1_aarch64.app.tar.gz
```

The identity, minted without a Mac (keep `~/.apple-signing` out of the repo):

1. A key and a certificate signing request. Apple never sees the key; losing
   it means revoking the certificate and starting again.

   ```sh
   d=~/.apple-signing/caskade
   mkdir -p "$d" && chmod 700 "$d"
   openssl genrsa -out "$d/developer_id.key" 2048 && chmod 600 "$d/developer_id.key"
   openssl req -new -key "$d/developer_id.key" -out "$d/developer_id.csr" -sha256 \
     -subj "/emailAddress=<account email>/CN=<name>/C=GB"
   ```

2. At [Certificates][certs], add a **Developer ID Application** certificate
   with the **G2 Sub-CA** profile, upload the CSR and download the `.cer`.
   Only the Account Holder can, and an account has five.

3. The PEM bundle: key, certificate, then its chain, in that order.

   ```sh
   curl -fsSLO https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer
   curl -fsSLO https://www.apple.com/appleca/AppleIncRootCertificate.cer
   { cat "$d/developer_id.key"
     openssl x509 -inform DER -in "$d/developerID_application.cer"
     openssl x509 -inform DER -in DeveloperIDG2CA.cer
     openssl x509 -inform DER -in AppleIncRootCertificate.cer
   } > "$d/identity.pem" && chmod 600 "$d/identity.pem"
   ```

4. An App Store Connect API key with the Developer role, under
   [Integrations][asc] (the `.p8` downloads once), in rcodesign's form:

   ```sh
   rcodesign encode-app-store-connect-api-key -o "$d/notary.json" <issuer-id> <key-id> AuthKey_<key-id>.p8
   ```

Notarization also needs the developer account's current agreements accepted:
Apple answers `FORBIDDEN.REQUIRED_AGREEMENTS_MISSING_OR_EXPIRED` until the
Account Holder accepts them on developer.apple.com.

[rcodesign]: https://github.com/indygreg/apple-platform-rs
[certs]: https://developer.apple.com/account/resources/certificates/list
[asc]: https://appstoreconnect.apple.com/access/integrations/api

## Windows code signing

The Windows installer, `caskade.exe`, the uninstaller and the installer's
NSIS plugins are signed with the Certum "Open Source Code Signing in the
Cloud" certificate that MoGen and daccord also use. It is issued to "Open
Source Developer Jacob Cattrall" and expires 2027-08-11; Caskade qualifies
because it is public and open source. Its key can't be exported and stays
in Certum's cloud, so there is no `.pfx`:

- `scripts/windows/simplysign-login.ps1` installs SimplySign Desktop on the
  runner (a pinned version, its checksum and Authenticode signature
  checked), makes the one-time code from `SIMPLYSIGN_TOTP_SECRET` (the
  whole `otpauth://` link: Certum's codes are HMAC-SHA256), logs in as
  `SIMPLYSIGN_USER` (the account ID in Certum CertManager) and waits for the
  certificate to appear in `Cert:\CurrentUser\My`. It hands its SHA-1
  fingerprint to later steps as `WINDOWS_CERT_SHA1`.
- `scripts/windows/sign.ps1` signs with that certificate through
  `signtool`, timestamps with `WINDOWS_TIMESTAMP_URL` (Certum's
  `http://time.certum.pl` when unset; tried three times) and checks each
  file with `signtool verify /pa`. It never signs the pinned 7-Zip
  (`7zz.exe`, `7z.dll`): the app accepts those by their SHA-256
  (`SevenZip::pinned`), and a signature would change their bytes.
- `sign-windows.yml` gives the Tauri CLI's bundler `sign.ps1` as its
  `bundle.windows.signCommand` and bundles the build leg's staged files
  (the `staged-x86_64-pc-windows-msvc` artifact) again. The bundler signs
  `caskade.exe` before packing it, the NSIS plugins, the uninstaller as
  NSIS writes it, and the setup `.exe`; `TAURI_SKIP_SIDECAR_SIGNATURE_CHECK`
  keeps it off the sidecar 7zz. The job then checks the 7-Zip files are
  unchanged, installs the setup silently and checks that the installed
  `caskade.exe` and `uninstall.exe` and the setup are signed, trusted and
  timestamped, and that the installed 7-Zip is the pinned one.

The signed setup `.exe` replaces the unsigned one before the `sign` job
makes its updater signature, so updates install signed files too. A
release never falls back to an unsigned installer: missing secrets stop
`prepare`, and any failed login, signature, timestamp or check stops the
release.

To check the credentials without a release, run the `windows signing smoke`
workflow (Actions → windows signing smoke → Run workflow): it signs and
verifies a throwaway `.exe`. Give it a `bundles` run's id to also sign that
run's Windows build exactly as a release does:

```sh
gh workflow run windows-signing-smoke.yml --repo krazyjakee/caskade --ref master -f bundles-run=<run id>
```

Things to know:

- **Renewal**: the scripts find the certificate at login, not by a stored
  fingerprint, so renewing it with Certum needs no secret changes.
- **Don't use "Odzyskaj dostęp"** (recover access) in CertManager: it would
  most likely issue a new TOTP secret, and signing would break in Caskade,
  MoGen and daccord until all three secrets are updated.
- **Shared login**: the three repos log in to the same SimplySign account.
  Caskade's own signing jobs queue behind each other (the `simplysign`
  concurrency group), but don't run a Caskade release while MoGen or daccord
  is signing.
- **Who can sign**: with the TOTP secret in CI, two-factor login becomes one
  factor for anyone who can change these repos' workflows or secrets.
- **Unsigned files**: only the pinned 7-Zip, by design (above).
- **Moving the secrets**: GitHub secrets can't be read back. They came from
  MoGen through a temporary workflow on a throwaway branch there, which
  encrypted each with this repo's secrets public key and printed only the
  ciphertext, then `PUT /repos/krazyjakee/caskade/actions/secrets/<name>`;
  the branch and its run were deleted afterwards. Do the same to copy them
  to another repo.

## Cutting a release

1. Move the relevant entries in `CHANGELOG.md` from `## [Unreleased]` under a
   new `## [<version>] - YYYY-MM-DD` heading. `release.yml` extracts that
   section via `scripts/changelog-notes.py` for both the GitHub Release body
   and `latest.json`'s `notes` field (prerelease tags fall back to
   `[Unreleased]` if no exact section exists).

2. Set the new version in `apps/native/Cargo.toml` (`[package] version`),
   then run
   `flock /tmp/caskade-heavy.lock cargo metadata --format-version 1 > /dev/null`
   to update its entry in `Cargo.lock` (the release build uses `--locked`).
   The bundles take their version from there.

   Run `python3 scripts/check-release-metadata.py <version>` to check both
   version entries, the bundle identity and the updater key, then commit them
   (with `CHANGELOG.md`) to `master`.

3. Push `master`, then tag it:

   ```sh
   git push origin master
   git tag -a v0.2.3 -m "Caskade 0.2.3"
   git push origin v0.2.3
   ```

   A version with a suffix (`v0.2.3-beta.1`) becomes a prerelease, which the
   updater never offers.

4. Watch the `release` run on GitHub. The `prepare` job fails within a minute
   or two on a tag that doesn't match the version, on missing secrets or on
   a wrong key. When it's done, the run's summary links the draft.

5. Review the draft: the files (`.deb`, `.AppImage`, `-setup.exe`, two
   `.dmg`, two `.app.tar.gz`, their `.sig` files, `SHA256SUMS` and
   `latest.json`), and the populated release notes. Test the update if the
   release changes anything about installing or starting (below).

6. Publish it on the release page, or:

   ```sh
   gh release edit v0.2.3 --repo krazyjakee/caskade --draft=false
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
updater refuses one for another version or one that names no version
(`crates/caskade-app/src/self_update.rs`, #151): every release since 0.1.1
was signed with its version.

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

The macOS app and dmg are signed with Caskade's Developer ID and notarized,
with the tickets stapled, so they open after a download, offline too. The
Windows installer and what it installs are code-signed and timestamped
(Windows code signing, above); SmartScreen may still warn about a new
certificate until it has built a reputation. The updater signature is
separate from OS code signing and is always checked.

## What the bundles contain

Every bundle carries `caskade` (the native app) and `7zz`, and Windows's
`7z.dll`, with 7-Zip's license under `licenses/`. No developer tool is
packaged: the bundler takes only the staged executable and what
`Tauri.toml` lists.

### Binary size and release profile tradeoffs

Release builds currently set `[profile.release] strip = "debuginfo"` in the
root `Cargo.toml`, which drops DWARF debug info while keeping the `.symtab`
symbol table so panic backtraces in users' logs still name Rust functions.

- **Current baseline (`strip = "debuginfo"`, default `lto = false`, `codegen-units = 16`):**
  drops ~8 MB compared to unstripped (`caskade-native` 222.6 → 214.6 MB on
  Linux) while preserving readable backtraces and keeping 4-target CI release
  link times fast (~2–4 min link step per target). Compressed installers
  (AppImage / `.deb` / NSIS / `.dmg`) shrink the ELF/PE/Mach-O by ~65–70%.
- **Full symbol stripping (`strip = "symbols"`):** saves ~64 MB uncompressed on
  Linux (~150 MB binary), but removes symbol names from in-app panic logs unless
  external split-debuginfo symbolication (`.dwp`/`.pdb`/`.dSYM` archived per
  release + offline symbolication workflow) is maintained. Because Caskade has
  no crash-reporting telemetry service and relies on user-shared local logs
  (**Settings → About → Open logs folder**), keeping symbol names inline is
  currently preferred over a crash-symbol server.
- **Thin LTO (`lto = "thin"`):** eliminates dead generic instantiations across
  `stylo`, `wasmtime`, `iroh`, and `vello` (typically 15–25% smaller text
  sections with modest ~1.3–1.8× link-time overhead and good runtime speed).
  **Fat LTO + `codegen-units = 1`** squeezes slightly further (~5–10% beyond
  thin LTO) at the cost of much higher CI runner RAM and multi-minute single-threaded
  linking, which risks OOM or timeouts on macOS/Windows GitHub runners.
- **Recommendation:** keep `strip = "debuginfo"` until an external
  symbolication workflow is warranted; evaluate enabling `lto = "thin"` on
  a dedicated release-profile PR measured across all four CI legs (Linux,
  Windows, macOS arm64/x64) to verify runner RAM and link times before changing
  the release profile.

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

# Security policy

## Supported versions

Caskade publishes `0.2.x` releases. Security fixes go to `master` and ship in
the next release. Only the latest release and `master` receive fixes, and the
built-in updater delivers updates to installed releases.

| Version                  | Supported         |
| ------------------------ | ----------------- |
| Latest release (`0.2.x`) | Yes               |
| Older releases           | No: update first  |
| `master`                 | Yes (development) |

## Reporting a vulnerability

Please report vulnerabilities privately, not in public issues or pull
requests:

- **GitHub private vulnerability reporting:** use "Report a vulnerability"
  on the [Security tab of github.com/krazyjakee/caskade](https://github.com/krazyjakee/caskade/security/advisories/new).
- **Email:** krazyjakee@gmail.com, with "Caskade security" in the subject.

Please include:

- the affected version or commit and your OS
- the steps or a proof of concept
- the impact you expect

What happens next:

- You should get an acknowledgement within 7 days.
- We will agree a disclosure date with you. By default that is when a fix
  ships, or after 90 days, whichever comes first.
- We credit reporters who want credit.

The scope covers:

- the native desktop app (Dioxus/Blitz UI in `apps/native`), standalone web
  game and video webview windows (`caskade-player`), and the core and
  application crates (`caskade-core`, `caskade-app` and workspace libraries)
- the installer and discovery libraries (`caskade-installer`, `caskade-discovery`)
- the plugin host, plugin UI renderer, add-on catalogue and theme pipeline
- device sync, one-time game shares and streaming integration
- the update feed and the release workflows in this repository

The metadata server (`caskade-server`) is closed source, but reports about
it are welcome here too.

## Security model, briefly

- **Games, emulators and installers you choose run with your user's
  rights.** Caskade doesn't sandbox them. It makes sure they run only when
  you ask. Links from web pages, dropped files and pasted URLs wait for
  your confirmation before downloading or installing, and Settings → Installer
  lets you require an extra confirmation before running any downloaded
  installer program.
- **Plugins** run in a WebAssembly sandbox (`wasmtime`), with only the
  network hosts, folders and library access you grant. From plugin API 0.2,
  plugin UI is drawn from validated JSON component trees returned by the
  sandboxed backend; plugins run no JavaScript in the native app.
- **Themes** contain no general code. The theme check refuses remote
  resources and scripts; optional backdrop shaders are pure WGSL functions
  validated and namespaced before compiling and bounded by GPU time limits.
- **Secrets** (device sync keys, store sign-ins, RomM and streaming
  credentials) are kept only in the OS keychain and are never written to the
  database, settings, logs, backups or UI.
- **App updates and releases** are cryptographically verified: the updater
  checks minisign signatures (including the signed version), macOS releases
  are Developer ID signed and Apple notarized, and Windows releases are
  Authenticode-signed.
- **Local accounts and PINs** keep a household's profiles apart. They do
  not protect against someone who can use your OS account.

## Audits

An application security audit ran in October 2026 against the earlier `0.1.0`
Tauri/React app (its report is kept in the repository's git history as
`docs/security/audit-2026-10.md`, removed from the tree in November 2026).

Since that audit:

- The retired Tauri/React webview shell and JavaScript UI plugins were
  replaced in `0.2.0` (#143) by the native Dioxus/Blitz app and sandboxed
  API 0.2 plugin UI, eliminating the webview IPC bridge and JavaScript plugin
  UI surfaces. The audit's findings were fixed or superseded by the native
  rewrite.
- Windows and macOS releases are now built, tested in CI, and code-signed
  (macOS Developer ID signing and notarization, Windows Authenticode signing),
  with unsigned build jobs separated from signing jobs in
  `.github/workflows/release.yml`.
- A comprehensive new audit has not yet been run on the `0.2.x` native
  architecture.

No audit can show that software has no vulnerabilities. Please report
anything you find.

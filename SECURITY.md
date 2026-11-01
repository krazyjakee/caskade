# Security policy

## Supported versions

Caskade has not had a stable release yet. Security fixes go to `master` and
ship in the next release. Once releases exist, only the newest one gets
fixes, and the built-in updater delivers them.

| Version        | Supported         |
| -------------- | ----------------- |
| Latest release | Yes               |
| Older releases | No: update first  |
| `master`       | Yes (development) |

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

- the desktop app (webview, Tauri adapter and core)
- the installer and discovery libraries
- the plugin host and theme pipeline
- the update feed and the release workflows in this repository

The metadata server (caskade-server) is closed source, but reports about
it are welcome here too.

## Security model, briefly

- **Games, emulators and installers you choose run with your user's
  rights.** Caskade doesn't sandbox them. It makes sure they run only when
  you ask. Links from web pages, dropped files and pasted URLs wait for
  your confirmation, and an installer program downloaded from a link asks
  again before it runs.
- **Plugin backends** run in a WebAssembly sandbox, with only the network
  hosts, folders and library access you grant.
- **Plugins with a user interface** run inside Caskade with the app's own
  access. Install them only from people you trust.
- **Themes** contain no code. The theme check refuses remote resources and
  scripts.
- **App updates** are signed, and the app refuses any update whose
  signature doesn't match.
- **Local accounts and PINs** keep a household's profiles apart. They do
  not protect against someone who can use your OS account.

## Audits

- **October 2026:** [docs/security/audit-2026-10.md](docs/security/audit-2026-10.md).
  It records the scope, method and findings, and their status.

The October 2026 audit left some risks open:

- Several low and medium findings are still being fixed and tracked. Their
  details are withheld until the fixes ship.
- Windows and macOS were reviewed in code but not tested on those systems.
  The macOS quarantine of downloads and installed games has not been run on
  macOS yet.

No audit can show that software has no vulnerabilities. Please report
anything you find.

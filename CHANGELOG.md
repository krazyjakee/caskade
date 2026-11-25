# Changelog

All notable user-visible changes to Caskade are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Caskade uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.3] - 2026-11-25

### Added

- Home hero now offers a direct **Play** action alongside **View details**, with smoothly blended wide hero artwork.
- Diagnostics tools in **Settings → About**: **Open logs folder** and **Copy system info** for bug reports.
- Games installed on paired devices are now clearly distinguished in the library.
- Release pipeline now populates in-app updater release notes from `CHANGELOG.md` and publishes `SHA256SUMS` checksums alongside release downloads.
- Repository community templates, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, and Dependabot configuration.

### Changed

- Moved the **Console mode** button out of the OS window controls cluster into the top bar actions on Windows and Linux.
- Refined the app icon, branded the Windows NSIS and macOS DMG installers, and added Linux `.desktop` and AppStream metadata.
- Improved native UI polish: translucent scrims behind game card overlay icons, cleaner empty states on game pages, Settings sidebar scrollbar gutter, radial-gradient onboarding logo halo, centred wordmark, and consistent sentence-case button labels.
- Completed Swedish (`sv`) translations and localized native OS dialogs and the macOS app menu bar.
- Finalised Privacy Policy, Terms and Conditions, Licensing notices, and `SECURITY.md`.
- Shrunk SQLite caches when hiding to the tray and improved core, service, and native UI performance across database queries, background workers, and virtualised lists.
- Updated `cookie` to 0.18.3 for RustSec advisory RUSTSEC-2026-0594.

### Fixed

- Trackpad scroll gestures now lock to their dominant axis so vertical swipes over horizontal shelves no longer drift sideways.
- Reset the outer scroll position when navigating between pages.
- Show an icon for unreachable paired devices.
- Keep Console mode prompts on controller glyphs instead of falling back to keyboard hints.
- Clean up the emulator setup cache directory before marking the setup job finished.
- Improved overlay focus handoff and settle-frame handling for keyboard and gamepad navigation.

## [0.2.2] - 2026-11-23

### Added

- Authenticode code signing for Windows release builds (installer, `caskade.exe`, uninstaller, and NSIS plugins) using Certum cloud signing.
- macOS Developer ID signing and Apple notarization with stapled tickets for `.app` and `.dmg` bundles.
- Native `overscroll-behavior: contain` support across dialogs, popups, shelves, and media strips so inner scrollers no longer scroll the page behind them.

### Changed

- Updated website screenshots to capture directly from the native app.
- Renamed **Library discovery** consistently to **Discovery** across the UI.
- Updated `wasmtime` and `url` dependencies for RustSec advisories and moved CI to Rust 1.100.

### Fixed

- Fixed a blank first frame on startup by showing the window only once its initial content is painted.
- Fixed IGDB protocol-relative cover URLs in the Store and logged image load failures.
- Fixed button hover cursors showing the arrow pointer instead of the text cursor.
- Fixed macOS discovery warnings to name the failing path in every message.
- Fixed game editor sections overflowing their dialog and named launch actions for screen readers.
- Fixed fixture mode keeping sync endpoints on loopback.

## [0.2.1] - 2026-11-16

### Changed

- Made overlay scrollbars stay visible when dimmed and added smooth eased mouse-wheel scrolling.
- Preferred the low-power GPU adapter on macOS to avoid growing dual-GPU Metal allocations, keeping GPU rendering as the default.

### Fixed

- Fixed macOS first-run setup failure recovery and restored editing after errors.
- Fixed popups clipping off screen, chamfer focus outlines, and hover states in dropdown menus.
- Fixed focusing the PIN input automatically when choosing an account.
- Fixed finding artwork for web games by exact IGDB title and capturing fetched Unity HTML from Blob frames.

## [0.2.0] - 2026-11-14

### Added

- Native Dioxus/Blitz desktop and console UI replaces the legacy Tauri/React webview shell across all platforms (Linux, Windows, and macOS).
- Automatic CPU rendering fallback when no usable GPU adapter is available.
- Version-bound updater signatures so installed copies reject signatures minted for a different version.

### Changed

- Evict unused decoded images beyond 16 MiB (LRU) and keep sync field winners as text to reduce idle memory.
- Mask password and PIN fields in rendering so secret characters are never drawn.

### Fixed

- Fixed text wrapping at its container width after an edit re-measures a layout.
- Fixed library covers flickering while pages reload.
- Fixed false unreachable-device toasts before launching a game.
- Fixed accepting the AppImage's bundled 7-Zip binary.

## [0.1.4] - 2026-11-08

### Added

- Double-clicking a game in the library launches it.
- Remove failed or cancelled jobs from the Activity view.
- Home hero indicates which paired device a game was last played on.
- Match synced games by their source device's IGDB identifier.

### Changed

- Reduced idle resource usage across discovery watchers, sync log compaction, Ludusavi manifest indexing, plugin host epoch thread, and copy stores.
- Fall back from GPU rendering in WebKit when a page fails to render on the GPU.

### Fixed

- Fixed Ready to Install counts starving library database connections and write-lock contention on read-first transactions.
- Fixed RomM prompting each device to sign in cleanly instead of showing a generic connection error and backing off for a week on missing covers.
- Fixed copying offline web games and prioritizing LAN addresses for nearby paired devices.

## [0.1.3] - 2026-11-07

### Added

- Cross-platform local-network game streaming with Windows and Linux hosts, pad support on Windows, and Guide menu access over a stream.

### Changed

- Kept streaming helper processes windowless on Windows and avoided leaking client names in firewall prompts and desktop notifications.

### Fixed

- Fixed folder browser hanging on disconnected network drives.
- Fixed console sidebar scrolling and clipped hints/labels in Swedish.
- Fixed copy stores remaining active after a closed port or endpoint.

## [0.1.2] - 2026-11-03

### Added

- In-app self-updates for Debian/Ubuntu `.deb` packages via `pkexec apt-get install`.
- One-time game sharing links with QR codes, peer-assisted transfers, and sharing of saves, collections, web games, and ROMs.
- New-device setup from an existing paired Caskade device, including emulator configuration sync and LAN-first downloads.
- Remote launch ("Play on device") with Wake-on-LAN support and paired-device backups.

### Fixed

- Fixed receiving the same game or share twice corrupting the copy store.
- Fixed emulated game copies transferring ROM files rather than the entire ROM folder.

## [0.1.1] - 2026-11-02

### Changed

- Open Settings on General at each start and install downloaded games without extra confirmation by default.

### Fixed

- Fixed Windows staged installs, fallback installers, and filesystem path display formatting.
- Fixed onboarding continuing while discovery runs, skipping unresponsive Steam drives, and stopping the installer from re-asking after failed extraction.
- Fixed WebView2 process recovery preserving routes and diagnostics.

## [0.1.0] - 2026-11-01

### Added

- Initial public release of Caskade: unified game library across stores, emulators, web games, and fan ports; offline-first storage; local household accounts; device sync; theming and WebAssembly plugins; desktop and console modes.

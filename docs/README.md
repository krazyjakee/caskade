# Documentation

- [Project overview and build instructions](../README.md)
- [Technical architecture](TECH-PLAN.md): stack, domain model, plugin and theming design, and open questions. Prototype milestones provide historical context.
- [Emulation design](EMULATION-PLAN.md): definitions, scanning and launch behavior, plus pending plugin definitions and ROM importers (E5).
- [Web games](WEB-GAMES-PLAN.md): the Library's Web tab, Add web game, the game window, the mosaic, and the planned offline bundles (guided capture).
- [Game streaming](STREAMING-PLAN.md) (planned): streaming games between a household's devices through Sunshine and Moonlight, installed, configured and driven by Caskade without showing either.
- [Devices, sync, saves and sharing](sync.md): how devices find and reach each other (local network first, relays only when allowed, never for game files), pairing, what syncs and what never does, conflicts, saves and the Ludusavi manifest, copies, playing on another device and Wake-on-LAN, one-off shares, every switch in Settings → Devices, and the manual checklist.
- [Background jobs](background-jobs.md)
- [Errors and recovery](errors.md): the structured error shown across the app, the self-healing policy, guidelines for writing errors, and the audit inventory with its follow-ups.
- [Webview recovery and diagnostics](webview-recovery.md): native process failure handling, rolling logs and Windows/Linux verification.
- [Notifications](notifications.md): the feed in Activity, its read state, hourly batches of new games, and the event/category matrix.
- [System statistics](system-stats.md): Settings → Statistics, what it samples and when, and which metrics each OS has.
- [Local accounts](accounts.md): roles, the rules the core enforces, and why the PIN is a household lock, not data protection.
- [Popularity sorting](popularity.md): IGDB Visits, the daily offline cache and the metadata server contract for Library, smart collections and Store sorting.
- [Game editing](game-editing.md): the editor's patches, the fields the user controls and how automatic refreshes, merges and separation keep them.
- [Launch actions](launch-actions.md): a release's named play and tool actions, the default or ask-at-launch choice, tracking modes and what each target supports.
- [Lifecycle scripts](lifecycle-scripts.md): commands the user runs before a launch, after the game starts and after it exits, globally, per emulator profile and per game: order, failures, values as data, shells per OS and permissions.
- [Localization](localization.md): gettext PO catalogs, the Rust side's runtime and `tr!` macros, Lingui in the webview and `just i18n` extraction.
- [Words for actions](ui-wording.md): which verb a button uses, and when it ends with an ellipsis.
- [Translating Caskade](translating.md): the domains, starting and editing a language, trying it, `just i18n-check`, and writing translator comments and contexts.
- [Plugin contract](plugins/README.md), the [add-on catalogue](plugins/catalogue.md) and [publishing to it](plugins/publishing.md)
- [Theming contract](theming/README.md) and [writing a theme](theming/writing-a-theme.md)

## Implementation guides and verification

- [Build performance and CI scope](build-performance.md): incremental build settings, PR checks, nightly/opt-in packaging and timing reports.
- [Installer library and CLI](../crates/caskade-installer/README.md), [fixtures](../crates/caskade-installer/tests/fixtures/README.md), and [CI](../crates/caskade-installer/docs/ci.md)
- [Discovery CI](../crates/caskade-discovery/docs/ci.md), [hardening](../crates/caskade-discovery/tests/HARDENING.md), and [third-party notices](../crates/caskade-discovery/THIRD-PARTY.md)
- [Emulation catalogue and managed setup](../crates/caskade-sources/emulation/README.md)
- [Console hardware checklist](console-native-checklist.md)
- [Manual checks still open](manual-checks.md): the real-hardware, network and account checks left on GitHub issues, grouped by the machines they need.
- [Platform support and deferred checks](platform-support.md): what runs on Linux, Windows and macOS, how each part was verified, and what is left for native hardware and real accounts.
- [Security policy](../SECURITY.md) and the [October 2026 security audit](security/audit-2026-10.md): reporting vulnerabilities, findings and residual risks.
- [Releasing](RELEASING.md): the tag-built draft release, the updater signing key and testing an update before publishing.
- [Legal documents and notices](legal/README.md): the terms, privacy policy and licences Settings → About shows, and regenerating the third-party notices.

## Research still used by the implementation

- [Blaze and Rust build performance](research/rust-build-performance.md): tested Blaze releases, compatibility gaps, and prioritized alternatives for local compilation and CI.
- [Emulator sources](research/emulator-sources.md): download sources, asset patterns, first-run flags and BIOS requirements.
- [JavaScript backend spike](research/js-backend-spike.md): measurements behind the Rust-only backend contract for plugin API 0.1, with a reproducible guest and build script.
- [Web game capture spike](research/web-capture-spike.md): why offline web games go through a MITM proxy in the game window, and what `SharedArrayBuffer` and cross-origin isolation need on WebKitGTK, with a reproducible spike.

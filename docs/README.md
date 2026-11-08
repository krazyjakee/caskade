# Documentation

Start here:

- [Mission](MISSION.md): why Caskade exists and what it will and won't be.
- [Product spec](PRODUCT-SPEC.md): what Caskade does today, what is planned, and what was decided against.
- [Tech spec](TECH-SPEC.md): architecture, data, subsystems, errors, security, and how it is built, tested and released.
- [Design guidelines](DESIGN-GUIDELINES.md): look, layout, input, components, accessibility and writing.

Contracts for extension authors:

- [Plugins](plugins/README.md), the [add-on catalogue](plugins/catalogue.md) and [publishing to it](plugins/publishing.md). The manifest schema is [`plugin.schema.json`](plugins/plugin.schema.json); the backend contract is [`wit/caskade-plugin.wit`](../wit/caskade-plugin.wit).
- [Theming](theming/README.md) and [writing a theme](theming/writing-a-theme.md), with the generated [token](theming/tokens.md) and [anatomy](theming/anatomy.md) references.

Guides:

- [Releasing](RELEASING.md): cutting a release, the updater signing key, testing an update before publishing.
- [Translating Caskade](translating.md): languages, catalogs and `just i18n-check`.
- [Manual checks](manual-checks.md): the real-machine, network and account checks still open on GitHub issues.
- [Legal documents](legal/README.md): the terms, privacy policy and licences shown in Settings → About.
- [Security policy](../SECURITY.md).

Crate guides live beside their code: [installer](../crates/caskade-installer/README.md), [discovery](../crates/caskade-discovery/docs/ci.md), [emulator definitions](../crates/caskade-sources/emulation/README.md) and their [sources](../crates/caskade-sources/emulation/SOURCES.md), and module READMEs under `crates/caskade-core/src/`.

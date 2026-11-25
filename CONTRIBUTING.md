# Contributing to Caskade

Thank you for your interest in contributing to Caskade! By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

If you discover a security vulnerability, please do not open a public issue or pull request — follow the private reporting process in [SECURITY.md](SECURITY.md).

## Before you start

Read the project documentation before making substantial changes so your work fits Caskade's direction and architecture:

- [AGENTS.md](AGENTS.md): engineering rules, workspace conventions, generated files, and invariants enforced by tests and reviews.
- [docs/MISSION.md](docs/MISSION.md): what Caskade is for and what it intentionally avoids.
- [docs/PRODUCT-SPEC.md](docs/PRODUCT-SPEC.md): what Caskade does today and what is planned.
- [docs/TECH-SPEC.md](docs/TECH-SPEC.md): architecture, data model, subsystems, errors, security, build, and test design.
- [docs/DESIGN-GUIDELINES.md](docs/DESIGN-GUIDELINES.md): visual system, input/gamepad navigation, components, and UI wording.
- [docs/translating.md](docs/translating.md): adding or updating translations.
- [docs/README.md](docs/README.md): full documentation index (plugins, themes, releasing, and crate guides).

## Ways to contribute

- **Bug reports & feature requests:** open an issue using the [issue templates](https://github.com/krazyjakee/caskade/issues/new/choose), or start a conversation in [GitHub Discussions](https://github.com/krazyjakee/caskade/discussions).
- **Translations:** Caskade uses gettext PO files in `locales/`. See [docs/translating.md](docs/translating.md) to start a language or review existing translations.
- **Themes and plugins:** see [docs/theming/README.md](docs/theming/README.md) and [docs/plugins/README.md](docs/plugins/README.md).
- **Code & docs:** pick an open issue or discuss a larger change before writing code.

## Development setup

You will need:

- **Rust** 1.94 or newer (CI pins Rust `1.100.0` with `clippy`, `rustfmt`, and the `wasm32-wasip2` target for plugin host tests).
- **Node.js** with **pnpm 9** (for the stylesheet, token compiler, website, and plugin scaffolder tooling).
- **`just`** (task runner for the recipes in [`justfile`](justfile)).
- **Python 3** (used for bundling, CI scope checks, legal notices, and fetching the pinned 7-Zip sidecar).
- **GNU gettext** (`msgfmt`) for translation checks (`just i18n-check`).
- **Linux system libraries:** GTK, WebKitGTK, `libudev-dev`, and `libasound2-dev` (plus `Xvfb`, D-Bus, and a Vulkan driver such as lavapipe for window tests).

Install JavaScript dependencies once from the repository root:

```sh
pnpm install
```

## Common commands

See [AGENTS.md](AGENTS.md) and [`justfile`](justfile) for the full list of commands. On Linux, `just` recipes serialize Cargo invocations with `flock /tmp/caskade-heavy.lock`.

| Task                                  | Command                                                            |
| ------------------------------------- | ------------------------------------------------------------------ |
| Build and run the native app          | `just dev`                                                         |
| Build without running                 | `cargo build`                                                      |
| Run app Rust + JS tests               | `just test-app`                                                    |
| Run native UI & screenshot tests      | `just test-native`                                                 |
| Run native window / console E2E tests | `just test-native-window`                                          |
| Run installer / discovery tests       | `just test-installer`, `just test-discovery`                       |
| Run plugin host / theme tests         | `cargo test -p caskade-plugin-host`, `cargo test -p caskade-theme` |
| Lint Rust (matches CI)                | `cargo +1.100.0 clippy --workspace --all-targets -- -D warnings`   |
| Typecheck & lint JS tooling           | `pnpm typecheck` and `pnpm lint`                                   |
| Rebuild native stylesheet             | `just native-css`                                                  |
| Update / check translations           | `just i18n` and `just i18n-check`                                  |

## Rules and expectations for pull requests

1. **Keep specs in sync:** any change that alters what Caskade does or how a subsystem works must update `docs/PRODUCT-SPEC.md` or `docs/TECH-SPEC.md` in the same pull request.
2. **Keep generated files current:** if your change touches UI classes/styles (`just native-css`), translatable strings (`just i18n`), design tokens/anatomy, legal notices, or plugin/backdrop schemas, regenerate and commit them (see [AGENTS.md](AGENTS.md)).
3. **Format only files you changed:** run `rustfmt <file>` or `biome format <file>` on your edits rather than reformatting unrelated files.
4. **Respect architectural boundaries:** keep `caskade-core` and `caskade-app` independent of UI toolkits; route UI file/process/network access through application-layer commands; keep secrets exclusively in the OS keychain.
5. **Do not hit the network or the host desktop in tests:** use fixtures, fakes, or `wiremock` for unit/integration tests, and `PrivateDesktop` / `scripts/private-desktop.sh` when launching the app outside a headless test.
6. **Do not update pinned networking crates:** never run `cargo update` on `iroh`, `iroh-base`, `iroh-relay`, `iroh-dns`, or `noq` — `Cargo.lock` pins them together.
7. **Write clear PR descriptions:** explain what changed and why, link the relevant issue, and note which checks you ran.

## Licensing

By contributing to Caskade, you agree that your contributions will be licensed under the [MIT License](LICENSE).

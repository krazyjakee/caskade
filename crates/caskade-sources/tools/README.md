# Helper tools

Programs Caskade installs and manages for its own features, never shown to
people by name: `sunshine.toml` (the streaming
host) and `moonlight.toml` (the streaming client). `caskade_sources::tools`
embeds them and `caskade_core::managed::ManagedTools` installs them.

A definition has:

- `id`, `name` (for logs), and `dir`: its folder under Caskade's data folder.
- `[detect.<os>]`: the emulator definitions' detect rules, used to find the
  program in what a release unpacks.
- `[[install.<os>]]`: sources in order of preference. Each is an emulator
  install source (`github` with an `asset` pattern, `url` or `flatpak`)
  plus `when`: any of `x86_64`, `aarch64`, `wayland`, `x11`, `gamescope`.
  A machine takes its OS's first source whose `when` it meets (or that has
  none).

`tests/tools.rs` checks every machine's pick and every asset pattern against
the recorded file lists of the releases the plan was checked against. When
upstream renames a file, update the pattern and the recorded list.

Live checks (they reach github.com; the second downloads the releases for
this machine into a temp folder):

```sh
cargo test -p caskade-sources --features live-tests --test it tools::live_ -- --ignored --nocapture
```

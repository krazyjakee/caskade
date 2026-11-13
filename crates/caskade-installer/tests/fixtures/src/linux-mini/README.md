# Linux I2 acceptance catalogue (#40)

`cargo xtask fixtures linux-mini` rebuilds the deterministic itch ZIP and the
seven `expected/linux-mini/*.toml` catalogue entries. Other entries reuse the
canonical committed bytes and golden trees listed in `catalogue.toml`; the
`rpm-indie` name is the acceptance alias for `rpm-self-contained.rpm`.
`cargo xtask fixtures check` reads committed files without downloads or builders.

| Mini | Genuine builder | Installation / launch proof |
| --- | --- | --- |
| gog-linux-mojosetup | makeself 2.5.0 shell plus authored MojoSetup ZIP layout | CLI static extraction, start.sh --token |
| humble-linux-tar | Rust tar/gzip generator | CLI extraction, start.sh --token |
| itch-zip-with-manifest | Python zipfile, DEFLATE 9, fixed DOS epoch and Unix modes | CLI extraction; .itch.toml chooses the game over tools/Editor.x86_64; --token |
| appimage-indie | pinned official appimagetool/type2 runtime | CLI copy; real AppRun via APPIMAGE_EXTRACT_AND_RUN=1, --token |
| deb-missing-dep | dpkg-deb 1.22.6 | disposable Debian apt installs the real hello dependency, launches, removes package |
| rpm-indie | rpm-rs 0.30.2 genuine RPM v4 builder | CLI extraction; disposable Fedora DNF installs, launches, removes package |
| flatpak-bundle | flatpak 1.14.6 build-export/build-bundle | private FLATPAK_USER_DIR install/run/uninstall; local test runtime, no runtime download |

The RPM is built by the pure Rust RPM builder already pinned in
`src/builders.toml`, rather than rpmbuild (unavailable on the build host). Fedora's
real rpm/DNF validates it in the native test. No installer headers are fabricated.
The shell GOG fixture is structurally faithful, containing our own fixture-game;
it contains no proprietary MojoSetup binary and is never executed as an installer.

Run Linux payload installs, discovery, manifest verification, actual launches and
manifest uninstall (all five portable minis):

```sh
cargo test -p caskade-installer --all-features --test it linux_mini::
```

Run the disposable Debian and Fedora delegates and the isolated Flatpak sandbox
on a native runner (Docker daemon + Flatpak + bubblewrap/user namespaces required):

```sh
CASKADE_TEST_NATIVE=1 cargo test -p caskade-installer --all-features \
  --test it -- packages_delegate:: linux_delegate:: flatpak:: --include-ignored
```

Every package-manager call is made through `docker exec` in a disposable pinned
container; no host apt/dnf/pkexec is invoked. Containers are removed on success,
error or test panic. Flatpak receives private installation and XDG directories in
each child process and compares its final refs to the initial empty state.
The minimal runtime is built from host ELF dependencies in a temporary directory,
never committed, distributed, or downloaded by the tests. Native sandbox launch
requires the runner to permit bubblewrap's namespace setup.

The native nextest profile runs these ignored tests in CI's
`installer-linux-native` job (`.github/workflows/ci.yml`), which uploads JUnit
results. Steam Deck evidence must
be entered in [STEAM-DECK.md](STEAM-DECK.md); a Linux host run does not establish
Deck acceptance.

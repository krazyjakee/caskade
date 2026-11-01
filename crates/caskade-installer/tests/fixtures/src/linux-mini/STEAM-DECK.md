# Steam Deck I2 manual acceptance (#40)

Status: **PENDING — no Steam Deck hardware run has been performed.**

Record device/model, SteamOS version, Desktop/Game mode, CPU architecture,
filesystem of the selected storage root, installer commit, fixture build.lock
source hash and tester/date. Attach terminal output and native JUnit as artifacts
on issue #40; never replace a pending row with an inferred result.

Use a disposable storage folder in the user's selected Deck library (repeat the
portable cases on the removable SD-card root when available). The committed
fixtures contain only our fixture-game and authored metadata.

| Mini | Install / title / selected launch target | Actual --token launch | Verify / uninstall / cleanup | Result / evidence |
| --- | --- | --- | --- | --- |
| gog-linux-mojosetup | Extract; start.sh | token ends FixtureGame.x86_64 | manifest verifies; source retained; game absent | PENDING |
| humble-linux-tar | Extract; versioned start.sh | token ends FixtureGame.x86_64 | manifest verifies; game absent | PENDING |
| itch-zip-with-manifest | Extract; Caskade Itch Fixture; bin/FixtureGame.x86_64, not tools/Editor.x86_64 | real game token | manifest verifies; game absent | PENDING |
| appimage-indie | Copy; Indie Game; icon and AppImage target | APPIMAGE_EXTRACT_AND_RUN=1; token ends usr/bin/indie-game | manifest verifies; AppImage absent | PENDING |
| deb-missing-dep | Debian disposable container, apt installs hello | start.sh token inside container | package/payload absent; container removed | PENDING |
| rpm-indie | Fedora disposable container, DNF delegate | start.sh token inside container | package/payload absent; container removed | PENDING |
| flatpak-bundle | private user installation; Pinned delegated outcome | flatpak run in sandbox, token ends bin/fixture-game | app/runtime refs restored to initially empty set | PENDING |

1. Run `cargo xtask fixtures check` and the `linux_mini` integration test from
   README.md in Desktop mode. Save output, including all five actual launch proofs.
2. Run the native Debian/Fedora/Flatpak command in README.md when a disposable
   Docker daemon and Flatpak namespaces are available. Do not install apt/rpm
   packages on SteamOS. Record unavailable Docker or namespace support as a gap.
3. For each portable mini, install through the CLI into the chosen library,
   inspect its discovered title/icon/launch target and run the recorded target
   with `--token <temporary-path>`. AppImage uses
   `APPIMAGE_EXTRACT_AND_RUN=1` when FUSE is unavailable. Confirm the token's
   executable path, then verify and uninstall through the CLI.
4. Repeat an installed portable launch from Game mode using the same launch
   command and working directory, with stdout/stderr redirected to a temporary
   log. This tiny fixture is a command-line program; successful completion and
   its token, rather than a graphical window/controller UI, prove the launch.
5. Compare the test-owned library and Flatpak private directory states before
   and after cleanup. Retain original input fixtures. Remove only test-owned
   root identity files/directories. Attach logs and fill every row above.

The automated tests already perform steps 1 and 2 on Linux; all Deck rows remain
pending until a tester runs them on the device. Issue #40 stays open for that
literal acceptance criterion and the self-hosted native workflow (#22).

# Flatpak app bundle

`cargo xtask fixtures flatpak` uses flatpak build-export/build-bundle 1.14.6
with a fixed commit timestamp to package the actual Linux fixture-game. The
committed bundle is genuine OSTree static-delta data.

No external runtime is downloaded. Native tests invoke
`python3 xtask/flatpak-fixtures.py --runtime-output <temporary>/runtime.flatpak`.
It builds a disposable runtime from the host ELF dependencies of fixture-game;
those libraries are neither redistributed in the corpus nor installed in the
user's real Flatpak installation. All builder commands set private Flatpak/XDG
roots; native tests inject private FLATPAK_USER_DIR into install/uninstall.
The app and test runtime are explicitly uninstalled before discarding the
temporary installation. Native acceptance remains a #22 runner gate.

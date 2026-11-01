# Native uninstall mini installers

Build with `cargo xtask fixtures native-uninstall` (cached builders) or add
`--prepare` for the explicit SHA-checked downloads declared in builders.toml.
ISCC 6.0.5, NSIS 3.11, and wixl 0.103 author real installers from the existing
Unity fixture-game payload. No installer bytes are synthesized.

Each installer writes its own HKCU Software/CaskadeNativeFixture/<family>
InstallPath value. Inno and NSIS also generate an uninstaller and user shortcuts;
MSI registers ProductCode {F78B4949-77EC-441F-B1E9-9673F41E1A18} and installs a
registry component. Inno is per-user and includes desktop and Programs shortcuts.
Static golden trees describe authored payload files; native-generated files are
listed separately in each TOML. Native uninstall must remove files, owned
registry values, shortcuts, and its Uninstall registration.

These fixtures have their own `load_native_uninstall` loader; I0 and the Windows
handler matrix remain unchanged. Native Windows acceptance is gated on #22.

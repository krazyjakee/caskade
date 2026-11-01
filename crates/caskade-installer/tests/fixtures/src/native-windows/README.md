# T7 native Windows fixtures

Six real installers share two static payload goldens with extraction tests:
Inno Setup 5.6.1 and 6.0.5 carry the GOG layout; NSIS 2.51/3.11 and MSI with
embedded/external cabinets carry Unity. The existing Windows and native-uninstall
fixtures are preserved. Every package has an authored per-user Uninstall entry.

Rebuild explicitly with `cargo xtask fixtures native-windows --prepare`. Without
`--prepare`, all builders must already be in the SHA-verified cache. The script
reuses `src/windows/builders.json` and the existing payload archives. The namespace
is limited to 4.5 MB, each binary to 3 MB, and the full corpus to 80 MB. Current
six installers/cabinets total 2,927,842 bytes before any subsequent rebuild.

Shortcut opt-outs are explicit: Inno uses `/NOICONS` and an authored
`Check: not WizardNoIcons` on its Start-menu item; NSIS supports the package flag
`/CASKADE_NO_SHORTCUTS=1` before the final unquoted `/D=`; MSI authors the public
property `CASKADE_NOSHORTCUTS=1`. The latter two are fixture recipes, not generic
NSIS/MSI switches. Installers retain shortcut creation when those opt-outs are
absent, allowing the tests to detect ineffective arguments.

The WiX source is retained per MSI. Because wixl 0.103 rejects the standard
Component/Condition child, `shortcut-condition.sql` applies that authored row via
`msibuild` from the same SHA-pinned msitools distribution. No MSI bytes or parser
are hand-crafted. Product/component GUIDs are deterministic per fixture/path.

The same `gog.tree.json` or `unity.tree.json` is compared for real native installs
and static extraction. Only explicitly listed generated uninstaller files are
excluded from the native tree. Registry and shortcut assertions are separate.
Linux Wine runs are pre-evidence; native Windows and interactive OS command-line
verification remain #22 runner gates. `checked_versions` stays unverified until
native evidence is reviewed.

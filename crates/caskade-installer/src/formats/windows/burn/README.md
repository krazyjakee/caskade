# Windows fallback handlers

`BurnHandler`, `InstallShieldHandler`, `WiseHandler` (including Setup Factory),
and `GenericHandler` propose typed plans and use injected host/runtime capabilities.
The generic handler requires an identified PE **installer** role; ordinary games,
HTML renamed to .exe, uninstallers and SFX archives do not become wizards.

Burn uses the documented `/quiet /norestart` and authored `InstallFolder` variable.
The genuine fixture authors that variable as overridable and persisted, and passes it to the embedded MSI's
INSTALLDIR/TARGETDIR properties. Other bundles may use different variables; preset
warnings remain visible. Language, shortcut and finish-action switches are never
invented. Interactive modes use only accepted family presets.

Burn v2 static extraction reads the bounded PE header through the shared aligned
`pelite` wrapper, validates the documented `.wixburn` section and container spans,
reads at most 2 MiB of manifest XML from the UX cabinet, and opens the attached CAB
with the existing Rust cabinet reader. Payload paths, counts, sizes and SHA1 are
validated; temporary files go through `SafeDir` below a private staging scratch.
MSI payloads then use the existing safe `MsiExtractor`. Bundle bootstrapper code,
prerequisites, scripts, custom actions and registration are skipped (Partial).
External payloads, unknown table versions and layouts with more attached containers
are conservatively unsupported; execution candidates remain separate fallbacks.

InstallShield's generic marker is insufficient to select a silent contract.
Only `installshield.variant = "basic_msi"`, supplied by an identifying adapter or
trusted input metadata, enables the table's `/s` plus single nested `/v` MSI
argument. Unknown InstallScript/legacy variants stay interactive. No response file
or unverified `/S` switch is invented for Wise or Setup Factory. Custom installers
receive no flags. Every such fallback records a machine-readable warning.

Where an injected pinned 7-Zip is available, a PE overlay gets a safe, separate
extraction attempt before the interactive fallback. Each failed candidate rolls
back before the next; no executable is run merely to list or unpack it.

The original fixture is built by `xtask/burn-fixtures.py` with SHA-pinned official
WiX 3.14.1 and Wine Mono 9.4.0. Compiler/runtime downloads remain under ignored
`target/`. Its attached payload is the existing genuine embedded-cab MSI and
fixture-game binary. `tests/windows_fallback.rs` covers exact extraction trees,
three host profiles, policy, argv, variant gating, unknown warnings and hostile
metadata. The native Windows silent/actual launch test is an ignored T7 profile.
`windows_fallback_support/burn_wine.py` supplies isolated Wine pre-evidence only.

Format references:
- https://github.com/wixtoolset/wix3/blob/wix3141rtm/src/tools/wix/BurnCommon.cs
- https://github.com/wixtoolset/wix3/blob/wix3141rtm/src/tools/wix/BurnReader.cs
- https://docs.firegiant.com/wix3/bundle/
- https://docs.revenera.com/installshield/helplibrary/IHelpSetup_EXECmdLine.htm

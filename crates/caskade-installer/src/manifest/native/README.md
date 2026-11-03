# Native uninstall

`Uninstaller` prefers a recorded or uniquely detected Inno/NSIS/MSI uninstaller.
Arguments are rebuilt through `plan::presets::build_uninstall_args`; registry
command strings are identification data and are never executed verbatim.
File programs must occur in the manifest inventory, match size and optional
SHA-256, be regular non-symlink MZ files, and remain inside the game capability.
MSI ProductCodes are validated GUIDs. Native failures and timeouts fall back to
manifest cleanup and remain visible in `UninstallReport.native`. Cancellation
retains the game. Arbitrary unsupported uninstall DSLs retain their existing
explicit-error semantics.

Native registry inverse effects are recorded as
`ExternalEffect::Registry { effect: RegistryEffect::Value/Key }`, preserving
original values when a pre-existing value was changed. On Windows the
pipeline's `runtime::RegistryWatcher` journals them around each native run:
keys that appear one or two levels below `Software` (outside `Microsoft`,
`Classes` and `Policies`) are recorded with their subkeys and every value's
data as installed, so the manifest route can remove them after a move that
leaves the native uninstaller unsafe (#38). Value reversal restores the old
typed value or deletes a created value, but keeps a value whose data changed
since the install; a created key is removed only when empty. The default
Windows adapter allows only explicit HKCU/HKLM 32/64 Software scopes. Other
hosts inject a `NativeRegistry` for the Wine/Proton prefix; the host registry
is never substituted for a prefix registry.

After a native install, call `uninstall_registration_effects(game, registry)`
and append the returned effects to the manifest. This stores the exact logical
Uninstall key and an identity hash, without persisting absolute game paths.
It supports manifest cleanup after `Rewritable`/`Fragile` relocation, when the
native uninstall log may refer to a stale location. Changed registrations are
preserved and reported for retry. Stable alias targets are canonicalized for
ownership checks. The Windows adapter rechecks an owned registration before
removing that exact Uninstall subtree; unrelated registrations remain intact.

Uninstall cleanup runs under the executor's root exclusion through native steps,
recorded out-of-root and registry effects, alias cleanup, and tree removal.
Native Windows acceptance is `tests/uninstall_native/windows.rs`, opt-in via
`CASKADE_TEST_NATIVE=1` on a dedicated clean runner. It installs each genuine
native-uninstall fixture, checks files and registry/Uninstall registration,
then requires a successful native attempt and clean managed root. Linux tests
use injected runtime/registry seams; actual Wine installer checks additionally
verified all three authored fixture uninstallers remove files and both their
custom registry and Uninstall keys. Native runner results remain gated on #22.

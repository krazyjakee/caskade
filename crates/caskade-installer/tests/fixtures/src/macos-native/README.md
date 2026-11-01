# macOS native fixture recipes (#44)

Run `python3 xtask/macos-fixtures.py --artifact target/macos-fixture-artifact`
on the macOS runner. Linux/Windows support only `--check` source validation.
The builder compiles the real fixture-game for the runner architecture, creates
a native .app with framework symlinks, signs it ad hoc, and uses Apple's ditto,
hdiutil (UDZO/ULFO/real SLA resource), pkgbuild and productbuild. DMG images and
native packages cannot be manufactured or validated by this Linux checkout.

The native jobs preserve generated files, expected trees, SHA256 inventory and
tool versions as a runner artifact. PR native tests consume that artifact through
CASKADE_MACOS_FIXTURES. Committed fixture staleness/size checks run before generation
and are not re-blessed by a PR job. The trusted installer-fixtures workflow can
propose genuine generated bin/macos-native + expected/macos-native for review;
all additions must still pass the global 80 MB committed corpus budget.

Native managed tests install -> verify tree/modes/framework links -> launch the
real Mach-O fixture --token -> write/verify manifest -> uninstall -> clean. Native
system packages use sudo installer only on the disposable runner and clean only
their own directory and receipt. Quarantine preservation is checked separately
from executing unsigned quarantined fixture code. No notarization is claimed.

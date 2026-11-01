# D2 integration evidence

`cargo test -p caskade-discovery --all-features --test it windows_integration:: -- --nocapture`
loads only the committed synthetic `windows-integration` machine. Its authored
`expected.json` names every logical game independently of discovery. The fixture
combines all D2 launchers with GameUX, Start Menu and the 100-record uninstall
corpus. Games repeated through launcher protocols and local executable paths
must merge, retaining alternatives. Hollow Knight has three independent records:
uninstall, GameUX and a Start Menu link. Xbox is installed on the second drive.

The precision test prints aggregate JSON. Of the 100 labelled public-product
shapes, 24 are games, 60 are applications, eight are hidden records and eight are
launcher-routing aliases. The standalone classifier confusion matrix covers the
92 non-alias records. The separate composite verifies launcher alias merging;
an orphan alias must never create a standalone group. Both precision and recall
must reach 95%, and exact expected counts are asserted. Discovery CI exposes
successful nextest output so the matrix is retained in CI logs.

The ignored `windows_integration_native` test is selected automatically by the
trusted Windows `discovery-live` workflow with `CASKADE_DISCOVERY_LIVE=1` and
`CASKADE_TEST_NATIVE=1`. It creates unique temporary HKCU uninstall keys in both
registry views, scans the real registry, verifies both original fixture games
appear once, removes its own keys with RAII cleanup, and verifies deletion. It
never executes an installer or uninstaller, never modifies existing keys and
prints only aggregate metrics. Native Windows execution is tracked on #71;
Linux tests and cross-compilation cover the committed fixture and native-test
code respectively.

All source descriptions are original and reviewable; marker files are inert
text. No real user paths, registry exports, credentials or game files are used.
Regenerate with `cargo xtask discovery build`; verify without builders using
`cargo xtask discovery check`.

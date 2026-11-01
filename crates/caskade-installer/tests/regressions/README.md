# Parser regression inputs

Preserve minimized robustness-test inputs here, with an issue number and a
short description. Pair every input with a deterministic T5 test before its
parser repair. Record the affected target, expected behavior and reproducer in
the issue. Use TestRoot sentinel checks whenever the test touches a filesystem.

No artificial report inputs are added merely to populate this directory.
See ../../fuzz/README.md for the reduction and owner-routing procedure.

EA app and Origin discovery reads publisher registry Install Dir values (both
registry views), each game's `__Installer/installerdata.xml`, and form-encoded
`ProgramData/Origin/LocalContent/**/*.mfst`. The same source works against an
injected Wine registry and a fixture HostEnv; it never falls back to this
machine's folders. A bounded Program Files/EA Games XML search covers installs
whose publisher key is missing.

XML supplies content IDs, a preferred English title and confined executable
tracking hints. Origin manifests supply offer IDs, install state and size. IDs
are retained as aliases; missing folders and incomplete installs stay visible
with conservative confidence. DLC manifests are skipped. Malformed records
produce warnings while other games remain discoverable. Registry/XML/manifest
parsing, directory walks and file sizes have explicit bounds; DTDs are rejected.

EA Desktop's hardware-bound encrypted IS file is an optional stretch item and
is deliberately not decrypted. No account tokens, authentication, network
requests, subprocesses or launcher invocation are involved. Native Windows
parity with current EA/Origin releases is tracked on #71; Linux tests use
hand-authored synthetic records with two launcher generations.

This is an original implementation. Format references (no code ported):

- BoilR, Apache-2.0: https://github.com/PhilipK/BoilR/blob/main/src/platforms/origin/origin_platform.rs
- Origin MFST layout and duplicate empty/nonempty dipInstallPath fields: https://github.com/Nexus-Mods/Vortex/wiki/MODDINGWIKI-Developers-General-Game-detection
- Caskade discovery plan §6: publisher registry, installerdata.xml contentIDs,
  LocalContent and EA/Origin URI contracts.

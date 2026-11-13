# Alias spike (#31, stable install aliases)

Question: do native installers accept the stable alias as their install
directory, and do they record only the alias, never the real game path?

## Method

`docs/alias-spike.py <fixtures/bin/windows>` creates, per case, a real
directory and an alias symlink to it. It then runs the genuine #34 fixture
under wine 9.0 (fresh prefix, `xvfb-run`) with the alias as the install
directory. Afterwards it searches every installed file, every `.lnk`/`.ini`
in the prefix and the prefix registry (`*.reg`) for the real path and for the
alias path, as `/` and `\` forms and `Z:` paths, in UTF-8 and UTF-16LE, plus
registry-escaped.

| Case | Command | Exit | Real-path references | Alias references |
|---|---|---|---|---|
| `inno60-ini` | `setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /CURRENTUSER /DIR=<alias>` | 0 | none | `config.ini` `InstallPath` |
| `inno60-registry` | same, `/ALLUSERS` | 0 | none | HKLM `Software\CaskadeFixture\FixtureGame\InstallPath` (`system.reg`) |
| `nsis3-unicode-nonsolid` | `setup.exe /S /D=<alias>` | 0 | none | `config.ini`, Start-menu `Fixture Game.lnk` (UTF-8 and UTF-16LE target) |
| `msi-embedded` | `msiexec /i setup.msi /qn INSTALLDIR=<alias>\ TARGETDIR=<alias>\` | 0 | none | MSI component/install records (`system.reg`) |

All four installed their complete payload into the real directory through the
alias, and every recorded path is the alias. This is the property layer 2
relies on: moving the game later is a retarget, with no rewrites.

## What this does not prove (native Windows, #22 runner)

- Under wine the alias is a Unix symlink that wine presents as an ordinary
  directory. It is not an NTFS junction, so an installer that resolves reparse
  points (`GetFinalPathNameByHandle`) is not exercised here. The native check
  is the same fixtures with `AliasStore` junctions on the Windows runner.
- Junctions into exFAT or other local volumes. Junction targets must be local
  NTFS-addressable paths; exFAT roots need the runner's second volume (#22).
- Network-share roots: `supports_alias` refuses UNC targets (`\\server\…`,
  `\\?\UNC\…`), so those games get no alias and fall back to the reference scan
  (`Rewritable`/`Fragile`).

## Implementation notes

- `runtime::alias::AliasStore`: `<base>/<short-id>`, where base is
  `%LOCALAPPDATA%\Caskade\g`, `$XDG_DATA_HOME/caskade/g` or
  `~/Library/Application Support/Caskade/g`. The short ID is 12 hex digits
  of SHA-256 of a stable game key. `link` creates or retargets, `target`
  reads, and `remove` deletes only the link and refuses real directories.
- Windows uses the `junction` crate (safe API; no admin rights, no `unsafe` in
  this crate).
- Executor orchestration (alias → staging for `Run` steps, `INSTALL_DIR` →
  alias, retarget to the final directory on commit), the manifest alias ID and
  uninstall removal are wired through the executor, manifest and uninstall
  seams of their owners. (The per-package recipes and their `disable_alias`
  flag were removed; a caller that wants no alias omits it.)

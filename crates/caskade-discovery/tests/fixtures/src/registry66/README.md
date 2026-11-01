# Fresh-prefix registry samples (#66)

These four small files were produced on 27 October 2026 by **Wine 9.0
(Ubuntu 9.0~repack-4build3)** and **UMU-Proton-10.0-4**, invoked with
**umu-launcher 1.4.4**. They are actual Wine-format system.reg/user.reg records
from newly created temporary prefixes, populated exclusively with original
CaskadeDiscoveryFixture keys by generate.py. No existing prefix, launcher
installation, account, save, or developer registry was used.

The generator imports six authored types (SZ, EXPAND_SZ, MULTI_SZ, DWORD,
QWORD, BINARY), a Unicode path and empty subkeys into HKLM/HKCU. The engines
supply their own serialization and view redirection. In this Wine build,
regedit is 32-bit and HKLM records appear in Wow6432Node; Proton writes both
requested key paths. Tests use explicit views and compare the same typed data.

Only sections whose keys contain CaskadeDiscoveryFixture are exported. Section
timestamps and #time are replaced with fixed synthetic values. The authored
user path is "C:\Users\fixture"; no actual user names, SIDs, account identifiers,
Windows defaults, hardware or other prefix contents enter these files.

To regenerate using cached local engines, hold /tmp/caskade-heavy.lock and run:

```sh
python3 generate.py --output /tmp/registry66-output \
  --umu /path/to/umu-run --proton /path/to/UMU-Proton-10.0-4
```

Review the four resulting .reg files before replacing their captured source
copies here. Ordinary cargo xtask discovery build copies these reviewed files
into formats/registry66 and records their hashes, without requiring Wine,
Proton, umu, a display, network access or subprocess execution at test time.
The format reader is original code shared with caskade-installer; no Wine
implementation code or comments were copied.

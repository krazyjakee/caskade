These are genuine RAR5 archives made once with RAR for Linux x64 7.23,
downloaded from the [official RAR downloads page](https://www.rarlab.com/download.htm)
([builder tarball](https://www.rarlab.com/rar/rarlinux-x64-723.tar.gz)).
The builder is not redistributed or required to run tests.

Payloads came from `src/layouts/{unity,unreal,gog}` plus the size-optimized
Windows fixture-game build at initial creation. Their SHA-256 golden trees
are committed in `expected/`. They remain fixed even if today's game source
or layout changes; `cargo xtask fixtures` never rewrites them or their goldens.

Creation commands (from each populated layout directory):

```sh
rar a -cfg- -r -m5 -mt1 -tsm- unity-rar5.rar .
rar a -cfg- -r -m5 -mt1 -tsm- unreal-rar5.rar .
rar a -cfg- -r -m5 -mt1 -tsm- -v32k rar-multivolume-portable.rar .
```

The GOG-style payload produces twelve 32 KiB volumes, named `.part01.rar`
through `.part12.rar`. All parts are required. RAR4 and RAR SFX are not in
this initial archive corpus; no renamed ZIPs or fabricated RAR headers are used.


## Windows RAR SFX (#34 / #29)

`rar-sfx.exe` is an immutable genuine RAR 5 archive created with official Linux
RAR 7.23 and the unmodified `Default.SFX` extracted from official
https://www.rarlab.com/rar/winrar-x64-723.exe (SHA-256
`8ff0daf3ed564cc743c0e23ff2e253997ffc74460f9673f0b6dd037b2db4ce7b`).
The Default.SFX SHA-256 is
`94db9ebc8086ea19ca2334f5c90ccedbee25d0b53f3b5f69103267e25abde87d`.
RAR was run once during its evaluation period; this is not a rebuild dependency.
The source payload was the regenerated I0 Unity ZIP with the corrected registry
query flags. Creation command from the unpacked Unity directory:

```
rar a -r -idq -sfx/path/to/Default.SFX /output/rar-sfx.exe .
```

The recursive flag is required. Official pinned 7zz 26.05 extraction matches
`expected/windows/rar-sfx.tree.json`; no SFX byte signatures were fabricated.
The regular three handmade RAR archives above remain unchanged.

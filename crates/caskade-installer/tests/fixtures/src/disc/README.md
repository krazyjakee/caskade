# Disc fixtures

`cargo xtask fixtures disc` invokes `xtask/disc-fixtures.py` using genisoimage 1.1.11. The PC ISO and UDF/ISO hybrid contain the committed genuine Inno 6.0 basic setup.exe and autorun.inf. Its golden tree describes the nested installer payload.

The PS1-style image is a genuine ISO9660 filesystem containing SYSTEM.CNF and an original inert PS-X EXE test header/payload. The CUE accurately declares its cooked MODE1/2048 track; it is a classification fixture, not a playable licensed game or a raw-sector dump. No copyrighted console firmware, game data, or license sectors are included.

Runtime tests read committed images with the pinned official 7zz. They never need genisoimage or Wine. Native insert-next-disc requests use interactive fallback with all source discs retained; automatic mounting is deliberately deferred per the plan's open question 4.

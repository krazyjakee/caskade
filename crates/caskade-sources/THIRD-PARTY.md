# Third-party notices

## Playnite emulation definitions (MIT)

The `emulation/platform-overrides.toml` platform names, IGDB/libretro identifiers and
Playnite ids, and Windows executable patterns, ROM extensions and command
profiles in the ten `emulation/<emulator>.toml` files are adapted from
[Playnite](https://github.com/JosefNemec/Playnite), commit
`3e8abb241873e326e27c75b59dd0d2ea367698e3`, fetched 2026-10-27.
Source paths are recorded in each file header. Caskade supplies its own ids,
TOML representation, ordered subset of profiles, per-OS/install-kind variables,
standard location rules, Linux/macOS rules and argv tokenization. No emulator
binaries, cores, BIOS or ROMs are included.

The complete Playnite platform list's factual names, ids and libretro database
names are recorded in `emulation/platform-sources.json` and used to generate
`emulation/platforms.toml`. The generated file retains the Caskade overrides.

MIT License

Copyright (c) 2020 Josef Nemec

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Factual BIOS metadata

File names and hashes are reference metadata, not BIOS bytes. Sources:
[Libretro BIOS reference hashes](https://github.com/libretro/libretro-database/blob/master/dat/System.dat),
[Libretro core information](https://github.com/libretro/libretro-core-info),
[DeSmuME BIOS documentation](https://docs.libretro.com/library/desmume/),
[Beetle Saturn documentation](https://docs.libretro.com/library/beetle_saturn/),
[mGBA documentation](https://docs.libretro.com/library/mgba/), and
[Flycast documentation](https://docs.libretro.com/library/flycast/).

## Factual RomM platform metadata

`emulation/platform-sources.json` also records platform names, RomM slugs and
IGDB identifiers from RomM's public platform lists, and
`scripts/platform-catalog.py` generates Caskade definitions from those facts.
No RomM implementation code is included or executed. Pinned sources:

- [RomM platform slugs](https://github.com/rommapp/romm/blob/983ee142fbf963f00c7767f9b7888713915c8f24/backend/utils/platform_slugs.py).
- [RomM IGDB platform metadata](https://github.com/rommapp/romm/blob/983ee142fbf963f00c7767f9b7888713915c8f24/backend/adapters/services/igdb.py).
- [RomM's supported platform names](https://github.com/rommapp/docs/blob/262f98248f577a2a49caa00e32669c283fdabf80/docs/resources/snippets/supported-platforms.md).

The exact source URLs are retained in the JSON snapshot. Platform names and
identifiers are reference metadata, not emulator binaries, firmware or ROMs.

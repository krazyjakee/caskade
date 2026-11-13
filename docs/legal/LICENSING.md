# Licenses

Revision: 2026-11-13

## Caskade

Caskade is released under the MIT License (the repository's `LICENSE` file):

> Copyright (c) 2026 Jake Cattrall
>
> Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

Caskade's own code, its built-in themes (apart from their fonts), its app icon and its platform artwork are the project's work. Everything else Caskade ships is listed below and under Open-source packages, with the licence each comes under.

## Components bundled with every build

- **7-Zip** (the `7zz` program; Windows builds also ship `7z.dll`), version pinned in `scripts/7zz-pins.json`. Caskade runs it to unpack archives and installers. It comes under the GNU LGPL with BSD 3-clause parts and the unRAR licence restriction; its full licence text is below, and every installed copy also has it as `licenses/7zip-License.txt`. Source: [7-Zip releases](https://github.com/ip7z/7zip/releases).
- **Fonts**: Oxanium, JetBrains Mono and Inter, and the fonts of the built-in themes, under the SIL Open Font License 1.1. They are listed with their copyright holders under Open-source packages.
- **Rust crates** compiled into the Caskade program, and the **Lucide icons** its interface draws, listed under Open-source packages with their licence texts.
- **Emulator definitions and platform catalogue**: some platform names, identifiers, executable patterns, ROM extensions and command profiles are adapted from [Playnite](https://github.com/JosefNemec/Playnite) (MIT License, Copyright (c) 2020 Josef Nemec; the MIT text is under Open-source packages). BIOS file names and hashes are factual metadata from libretro's databases and documentation, and platform slugs and IGDB identifiers come from RomM's public platform lists. No emulator binaries, cores, BIOS files or ROMs are included. Details: `crates/caskade-sources/THIRD-PARTY.md`.

## Platform-specific components

- **Linux AppImage**: the AppImage carries copies of the system libraries Caskade needs, including GTK 3, WebKitGTK 4.1 and libayatana-appindicator3, with their dependencies, taken from the Ubuntu 22.04 build machine. These are mostly under the GNU LGPL 2.1 or later; their sources are available from Ubuntu's archive.
- **Windows installer**: built with NSIS. Caskade uses Microsoft Edge WebView2, which Windows provides; if it is missing, the installer fetches Microsoft's WebView2 bootstrapper, which comes under Microsoft's own terms.
- **macOS**: Caskade uses the system's WebKit and ships no extra native libraries.

> **Missing:** the notices of the libraries in the AppImage are not collected into this view yet. The Debian package uses the system's own copies of these libraries and ships none of them.

## Software Caskade fetches at your request

Emulators, RetroArch cores, umu-launcher (and the Steam Runtime and Proton that umu downloads on Linux) and games are not part of Caskade. Caskade downloads them from their publishers when you ask it to, and each comes under its own licence.

## Data and services

- **IGDB**: game metadata and artwork come from [IGDB](https://www.igdb.com/), through Caskade's metadata server.
- **Hasheous**: hash and title data comes from the [Hasheous](https://hasheous.org/) data dumps, downloaded by Caskade.

Their data remains subject to those services' own terms.

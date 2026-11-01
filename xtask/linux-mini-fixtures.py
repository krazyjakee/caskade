#!/usr/bin/env python3
"""Consolidate Linux acceptance minis and build a deterministic genuine itch ZIP.

No external builder/download is needed: the ZIP contains the committed Linux
fixture-game ELF and authored itch metadata. Other rows alias real existing
archives/installers without duplicating their bytes.
"""
import hashlib
import json
from pathlib import Path
import re
import stat
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "crates/caskade-installer/tests/fixtures"
SOURCE = FIXTURES / "src/linux-mini"
EXPECTED = FIXTURES / "expected/linux-mini"

def main():
    EXPECTED.mkdir(parents=True, exist_ok=True)
    binary = FIXTURES / "bin/linux-mini"
    binary.mkdir(parents=True, exist_ok=True)
    elf = (FIXTURES / "bin/fixture-game-linux-x86_64").read_bytes()
    if not elf.startswith(b"\x7fELF"):
        raise RuntimeError("fixture game is not a real ELF executable")
    files = {".itch.toml": ((SOURCE / "itch.toml").read_bytes(), 0o644),
             "bin/FixtureGame.x86_64": (elf, 0o755),
             "tools/Editor.x86_64": (elf, 0o755)}
    tree = [{"path": d, "kind": "directory", "size": 0, "executable": False}
            for d in ["bin", "tools"]]
    output = binary / "itch-zip-with-manifest.zip"
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, (data, mode) in sorted(files.items()):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data, compresslevel=9)
            tree.append({"path": name, "kind": "file", "size": len(data),
                         "sha256": hashlib.sha256(data).hexdigest(),
                         "executable": bool(mode & 0o111)})
    (EXPECTED / "itch-zip-with-manifest.tree.json").write_text(
        json.dumps(sorted(tree, key=lambda e: e["path"]), indent=2) + "\n")
    lines = ['schema_version = 1', 'id = "itch-zip-with-manifest"',
             'title = "Caskade Itch Fixture"', 'layout = "itch"', 'format = "zip"',
             'platform = "linux"', 'input = "bin/linux-mini/itch-zip-with-manifest.zip"',
             'parts = ["bin/linux-mini/itch-zip-with-manifest.zip"]',
             'tree = "linux-mini/itch-zip-with-manifest.tree.json"',
             'launch_target = "bin/FixtureGame.x86_64"']
    for host in ['linux', 'windows', 'macos']:
        lines += ['', f'[host.{host}]', 'outcome = "Installed"', 'strategy = "Extract"', 'runtime = "Native"']
    (EXPECTED / "itch-zip-with-manifest.toml").write_text("\n".join(lines) + "\n")
    catalogue = tomllib.loads((SOURCE / "catalogue.toml").read_text())
    for name, path in catalogue['fixtures'].items():
        if name == 'itch-zip-with-manifest':
            continue
        text = (FIXTURES / 'expected' / path).read_text()
        text = re.sub(r'^id = .*$', 'id = ' + json.dumps(name), text, count=1, flags=re.MULTILINE)
        (EXPECTED / (name + '.toml')).write_text(text)
    print(f"7 Linux minis catalogued; itch ZIP {output.stat().st_size} bytes")

if __name__ == '__main__':
    main()

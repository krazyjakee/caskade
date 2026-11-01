#!/usr/bin/env python3
"""Populate persistent writable corpora from the committed, project-owned fixtures."""
import argparse
import hashlib
from pathlib import Path
from urllib.parse import quote

HERE = Path(__file__).resolve().parent

def seed(target):
    output = HERE / "corpus" / target
    output.mkdir(parents=True, exist_ok=True)
    def put(data):
        path = output / ("seed-" + hashlib.sha256(data).hexdigest())
        if not path.exists():
            path.write_bytes(data)
    fixtures = HERE.parent / "tests" / "fixtures"
    inputs = sorted((fixtures / "bin").rglob("*")) + sorted((fixtures / "handmade").glob("*"))
    for source in inputs:
        if not source.is_file() or source.stat().st_size > 8 * 1024 * 1024:
            continue
        if target == "content_disposition":
            put(f'attachment; filename="{source.name}"'.encode())
            put(("attachment; filename*=UTF-8''" + quote(source.name)).encode())
        elif target == "pkg" and source.suffix == ".pkg":
            # Whole package plus its inflated TOC XML.
            import zlib
            data = source.read_bytes()
            put(data)
            if data[:4] == b"xar!":
                header = int.from_bytes(data[4:6], "big")
                compressed = int.from_bytes(data[8:16], "big")
                try:
                    put(zlib.decompress(data[header:header + compressed]))
                except zlib.error:
                    pass
        elif target == "appimage" and source.suffix == ".AppImage":
            # Seed the squashfs behind the runtime; the target supplies a header.
            data = source.read_bytes()
            shoff = int.from_bytes(data[0x28:0x30], "little")
            end = shoff + int.from_bytes(data[0x3A:0x3C], "little") * int.from_bytes(data[0x3C:0x3E], "little")
            squashfs = data[end:]
            put(squashfs)
            for percent in (25, 50, 75, 99):
                put(squashfs[:len(squashfs) * percent // 100])
        elif target in ("probe", "archive_listing", "pe") or (
            target == "makeself_header" and source.suffix == ".sh"
        ) or (target in ("inno", "nsis", "burn") and source.suffix == ".exe" and target in source.parent.name) or (
            target == "msi" and source.suffix == ".msi"
        ) or (target in ("deb", "rpm") and source.suffix == "." + target):
            data = source.read_bytes()
            put(data)
            for percent in (25, 50, 75, 99):
                put(data[:len(data) * percent // 100])
    if target == "dmg":
        # Metadata-only UDIF sample: a resource plist plus a koly trailer
        # pointing at it (no filesystem image is fabricated).
        import struct
        plist = (b'<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict>'
                 b'<key>resource-fork</key><dict><key>blkx</key><array/>'
                 b'<key>LPic</key><array/></dict></dict></plist>\n')
        koly = bytearray(512)
        koly[0:4] = b"koly"
        struct.pack_into(">II", koly, 4, 4, 512)
        struct.pack_into(">QQ", koly, 216, 0, len(plist))
        put(struct.pack("<Q", len(plist) + 512) + plist + bytes(koly))
        put(plist)
    if target == "content_disposition":
        for text in ["attachment; filename=../escape.zip", "attachment; filename=CON", "attachment; filename=\"semi;colon.zip\"", "attachment; filename*=UTF-8''Caf%C3%A9.zip", "attachment; filename=\"unterminated", "attachment; filename=a; filename=b", "attachment; filename*=ISO-8859-1''caf%E9.zip"]:
            put(text.encode())
    return output

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("target", choices=sorted(p.stem for p in (HERE / "fuzz_targets").glob("*.rs")))
    print(seed(parser.parse_args().target))

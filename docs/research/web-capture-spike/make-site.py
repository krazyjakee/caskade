#!/usr/bin/env python3
"""Writes the capture fixture's binary and data files into site/ and other/."""
import os, struct, zlib

here = os.path.dirname(os.path.abspath(__file__))
site, other = os.path.join(here, "site"), os.path.join(here, "other")
os.makedirs(os.path.join(site, "data"), exist_ok=True)


def png(path):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    header = struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
                + chunk(b"IDAT", zlib.compress(b"\x00\xff\x00\x00")) + chunk(b"IEND", b""))


png(os.path.join(site, "bg.png"))
png(os.path.join(site, "sprite.png"))
with open(os.path.join(site, "mod.wasm"), "wb") as f:
    f.write(b"\x00asm\x01\x00\x00\x00")
n = 800  # 0.1 s of silence, 8-bit mono, 8 kHz
with open(os.path.join(site, "sound.wav"), "wb") as f:
    f.write(b"RIFF" + struct.pack("<I", 36 + n) + b"WAVEfmt "
            + struct.pack("<IHHIIHH", 16, 1, 1, 8000, 8000, 1, 8)
            + b"data" + struct.pack("<I", n) + b"\x80" * n)
for name in ["level1", "level2", "worker", "later"]:
    with open(os.path.join(site, "data", f"{name}.json"), "w") as f:
        f.write('{"ok":true}')
with open(os.path.join(other, "frame-data.json"), "w") as f:
    f.write("{}")

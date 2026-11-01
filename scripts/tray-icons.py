#!/usr/bin/env python3
"""Writes the tray icons from the app icon (apps/desktop/src-tauri/icons/source.png).

The app icon is the glyph on a dark square, which reads as a black block in
light trays. The tray icons keep only the glyph, on a transparent background:

- tray.png: in colour, for Windows and Linux.
- tray-template.png: black, for macOS, which tints template images to match
  the menu bar.

Needs Pillow. Run from anywhere: python3 scripts/tray-icons.py
"""

from pathlib import Path

from PIL import Image

ICONS = Path(__file__).resolve().parent.parent / "apps/desktop/src-tauri/icons"
SIZE = 64
# The share of the icon left empty around the glyph.
MARGIN = 0.04
# Glyph pixels are bright; the square behind them is near black.
THRESHOLD = 100


def glyph(source: Image.Image) -> Image.Image:
    """The glyph alone, cropped and centred on a transparent square."""
    source = source.convert("RGBA")
    mask = Image.new("L", source.size)
    mask.putdata(
        [255 if a > 0 and max(r, g, b) > THRESHOLD else 0 for r, g, b, a in source.getdata()]
    )
    box = mask.getbbox()
    if box is None:
        raise SystemExit("no glyph found in source.png")
    shape = Image.new("RGBA", source.size)
    shape.paste(source, mask=mask)
    shape = shape.crop(box)
    side = round(max(shape.size) / (1 - 2 * MARGIN))
    square = Image.new("RGBA", (side, side))
    square.paste(shape, ((side - shape.width) // 2, (side - shape.height) // 2))
    return square.resize((SIZE, SIZE), Image.LANCZOS)


def template(icon: Image.Image) -> Image.Image:
    black = Image.new("RGBA", icon.size, (0, 0, 0, 0))
    black.putalpha(icon.getchannel("A"))
    return black


def main() -> None:
    icon = glyph(Image.open(ICONS / "source.png"))
    icon.save(ICONS / "tray.png", optimize=True)
    template(icon).save(ICONS / "tray-template.png", optimize=True)
    print(f"wrote {ICONS / 'tray.png'} and {ICONS / 'tray-template.png'}")


if __name__ == "__main__":
    main()

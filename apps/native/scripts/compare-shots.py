"""Compares Blitz's screenshots (`caskade-native --shots`) with WebKitGTK's
(`webkit-shots.py`) of the same pages (#90): per screen and theme, the share
of pixels that differ by more than a tolerance, the mean difference, and the
areas (a coarse grid) where they differ, plus a side-by-side image with the
differing pixels in red, for listing what's left as Blitz deviations (#85).

    python3 -I apps/native/scripts/compare-shots.py <shots folder> [--tolerance 24]

Writes <name>.compare.png beside each pair and prints a Markdown table.
"""

import argparse
import os

from PIL import Image, ImageChops

GRID = 8  # the area map: GRID x GRID cells


def compare(blitz_path, webkit_path, out_path, tolerance):
    blitz = Image.open(blitz_path).convert("RGB")
    webkit = Image.open(webkit_path).convert("RGB")
    if webkit.size != blitz.size:
        webkit = webkit.resize(blitz.size)
    diff = ImageChops.difference(blitz, webkit)
    # The largest channel difference per pixel.
    r, g, b = diff.split()
    worst = ImageChops.lighter(ImageChops.lighter(r, g), b)
    mask = worst.point(lambda value: 255 if value > tolerance else 0)
    width, height = blitz.size
    total = width * height
    differing = sum(1 for value in mask.getdata() if value)
    mean = sum(worst.getdata()) / total
    cells = []
    for row in range(GRID):
        line = ""
        for col in range(GRID):
            box = (col * width // GRID, row * height // GRID,
                   (col + 1) * width // GRID, (row + 1) * height // GRID)
            part = mask.crop(box)
            share = sum(1 for value in part.getdata() if value) / (part.width * part.height)
            line += " .:-=+*#%@"[min(9, int(share * 10))]
        cells.append(line)
    red = Image.new("RGB", blitz.size, (255, 0, 0))
    marked = Image.composite(red, webkit.convert("L").convert("RGB"), mask)
    sheet = Image.new("RGB", (width * 3, height))
    sheet.paste(blitz, (0, 0))
    sheet.paste(webkit, (width, 0))
    sheet.paste(marked, (width * 2, 0))
    sheet.save(out_path)
    return differing / total, mean, cells


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("folder")
    parser.add_argument("--tolerance", type=int, default=24)
    args = parser.parse_args()
    names = sorted(name[: -len(".webkit.png")] for name in os.listdir(args.folder)
                   if name.endswith(".webkit.png"))
    print(f"| Screen and theme | Pixels differing by more than {args.tolerance} | Mean difference |")
    print("| --- | --- | --- |")
    maps = []
    for name in names:
        share, mean, cells = compare(
            os.path.join(args.folder, f"{name}.png"),
            os.path.join(args.folder, f"{name}.webkit.png"),
            os.path.join(args.folder, f"{name}.compare.png"),
            args.tolerance,
        )
        print(f"| {name} | {share:.1%} | {mean:.1f} |")
        maps.append((name, cells))
    for name, cells in maps:
        print(f"\n{name}, where they differ (rows top to bottom, '@' all, ' ' none):")
        for line in cells:
            print(f"    |{line}|")


if __name__ == "__main__":
    main()

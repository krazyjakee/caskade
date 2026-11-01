#!/usr/bin/env python3
"""Generate the complete platform catalogue from pinned, factual source metadata.

The default is offline. --refresh downloads the pinned sources and needs PyYAML
(uv run --with PyYAML==6.0.3 scripts/platform-catalog.py --refresh).
Edit platform-overrides.toml for Caskade-specific launch/BIOS metadata.
"""

import argparse
import ast
import copy
import json
from pathlib import Path
import re
import tomllib
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "crates/caskade-sources/emulation"
ROMM = "983ee142fbf963f00c7767f9b7888713915c8f24"
DOCS = "262f98248f577a2a49caa00e32669c283fdabf80"
PLAYNITE = "3e8abb241873e326e27c75b59dd0d2ea367698e3"
SOURCES = {
    "romm_slugs": f"https://raw.githubusercontent.com/rommapp/romm/{ROMM}/backend/utils/platform_slugs.py",
    "romm_igdb": f"https://raw.githubusercontent.com/rommapp/romm/{ROMM}/backend/adapters/services/igdb.py",
    "romm_names": f"https://raw.githubusercontent.com/rommapp/docs/{DOCS}/docs/resources/snippets/supported-platforms.md",
    "playnite": f"https://raw.githubusercontent.com/JosefNemec/Playnite/{PLAYNITE}/source/Playnite/Emulation/Platforms.yaml",
}


def refresh():
    import yaml

    texts = {}
    for key, url in SOURCES.items():
        with urlopen(url, timeout=60) as response:
            texts[key] = response.read().decode("utf-8-sig")
    # Parse data literals only; never execute upstream Python.
    tree = ast.parse(texts["romm_slugs"])
    enum = next(n for n in tree.body if isinstance(n, ast.ClassDef))
    slugs = {
        n.targets[0].id: ast.literal_eval(n.value)
        for n in enum.body
        if isinstance(n, ast.Assign)
    }
    tree = ast.parse(texts["romm_igdb"])
    mapping = next(
        n.value
        for n in tree.body
        if isinstance(n, ast.AnnAssign)
        and isinstance(n.target, ast.Name)
        and n.target.id == "IGDB_PLATFORM_LIST"
    )
    igdb = {
        slugs[key.attr]: ast.literal_eval(value)
        for key, value in zip(mapping.keys, mapping.values, strict=True)
    }
    names = {}
    for line in texts["romm_names"].splitlines():
        if line.startswith("| ") and "`" in line:
            columns = line.split("|")
            names[columns[2].strip(" `")] = columns[1].strip()
    romm = []
    for slug in sorted(slugs.values()):
        meta = igdb.get(slug, {})
        row = {"slug": slug, "name": meta.get("name") or names.get(slug) or slug.replace("-", " ").title()}
        if meta.get("id"):
            row["igdb"] = meta["id"]
        if meta.get("abbreviation"):
            row["short"] = meta["abbreviation"]
        romm.append(row)
    playnite = []
    for platform in yaml.safe_load(texts["playnite"]):
        row = {"id": platform["Id"], "name": platform["Name"]}
        if platform.get("IgdbId"):
            row["igdb"] = platform["IgdbId"]
        if platform.get("Databases"):
            row["libretro"] = platform["Databases"][0]
        playnite.append(row)
    snapshot = {"sources": SOURCES, "romm": romm, "playnite": playnite}
    (DATA / "platform-sources.json").write_text(json.dumps(snapshot, ensure_ascii=False, indent=2) + "\n")


def words(value):
    # '+' distinguishes MSX2 from MSX2+.
    return re.sub(r"[^a-z0-9+]", "", value.casefold())


def generate(check=False):
    source = json.loads((DATA / "platform-sources.json").read_text())
    overrides = tomllib.loads((DATA / "platform-overrides.toml").read_text())["platform"]
    platforms = {p["id"]: copy.deepcopy(p) for p in overrides}

    def find(row, kind):
        for platform in platforms.values():
            ids = platform.get("ids", {})
            if kind == "romm" and row["slug"].replace("-", "_") == platform["id"]:
                return platform
            if kind == "playnite" and ids.get("playnite") == row["id"]:
                return platform
            if row.get("igdb") and ids.get("igdb") == row["igdb"]:
                return platform
        for platform in platforms.values():
            if words(platform["name"]) == words(row["name"]):
                return platform
        return None

    def create(row, key):
        identifier = row[key].replace("-", "_")
        if identifier in platforms:
            raise ValueError(f"platform id collision: {identifier}")
        platform = {"id": identifier, "name": row["name"], "short": row.get("short", row["name"]), "ids": {}}
        platforms[identifier] = platform
        return platform

    for row in source["playnite"]:
        # This pinned Playnite list mistakenly gives Vectrex Intellivision's
        # IGDB id (67). IGDB/RomM identify Vectrex as 70.
        if row["id"] == "vectrex":
            row = {**row, "igdb": 70}
        platform = find(row, "playnite") or create(row, "id")
        ids = platform.setdefault("ids", {})
        ids["playnite"] = row["id"]
        for key in ["igdb", "libretro"]:
            if key in row:
                ids.setdefault(key, row[key])
    for row in source["romm"]:
        platform = find(row, "romm") or create(row, "slug")
        ids = platform.setdefault("ids", {})
        ids.setdefault("romm", []).append(row["slug"])
        if "igdb" in row:
            ids.setdefault("igdb", row["igdb"])

    # RomM renamed these slugs. Keep older servers working too.
    for old, current in [("genesis-slash-megadrive", "genesis"), ("ps", "psx")]:
        platform = next(p for p in platforms.values() if current in p["ids"].get("romm", []))
        platform["ids"]["romm"].append(old)

    def value(item):
        if isinstance(item, dict):
            return "{ " + ", ".join(f"{key} = {value(val)}" for key, val in item.items()) + " }"
        if isinstance(item, list):
            return "[" + ", ".join(value(val) for val in item) + "]"
        return json.dumps(item, ensure_ascii=False)

    output = [
        "# Generated by scripts/platform-catalog.py; do not edit.",
        "# Sources: platform-sources.json (pinned factual names and identifiers).",
        "# Caskade launch metadata: platform-overrides.toml. See ../THIRD-PARTY.md.",
        "# A platform can be imported without a shipped emulator or known extensions.",
    ]
    for platform in sorted(platforms.values(), key=lambda p: p["id"]):
        output += ["", "[[platform]]"]
        output += [f"{key} = {value(val)}" for key, val in platform.items()]
    text = "\n".join(output) + "\n"
    destination = DATA / "platforms.toml"
    if check:
        if destination.read_text() != text:
            raise SystemExit("platforms.toml is stale; run python3 scripts/platform-catalog.py")
    else:
        destination.write_text(text)
    print(f"Generated {len(platforms)} platforms covering {len(source['romm'])} RomM slugs and {len(source['playnite'])} Playnite platforms.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--refresh", action="store_true")
    parser.add_argument("--check", action="store_true", help="fail if generated definitions are stale")
    args = parser.parse_args()
    if args.refresh:
        refresh()
    generate(check=args.check)

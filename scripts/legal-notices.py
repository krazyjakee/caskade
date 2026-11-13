#!/usr/bin/env python3
"""Generate the third-party notices Settings → About → Licenses shows offline.

Inventories what the app bundle ships and writes one compact JSON file the
native app embeds (apps/native/assets/notices.generated.json):

- Rust crates: `cargo metadata`, the normal (non-dev, non-build) dependency
  graph of the binary the bundle ships, `caskade-native`, for every target
  platform (a superset of what any one platform's binary links). The
  vendored crates (vendor/, patched in by the workspace's Cargo.toml) are
  among them.
- Fonts: the app's own (apps/native/assets/fonts, each with its
  LICENSE-<family>) and the built-in themes' (themes/<theme>/assets/fonts
  with its OFL.txt, for every theme crates/caskade-core/src/themes.rs
  embeds).
- Icons: lucide's path data, which apps/native/src/ui/icons.rs draws
  (apps/native/assets/LICENSE-lucide).
- 7-Zip: the upstream License.txt of each pinned sidecar package the release
  builds bundle (scripts/7zz-pins.json), verified and cached by
  scripts/7zz-download.py (the first run downloads them).

Each package keeps its licence expression, repository and the copyright lines
found in its licence files (its authors otherwise); each licence's text is kept
once, from the first package that ships it. Run it after changing dependencies
or the 7-Zip pin; see docs/legal/README.md.
"""
import argparse
import importlib.util
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "apps/native/assets/notices.generated.json"
# The binaries the bundle ships.
APP_CRATES = ("caskade-native",)
APP_FONTS = ROOT / "apps/native/assets/fonts"
# Where the built-in themes are embedded, one `builtin_theme!("<folder>": …)` each.
BUILTIN_THEMES = ROOT / "crates/caskade-core/src/themes.rs"
LUCIDE = ROOT / "apps/native/assets/LICENSE-lucide"
OFL = "SIL Open Font License, Version 1.1"
# The packages each release target bundles (release.yml's matrix).
SEVEN_ZIP_PACKAGES = {"Linux": "linux-x64", "macOS": "macos-x64", "Windows": "windows-x64"}

LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|unlicense|notice)", re.IGNORECASE)
COPYRIGHT = re.compile(r"^\s*(copyright\b|\(c\)|©).*\d{4}", re.IGNORECASE)
# A licence id → words its dedicated file name carries (LICENSE-MIT, LICENSE.APACHE2, …).
FILE_HINTS = {
    "MIT": ["MIT"],
    "Apache-2.0": ["APACHE"],
    "BSD-2-Clause": ["BSD"],
    "BSD-3-Clause": ["BSD"],
    "ISC": ["ISC"],
    "Zlib": ["ZLIB"],
    "Unicode-3.0": ["UNICODE"],
    "Unicode-DFS-2016": ["UNICODE"],
    "MPL-2.0": ["MPL"],
    "BSL-1.0": ["BOOST", "BSL"],
    "Unlicense": ["UNLICENSE"],
    "CC0-1.0": ["CC0"],
    "0BSD": ["0BSD"],
    "OFL-1.1": ["OFL"],
}


def run(command, cwd=ROOT):
    return subprocess.run(command, cwd=cwd, check=True, capture_output=True, text=True).stdout


def terms(expression):
    """The licences an SPDX expression names: `(MIT OR Apache-2.0) AND Unicode-3.0` → its three ids."""
    cleaned = expression.replace("(", " ").replace(")", " ").replace("/", " OR ")
    parts = re.split(r"\s+(?:OR|AND)\s+", cleaned.strip())
    return [" ".join(part.split()) for part in parts if part.strip()]


def normalized(expression):
    """`MIT/Apache-2.0` and `Apache-2.0 OR MIT` both as `Apache-2.0 OR MIT`, so they group together."""
    expression = " ".join(expression.replace(" / ", " OR ").replace("/", " OR ").split())
    if "(" in expression or " AND " in expression:
        return expression
    return " OR ".join(sorted(set(expression.split(" OR ")), key=str.lower))


def license_files(directory):
    if not directory.is_dir():
        return []
    return sorted(p for p in directory.iterdir() if p.is_file() and LICENSE_FILE.match(p.name))


def read(path):
    try:
        return path.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip()
    except OSError:
        return ""


def copyright_lines(files, fallback):
    found = []
    for path in files:
        for line in read(path).splitlines():
            line = " ".join(line.strip().strip("#*/").split())
            if COPYRIGHT.match(line) and "[yyyy]" not in line and line not in found:
                found.append(line)
    if found:
        return found[:4]
    return [f"Copyright: {', '.join(fallback)}"] if fallback else []


def text_for(term, files):
    """The licence text `term` names, from a package's own files."""
    base = term.split(" WITH ")[0]
    hints = FILE_HINTS.get(base, [base.split("-")[0].upper()])
    licences = [p for p in files if not p.name.upper().startswith("NOTICE")]
    exception = " WITH " in term
    dedicated = [
        p
        for p in licences
        if any(h in p.name.upper() for h in hints) and ("EXCEPTION" in p.name.upper()) == exception
    ]
    if dedicated:
        return read(dedicated[0])
    if len(licences) == 1:
        return read(licences[0])
    return None


def body(text):
    """A licence text without the copyright lines of the package it came from."""
    return "\n".join(line for line in text.splitlines() if not COPYRIGHT.match(line)).strip()


def rust_packages():
    metadata = json.loads(run(["cargo", "metadata", "--format-version", "1", "--locked"]))
    packages = {p["id"]: p for p in metadata["packages"]}
    members = set(metadata["workspace_members"])
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    apps = [p["id"] for p in metadata["packages"] if p["name"] in APP_CRATES]
    assert len(apps) == len(APP_CRATES), f"missing one of {APP_CRATES}"
    seen, stack = set(), list(apps)
    while stack:
        node = nodes[stack.pop()]
        for dep in node["deps"]:
            if any(kind["kind"] is None for kind in dep["dep_kinds"]) and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                stack.append(dep["pkg"])
    for package_id in sorted(seen - members, key=lambda i: (packages[i]["name"], packages[i]["version"])):
        package = packages[package_id]
        directory = Path(package["manifest_path"]).parent
        expression = package.get("license")
        files = license_files(directory)
        if not expression and package.get("license_file"):
            expression = f"LicenseRef-{package['name']}"
            files = [directory / package["license_file"]]
        yield {
            "name": package["name"],
            "version": package["version"],
            "license": expression or "Unknown",
            "repository": package.get("repository") or package.get("homepage") or "",
            "authors": [re.sub(r"\s*<[^>]*>", "", a) for a in package.get("authors", [])],
            "files": files,
            "ecosystem": "rust",
        }


def font(licence, files):
    """A font family from its licence file: its project's name and home from the copyright line."""
    text = read(licence)
    assert OFL in text, f"{licence.relative_to(ROOT)} isn't the {OFL}"
    lines = copyright_lines([licence], [])
    project = re.search(r"The (.+?) Project Authors", lines[0]) if lines else None
    home = re.search(r"\((https?://[^)\s]+)\)", lines[0]) if lines else None
    stem = re.sub(r"-latin.*$", "", files[0].stem) if files else licence.stem
    return {
        "name": project.group(1) if project else stem.replace("-", " ").title(),
        "version": "",
        "license": "OFL-1.1",
        "repository": home.group(1) if home else "",
        "authors": [],
        "files": [licence],
        "ecosystem": "font",
    }


def fonts():
    for licence in sorted(APP_FONTS.glob("LICENSE-*")):
        family = licence.name.removeprefix("LICENSE-")
        files = sorted(APP_FONTS.glob(f"{family}-*.woff2"))
        assert files, f"{licence.relative_to(ROOT)} licenses no font"
        yield font(licence, files)
    licensed = {p.name.removeprefix("LICENSE-") for p in APP_FONTS.glob("LICENSE-*")}
    for path in APP_FONTS.glob("*.woff2"):
        assert any(path.name.startswith(f"{family}-") for family in licensed), f"{path.name} has no LICENSE-<family>"
    themes = re.findall(r'builtin_theme!\(\s*"([^"]+)":', BUILTIN_THEMES.read_text())
    assert themes, f"no built-in themes in {BUILTIN_THEMES.relative_to(ROOT)}"
    for theme in themes:
        folder = ROOT / "themes" / theme / "assets/fonts"
        files = sorted(folder.glob("*.woff2"))
        if files:
            licence = folder / "OFL.txt"
            assert licence.is_file(), f"{folder.relative_to(ROOT)} has fonts but no OFL.txt"
            yield font(licence, files)


def icons():
    yield {
        "name": "Lucide",
        "version": "",
        # ISC, with the icons derived from Feather under MIT.
        "license": "ISC AND MIT",
        "repository": "https://lucide.dev",
        "authors": [],
        "files": [LUCIDE],
        "ecosystem": "icons",
    }


def seven_zip():
    spec = importlib.util.spec_from_file_location("sevenzip_download", ROOT / "scripts/7zz-download.py")
    downloader = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(downloader)
    cache = ROOT / "target/sidecars/7zip" / downloader.PINS["version"]
    texts = []
    for platform, package in SEVEN_ZIP_PACKAGES.items():
        licence = cache / package / "License.txt"
        if not licence.is_file():
            downloader.install(cache, package, None)
        text = read(licence)
        same = next((t for t in texts if t["text"] == text), None)
        if same:
            same["platforms"].append(platform)
        else:
            texts.append({"platforms": [platform], "text": text})
    return {"version": downloader.PINS["version"], "release": downloader.PINS["release"], "licenses": texts}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args()

    texts, groups = {}, {}
    for package in [*rust_packages(), *fonts(), *icons()]:
        for term in terms(package["license"]):
            if term not in texts:
                text = text_for(term, package["files"])
                if text:
                    texts[term] = body(text)
        notices = [read(p) for p in package["files"] if p.name.upper().startswith("NOTICE")]
        entry = {
            "name": package["name"],
            "version": package["version"],
            "ecosystem": package["ecosystem"],
            "repository": package["repository"],
            "copyright": copyright_lines(package["files"], package["authors"]),
        }
        if any(notices):
            entry["notice"] = "\n\n".join(n for n in notices if n)
        groups.setdefault(normalized(package["license"]), []).append(entry)

    ordered = sorted(groups.items(), key=lambda item: (-len(item[1]), item[0]))
    used = {term for expression in groups for term in terms(expression)}
    result = {
        "generatedBy": "scripts/legal-notices.py",
        "sevenZip": seven_zip(),
        "groups": [{"license": expression, "packages": packages} for expression, packages in ordered],
        "texts": {term: texts[term] for term in sorted(used) if term in texts},
        "missingTexts": sorted(term for term in used if term not in texts),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    # Biome checks JSON under apps/: write it the way Biome formats it.
    subprocess.run(["pnpm", "exec", "biome", "format", "--write", str(args.output)], cwd=ROOT, check=True, capture_output=True)
    total = sum(len(p) for p in groups.values())
    print(f"{args.output.relative_to(ROOT)}: {total} packages, {len(result['texts'])} licence texts, missing {result['missingTexts']}")


if __name__ == "__main__":
    main()

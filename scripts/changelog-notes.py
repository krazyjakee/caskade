#!/usr/bin/env python3
"""Extract release notes for a version from CHANGELOG.md.

Used by `.github/workflows/release.yml` to populate both the GitHub Release
body and `latest.json`'s `notes` field. Reads a Keep a Changelog section
heading like `## [0.2.2] - 2026-11-23` (or `## [0.2.2]`). For prerelease tags
whose exact version is not yet in `CHANGELOG.md`, falls back to `[Unreleased]`
so test/RC tags still produce valid release notes.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
HEADING_RE = re.compile(r"^## \[([^\]]+)\](?:\s*-\s*.*)?$")


def extract_section(text: str, heading: str) -> str | None:
    lines = text.splitlines()
    start: int | None = None
    for index, line in enumerate(lines):
        match = HEADING_RE.match(line.strip())
        if not match:
            continue
        if start is None:
            if match.group(1) == heading:
                start = index + 1
        else:
            body = "\n".join(lines[start:index]).strip()
            return body
    if start is not None:
        return "\n".join(lines[start:]).strip()
    return None


def to_plain(markdown: str) -> str:
    """Convert Keep a Changelog markdown into clean plain text for the in-app updater."""
    out: list[str] = []
    for raw in markdown.splitlines():
        line = raw.strip()
        if not line:
            if out and out[-1] != "":
                out.append("")
            continue
        if line.startswith("### "):
            out.append(line.removeprefix("### ").strip() + ":")
            continue
        # Strip inline markdown formatting (`**bold**`, `` `code` ``, `[label](url)`).
        line = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", line)
        line = re.sub(r"\*\*([^*]+)\*\*", r"\1", line)
        line = line.replace("`", "")
        out.append(line)
    return "\n".join(out).strip()


def release_notes(
    changelog_text: str,
    version: str,
    *,
    allow_unreleased: bool = False,
    plain: bool = False,
) -> str:
    section = extract_section(changelog_text, version)
    if not section and (allow_unreleased or "-" in version):
        section = extract_section(changelog_text, "Unreleased")
    if not section:
        return ""
    return to_plain(section) if plain else section


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="Release version without leading v (e.g. 0.2.2)")
    parser.add_argument(
        "--changelog",
        type=Path,
        default=ROOT / "CHANGELOG.md",
        help="Path to CHANGELOG.md",
    )
    parser.add_argument(
        "--allow-unreleased",
        action="store_true",
        help="Fall back to [Unreleased] if the exact version heading is absent.",
    )
    parser.add_argument(
        "--plain",
        action="store_true",
        help="Render plain text suitable for the in-app updater (latest.json).",
    )
    parser.add_argument(
        "--require",
        action="store_true",
        help="Exit non-zero when no notes are found for the requested version.",
    )
    args = parser.parse_args()

    version = args.version.removeprefix("v")
    try:
        text = args.changelog.read_text(encoding="utf-8")
    except OSError as error:
        print(f"Could not read {args.changelog}: {error}", file=sys.stderr)
        return 1

    notes = release_notes(
        text,
        version,
        allow_unreleased=args.allow_unreleased,
        plain=args.plain,
    )
    if not notes and args.require:
        print(
            f"No changelog entry found for [{version}] in {args.changelog}. "
            f"Add a `## [{version}] - YYYY-MM-DD` section before tagging.",
            file=sys.stderr,
        )
        return 1
    if notes:
        print(notes)
    return 0


if __name__ == "__main__":
    sys.exit(main())

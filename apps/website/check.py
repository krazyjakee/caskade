#!/usr/bin/env python3
"""Check the generated artifact offline, including links below /caskade/."""
from html.parser import HTMLParser
from pathlib import Path
import json
import re
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parent / "dist"


class Page(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.path = path
        self.ids = set()
        self.links = []
        self.downloads = []
        self.headings = 0
        self.language = None
        self.parts = set()
        self.feed(path.read_text())

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "data-part" in attrs:
            self.parts.add(attrs["data-part"])
        if "id" in attrs:
            assert attrs["id"] not in self.ids, f"Duplicate id in {self.path}"
            self.ids.add(attrs["id"])
        if tag == "html":
            self.language = attrs.get("lang")
        if tag == "h1":
            self.headings += 1
        if tag == "img":
            assert "alt" in attrs, f"Missing image alternative in {self.path}"
        for name in ("href", "src"):
            if name in attrs:
                self.links.append(attrs[name])
        if tag == "a" and attrs.get("data-part") == "button" and not attrs["href"].startswith("#"):
            self.downloads.append(attrs["href"])
        if "popovertarget" in attrs:
            self.links.append("#" + attrs["popovertarget"])


pages = {path.resolve(): Page(path) for path in ROOT.rglob("*.html")}
anatomy = json.loads((ROOT.parents[2] / "packages/ui/anatomy.json").read_text())
parts = set(anatomy["roots"]["website"]["parts"])
assert {page.language for page in pages.values()} == {"en", "sv"}, "Both languages must build"
for path, page in pages.items():
    suffixes = ("_amd64.AppImage", "_amd64.deb", "_x64-setup.exe", "_aarch64.dmg", "_x64.dmg")
    assert len(page.downloads) == len(suffixes), f"Expected five direct installer links in {path}"
    for suffix in suffixes:
        assert sum(link.endswith(suffix) for link in page.downloads) == 1, f"Missing or duplicate installer: {suffix}"
    assert page.headings == 1, f"Expected one main heading in {path}"
    assert page.parts == parts, f"Website anatomy differs: {page.parts ^ parts}"
    assert "{{" not in path.read_text(), f"Unresolved template in {path}"
    for link in page.links:
        url = urlsplit(link)
        if url.scheme or url.netloc:
            assert url.scheme == "https", f"Unexpected external URL: {link}"
            continue
        assert not url.path.startswith("/"), f"Root-relative URL breaks project Pages: {link}"
        target = (path.parent / unquote(url.path)).resolve() if url.path else path
        assert target.is_relative_to(ROOT.resolve()), f"Link leaves the artifact: {link}"
        if target.is_dir():
            target /= "index.html"
        assert target.is_file(), f"Missing local target {link} from {path}"
        if url.fragment and target.suffix == ".html":
            assert unquote(url.fragment) in pages[target].ids, f"Missing anchor: {link}"

for path in (ROOT / "assets").glob("*.css"):
    for asset in re.findall(r'url\([\'"]?([^\)\'\"]+)', path.read_text()):
        assert (path.parent / asset).is_file(), f"Missing CSS asset: {asset}"

print("Both languages, headings, image alternatives, local assets and fragment links pass.")

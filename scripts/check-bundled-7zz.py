#!/usr/bin/env python3
"""Checks that the 7-Zip each Linux bundle carries is one the app accepts.

The app runs only a 7zz whose SHA-256 is in `SevenZip::pinned`
(crates/caskade-installer/src/formats/archive/sevenzip/mod.rs). The AppImage's
bundler rewrites the programs it packs, so a bundler change could ship a 7zz the
app refuses (v0.1.4's and the first v0.2.0 build's AppImages did).

    python3 scripts/check-bundled-7zz.py <bundle files: .deb, .AppImage>…
"""
import hashlib, pathlib, re, subprocess, sys, tempfile

PINNED = pathlib.Path(__file__).resolve().parent.parent / "crates/caskade-installer/src/formats/archive/sevenzip/mod.rs"


def pinned():
    source = PINNED.read_text()
    body = source[source.index("pub fn pinned"):source.index("UnpinnedBinary),", source.index("pub fn pinned"))]
    return set(re.findall(r'"([0-9a-f]{64})"', body))


def bundled_7zz(bundle, scratch):
    if bundle.suffix == ".deb":
        subprocess.run(["dpkg-deb", "-x", str(bundle), str(scratch)], check=True)
    elif bundle.suffix == ".AppImage":
        bundle.chmod(bundle.stat().st_mode | 0o111)
        subprocess.run([str(bundle), "--appimage-extract", "usr/bin/7zz"], cwd=scratch, check=True,
                       stdout=subprocess.DEVNULL)
        scratch = scratch / "squashfs-root"
    else:
        raise SystemExit(f"{bundle}: not a .deb or .AppImage")
    return scratch / "usr/bin/7zz"


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    accepted, failed = pinned(), False
    for bundle in map(pathlib.Path, sys.argv[1:]):
        with tempfile.TemporaryDirectory() as scratch:
            program = bundled_7zz(bundle.resolve(), pathlib.Path(scratch))
            if not program.is_file():
                print(f"{bundle.name}: carries no usr/bin/7zz")
                failed = True
                continue
            digest = hashlib.sha256(program.read_bytes()).hexdigest()
            ok = digest in accepted
            failed |= not ok
            print(f"{bundle.name}: 7zz {digest} {'accepted' if ok else 'NOT in SevenZip::pinned: the app would refuse it'}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()

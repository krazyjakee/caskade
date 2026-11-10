"""Writes the patched Blitz, anyrender, Stylo and Taffy crates into vendor/blitz/crates
as standalone packages (update.sh runs this).

  normalize.py <cargo metadata JSON> <out dir> <crate>...

Each crate's sources are copied without tests, benches or examples (only what
its manifest's `include` lists, when it has one), and its
Cargo.toml is regenerated from `cargo metadata`, which has every
`workspace = true` field and dependency already resolved: the crates then
build on their own, outside the workspaces they came from. Dependencies on
other vendored crates become path dependencies; dev-dependencies are left out.
"""
import glob
import json
import os
import shutil
import sys
import tomllib

meta_path, out, *names = sys.argv[1:]
names = set(names)
meta = json.load(open(meta_path))
# The checkouts' crates are the local (path) packages: a crates.io release of
# the same name (anyrender's debug_timer, say) stays where it is.
by_name = {}
for pkg in meta["packages"]:
    if pkg["name"] in names and pkg.get("source") is None:
        if pkg["name"] in by_name:
            sys.exit(f"{pkg['name']} appears twice in the checkouts")
        by_name[pkg["name"]] = pkg
missing = names - set(by_name)
if missing:
    sys.exit(f"not in the metadata: {sorted(missing)}")


def q(value):
    """A TOML basic string (JSON escaping is valid TOML)."""
    return json.dumps(value)


def key(name):
    return name if all(c.isalnum() or c in "-_" for c in name) else q(name)


def inline(table):
    parts = []
    for k, v in table.items():
        if isinstance(v, bool):
            parts.append(f"{key(k)} = {'true' if v else 'false'}")
        elif isinstance(v, list):
            parts.append(f"{key(k)} = [{', '.join(q(x) for x in v)}]")
        else:
            parts.append(f"{key(k)} = {q(v)}")
    return "{ " + ", ".join(parts) + " }"


def git_source(source):
    """git+https://host/repo?rev=abc#commit -> (url, {rev|branch|tag: ...})."""
    url, _, _commit = source[len("git+"):].partition("#")
    url, _, query = url.partition("?")
    spec = {}
    for pair in filter(None, query.split("&")):
        k, _, v = pair.partition("=")
        spec[k] = v
    return url, spec


IGNORED = {"tests", "benches", "examples", "target", ".git", "fuzz", "wpt"}


def is_vendored(dep):
    return dep["name"] in by_name and not (dep.get("source") or "").startswith("registry+")


# Optional path dependencies outside the vendored set (Blitz's devtools
# server) are left out, with every feature that would enable them, in any
# crate: neither app uses them.
dropped_deps = {
    (name, dep.get("rename") or dep["name"])
    for name, pkg in by_name.items()
    for dep in pkg["dependencies"]
    if dep["kind"] != "dev" and dep.get("path") and not is_vendored(dep)
}
for name, pkg in by_name.items():
    for dep in pkg["dependencies"]:
        if (name, dep.get("rename") or dep["name"]) in dropped_deps and not dep["optional"]:
            sys.exit(f"{name} needs {dep['name']} by path, outside the vendored set")
dropped_features = set()
changed = True
while changed:
    changed = False
    for name, pkg in by_name.items():
        for feature, enables in pkg["features"].items():
            if (name, feature) in dropped_features:
                continue
            for item in enables:
                target = item.removeprefix("dep:")
                crate, _, feat = target.replace("?/", "/").partition("/")
                if (name, crate) in dropped_deps or (
                    feat and crate in by_name and (crate, feat) in dropped_features
                ) or (not feat and (name, crate) in dropped_features):
                    dropped_features.add((name, feature))
                    changed = True
                    break

for name, pkg in sorted(by_name.items()):
    src_dir = os.path.dirname(pkg["manifest_path"])
    dest = os.path.join(out, name)
    shutil.rmtree(dest, ignore_errors=True)
    # A manifest's `include` lists what the package ships (Taffy's, whose
    # crate is its repository's root, leaves out its fixtures and tools).
    with open(pkg["manifest_path"], "rb") as manifest:
        include = tomllib.load(manifest).get("package", {}).get("include")
    if include:
        files = set()
        for pattern in include:
            for found in glob.glob(pattern, root_dir=src_dir, recursive=True):
                parts = found.split(os.sep)
                if os.path.isfile(os.path.join(src_dir, found)) and not any(
                    part in IGNORED for part in parts
                ) and found != "Cargo.lock":
                    files.add(found)
        for found in sorted(files):
            os.makedirs(os.path.dirname(os.path.join(dest, found)) or dest, exist_ok=True)
            shutil.copy2(os.path.join(src_dir, found), os.path.join(dest, found))
    else:
        shutil.copytree(
            src_dir,
            dest,
            ignore=lambda d, entries: [e for e in entries if e in IGNORED or e.endswith(".orig") or e == "Cargo.lock"],
        )

    lines = ["# Generated by vendor/blitz/normalize.py from the patched upstream manifest.", "[package]"]
    lines.append(f"name = {q(name)}")
    lines.append(f"version = {q(pkg['version'])}")
    lines.append(f"edition = {q(pkg['edition'])}")
    if pkg.get("rust_version"):
        lines.append(f"rust-version = {q(pkg['rust_version'])}")
    if pkg.get("license"):
        lines.append(f"license = {q(pkg['license'])}")
    if pkg.get("description"):
        lines.append(f"description = {q(pkg['description'])}")
    if pkg.get("repository"):
        lines.append(f"repository = {q(pkg['repository'])}")
    if pkg.get("links"):
        lines.append(f"links = {q(pkg['links'])}")
    lib = None
    for target in pkg["targets"]:
        kinds = target["kind"]
        rel = os.path.relpath(target["src_path"], src_dir)
        if "custom-build" in kinds:
            lines.append(f"build = {q(rel)}")
        elif any(k in kinds for k in ("lib", "rlib", "proc-macro", "cdylib", "staticlib", "dylib")):
            lib = (target, rel)
    lines += ["publish = false", "autobins = false", "autoexamples = false", "autotests = false", "autobenches = false"]
    if lib:
        target, rel = lib
        lines += ["", "[lib]", f"name = {q(target['name'].replace('-', '_'))}", f"path = {q(rel)}"]
        if "proc-macro" in target["kind"]:
            lines.append("proc-macro = true")
        elif target["kind"] != ["lib"]:
            lines.append(f"crate-type = [{', '.join(q(k) for k in target['crate_types'])}]")
    if pkg["features"]:
        lines += ["", "[features]"]
        for feature, enables in sorted(pkg["features"].items()):
            if (name, feature) in dropped_features:
                continue
            lines.append(f"{key(feature)} = [{', '.join(q(x) for x in enables)}]")

    sections = {}
    for dep in pkg["dependencies"]:
        if dep["kind"] == "dev" or (name, dep.get("rename") or dep["name"]) in dropped_deps:
            continue
        table = {}
        if dep.get("rename"):
            table["package"] = dep["name"]
        if is_vendored(dep):
            table["path"] = os.path.relpath(os.path.join(out, dep["name"]), dest)
            table["version"] = dep["req"]
        elif dep.get("source", "") and dep["source"].startswith("git+"):
            url, spec = git_source(dep["source"])
            table["git"] = url
            table.update(spec)
            if dep["req"] != "*":
                table["version"] = dep["req"]
        else:
            table["version"] = dep["req"]
        if not dep["uses_default_features"]:
            table["default-features"] = False
        if dep["features"]:
            table["features"] = dep["features"]
        if dep["optional"]:
            table["optional"] = True
        section = "dependencies" if dep["kind"] is None else "build-dependencies"
        if dep.get("target"):
            section = f"target.{q(dep['target'])}.{section}"
        sections.setdefault(section, []).append(f"{key(dep.get('rename') or dep['name'])} = {inline(table)}")
    for section in sorted(sections, key=lambda s: (s.startswith("target."), s)):
        lines += ["", f"[{section}]"] + sorted(sections[section])
    # The features left out above still appear in `cfg`s.
    lines += ["", "[lints.rust]", 'unexpected_cfgs = "allow"']

    with open(os.path.join(dest, "Cargo.toml"), "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print(f"{name} {pkg['version']}")

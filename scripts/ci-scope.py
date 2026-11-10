#!/usr/bin/env python3
"""Select ci.yml's PR checks without running Cargo; master and manual runs stay full.

Run in a full-history checkout of the pull request. Unknown paths, events or
unavailable history select the full suite rather than skipping checks.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
# GitHub cancels superseded PR runs itself (`concurrency` in each workflow).
PR_EVENTS = ("pull_request", "pull_request_target")
FIELDS = ("app", "frontend", "rust", "catalogs", "e2e", "native", "cross", "installer", "discovery")


def package_graph(root):
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    manifests = [p / "Cargo.toml" for pattern in workspace["members"] for p in root.glob(pattern)]
    packages = {}
    for path in manifests:
        if path.is_file():
            data = tomllib.loads(path.read_text())
            packages[data["package"]["name"]] = (path.parent, data)
    names = {directory.resolve(): name for name, (directory, _) in packages.items()}
    graph = {}
    for name, (directory, data) in packages.items():
        dependencies = set()
        for section in [data, *data.get("target", {}).values()]:
            for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
                for alias, dependency in section.get(kind, {}).items():
                    base = directory
                    if isinstance(dependency, dict) and dependency.get("workspace"):
                        dependency = workspace.get("dependencies", {}).get(alias, {})
                        base = root
                    if isinstance(dependency, dict) and "path" in dependency:
                        target = names.get((base / dependency["path"]).resolve())
                        if target:
                            dependencies.add(target)
        graph[name] = dependencies
    return packages, graph


def changed_paths(root, event):
    pr = event["pull_request"]
    base, head = pr["base"]["sha"], pr["head"]["sha"]
    if not all(isinstance(sha, str) and re.fullmatch(r"[0-9a-fA-F]{40,64}", sha) for sha in (base, head)):
        raise ValueError("missing or invalid PR commit IDs")
    # Full checkout history normally includes both; fetch a just-pushed head if
    # the checkout raced it. No PR-controlled text is interpreted by a shell.
    for sha in (base, head):
        exists = subprocess.run(["git", "cat-file", "-e", sha + "^{commit}"], cwd=root,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if exists.returncode:
            subprocess.run(["git", "fetch", "--no-tags", "origin", sha], cwd=root, check=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    result = subprocess.run(["git", "diff", "--name-only", "--no-renames", "-z", base + "..." + head],
                            cwd=root, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return [os.fsdecode(path) for path in result.stdout.split(b"\0") if path]


def scope(paths, root=ROOT):
    result = dict.fromkeys(FIELDS, False)
    packages, graph = package_graph(root)
    dirty = set()
    for path in paths:
        owners = {name for name, (directory, _) in packages.items()
                  if path.startswith(directory.relative_to(root).as_posix() + "/")}
        if not owners and path.endswith(".md"):
            # Prose can still be a compiled/tested input (e.g. docs/TECH-SPEC.md).
            # Derive direct include paths instead of maintaining a second list
            # that becomes stale when a crate adds a documentation contract.
            target = (root / path).resolve()
            for name, (directory, _) in packages.items():
                for source in directory.rglob("*.rs"):
                    includes = re.findall(r'include_(?:str|bytes)!\s*\(\s*"([^"]+)"', source.read_text())
                    if any((source.parent / included).resolve() == target for included in includes):
                        owners.add(name)
        if owners:
            dirty.update(owners)
            result["rust"] = result["frontend"] = result["catalogs"] = True
        elif path.startswith(("docs/legal/", "themes/", "plugins/", "wit/", "locales/")):
            result["rust"] = result["frontend"] = result["catalogs"] = True
            # The native UI's screenshots install a catalogue theme, and its
            # text comes from the bundled catalogs.
            if path.startswith(("themes/", "locales/")):
                result["native"] = True
        elif path.startswith("packages/") or path.startswith("apps/desktop/") or path in (
            "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "biome.json", "biome.jsonc",
            "docs/theming/anatomy.md", "docs/theming/tokens.md",
        ):
            result["frontend"] = result["catalogs"] = True
            # The theme compiler embeds these generated contracts.
            if path.startswith("packages/tokens/") or path == "packages/ui/anatomy.json":
                result["rust"] = result["native"] = True
        elif path in ("README.md", "AGENTS.md", "CLAUDE.md", "SECURITY.md") or (
            path.startswith("docs/") and path.endswith(".md")
        ):
            continue
        else:
            # Manifests, locks, vendored code, CI, tooling, new crates and
            # non-prose documentation can affect every package's contracts.
            return dict.fromkeys(FIELDS, True)
    affected = set(dirty)
    while True:
        more = {name for name, dependencies in graph.items() if dependencies & affected} - affected
        if not more:
            break
        affected.update(more)
    result["installer"] = "caskade-installer" in affected
    result["discovery"] = "caskade-discovery" in affected
    # The native UI's window, end-to-end and screenshot tests (the rust job
    # runs its headless ones): whatever caskade-native builds on.
    result["native"] = result["native"] or "caskade-native" in affected
    result["cross"] = result["rust"]
    result["app"] = result["frontend"] or result["rust"]
    return result


def select(event_name, event, root=ROOT):
    if event_name not in PR_EVENTS:
        result = dict.fromkeys(FIELDS, True)
    else:
        pr = event.get("pull_request", {})
        try:
            result = scope(changed_paths(root, event), root)
        except (KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
            print(f"Cannot determine PR scope ({type(error).__name__}); running all checks.")
            result = dict.fromkeys(FIELDS, True)
        labels = {label.get("name") for label in pr.get("labels") or []}
        if "run-e2e" in labels:
            result["app"] = result["frontend"] = result["rust"] = result["catalogs"] = result["e2e"] = True
            result["native"] = True
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", default=os.environ.get("GITHUB_EVENT_PATH"))
    parser.add_argument("--event-name", default=os.environ.get("GITHUB_EVENT_NAME", ""))
    args = parser.parse_args()
    try:
        event = json.loads(Path(args.event).read_text()) if args.event else {}
    except (OSError, ValueError):
        event = {}
    result = select(args.event_name, event)
    print(json.dumps(result, sort_keys=True))
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as stream:
            for key, value in result.items():
                stream.write(f"{key}={str(value).lower()}\n")


if __name__ == "__main__":
    main()

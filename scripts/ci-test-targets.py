#!/usr/bin/env python3
"""Prints `--test <name>` for every integration test `cargo test` would run
in the packages of a cargo argument list (#137).

`cargo test --test '*'` also names test targets whose required features are
off, and cargo refuses those, where a plain `cargo test` skips them. CI's
`rust` job runs the integration tests apart from the binaries' unit tests,
so it lists them with this:

    cargo test $APP_SUITES --lib $(scripts/ci-test-targets.py $APP_SUITES)

Reads `cargo metadata --no-deps`; understands -p/--package and
--features pkg/feature (comma or space separated), and each package's
default features.
"""

import json
import subprocess
import sys


def main(args: list[str]) -> None:
    packages: set[str] = set()
    enabled: set[tuple[str, str]] = set()
    it = iter(args)
    for arg in it:
        if arg in ("-p", "--package"):
            packages.add(next(it))
        elif arg == "--features":
            for item in next(it).replace(",", " ").split():
                if "/" in item:
                    package, feature = item.split("/", 1)
                    enabled.add((package, feature))
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    names: set[str] = set()
    for package in metadata["packages"]:
        if package["name"] not in packages:
            continue
        on = {feature for name, feature in enabled if name == package["name"]}
        # Defaults, and what features they turn on, transitively.
        pending = ["default"]
        while pending:
            feature = pending.pop()
            for implied in package["features"].get(feature, []):
                if "/" not in implied and not implied.startswith("dep:") and implied not in on:
                    on.add(implied)
                    pending.append(implied)
        for target in package["targets"]:
            if "test" not in target["kind"]:
                continue
            if all(feature in on for feature in target.get("required-features", [])):
                names.add(target["name"])
    print(" ".join(f"--test {name}" for name in sorted(names)))


if __name__ == "__main__":
    main(sys.argv[1:])

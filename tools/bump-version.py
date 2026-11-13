#!/usr/bin/env python3
"""Bump workspace and package versions without resolving dependency updates (Python 3.11+)."""
import argparse
import json
from pathlib import Path
import re
import tomllib


def parse_version(value):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value):
        raise ValueError("Version must be MAJOR.MINOR.PATCH (no leading zeros)")
    return tuple(map(int, value.split(".")))


def bump(root, requested, dry_run=False):
    manifest = root / "Cargo.toml"
    original = manifest.read_text()
    workspace = tomllib.loads(original)["workspace"]
    current = workspace["package"]["version"]
    numbers = list(parse_version(current))
    if requested in ("major", "minor", "patch"):
        index = ("major", "minor", "patch").index(requested)
        numbers[index] += 1
        numbers[index + 1:] = [0] * (2 - index)
        version = ".".join(map(str, numbers))
    else:
        version = requested
    if parse_version(version) <= parse_version(current):
        raise ValueError(f"New version must be greater than {current}")

    names = set()
    for member in workspace["members"]:
        package = tomllib.loads((root / member / "Cargo.toml").read_text())["package"]
        if package.get("version") == {"workspace": True}:
            names.add(package["name"])
    lock = root / "Cargo.lock"
    blocks = lock.read_text().split("[[package]]")
    found = set()
    for index, block in enumerate(blocks[1:], 1):
        package = tomllib.loads(block)
        if package["name"] in names and "source" not in package:
            if package["version"] != current:
                raise ValueError(f"Lockfile version is out of sync: {package['name']}")
            blocks[index] = block.replace(f'version = "{current}"', f'version = "{version}"', 1)
            found.add(package["name"])
    if found != names:
        raise ValueError("Workspace packages missing from Cargo.lock; run cargo check first")
    updated, count = re.subn(
        r'(\[workspace\.package\]\s*\nversion = ")[^"]+(")',
        lambda match: match[1] + version + match[2], original,
    )
    if count != 1:
        raise ValueError("Could not locate workspace.package.version")
    config_path = root / "crates/iptv-app/nsh.json"
    config = json.loads(config_path.read_text())
    config["version"] = version
    changes = {manifest: updated, lock: "[[package]]".join(blocks),
               config_path: json.dumps(config, indent=2) + "\n"}
    if not dry_run:
        for path, content in changes.items():
            path.write_text(content)
    return current, version


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="patch, minor, major, or an explicit MAJOR.MINOR.PATCH")
    parser.add_argument("--dry-run", action="store_true", help="validate and preview without writing")
    args = parser.parse_args()
    try:
        old, new = bump(Path(__file__).resolve().parents[1], args.version, args.dry_run)
    except (ValueError, OSError, KeyError) as error:
        parser.exit(1, f"error: {error}\n")
    print(f"{'Would bump' if args.dry_run else 'Bumped'} {old} -> {new}")
    print(f"Review and commit the changes, then tag that commit v{new} when ready to release.")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""A source dependency update must also reach the committed runner lock."""
from pathlib import Path
import tomllib
import json
import subprocess
root = Path(__file__).resolve().parents[2]
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=root))
nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
pending = list(metadata["workspace_members"])
reachable = set()
while pending:
    node = pending.pop()
    if node in reachable:
        continue
    reachable.add(node)
    for dependency in nodes[node]["deps"]:
        if any(kind["kind"] != "dev" for kind in dependency["dep_kinds"]):
            pending.append(dependency["pkg"])
needed = {(p["name"], p["version"], p["source"]) for p in metadata["packages"] if p["id"] in reachable}
source = [p for p in tomllib.loads((root / "Cargo.lock").read_text())["package"]
          if (p["name"], p["version"], p.get("source")) in needed]
packaged = tomllib.loads((root / "tools/ci/package.Cargo.lock").read_text())["package"]
names = {(p["name"], p.get("source")) for p in packaged}
identities = {(p["name"], p["version"], p.get("source"), p.get("checksum")) for p in packaged}
missing = [f'{p["name"]} {p["version"]}' for p in source
           if (p["name"], p.get("source")) in names and
           (p["name"], p["version"], p.get("source"), p.get("checksum")) not in identities]
if missing:
    raise SystemExit("Regenerate tools/ci/package.Cargo.lock; mismatched source dependencies: " + ", ".join(missing))
print("shared source and runner lock identities agree")

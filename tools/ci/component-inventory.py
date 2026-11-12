#!/usr/bin/env python3
"""Project Cargo's graph to portable component identities, without build paths."""
import json
from pathlib import Path
import sys
import tomllib
metadata = json.load(sys.stdin)
locked = tomllib.loads(Path(sys.argv[1]).read_text())["package"]
checksums = {(p["name"], p["version"], p.get("source")): p.get("checksum") for p in locked}
nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
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
components = [{"name": p["name"], "version": p["version"], "source": p["source"],
               "license": p["license"], "checksum": checksums.get((p["name"], p["version"], p["source"]))}
              for p in metadata["packages"] if p["id"] in reachable]
json.dump({"scope": "resolved normal and build dependency superset; generated workspaces also include unused host variants",
           "components": sorted(components, key=lambda p: (p["name"], p["version"]))}, sys.stdout, indent=2)
print()

#!/usr/bin/env python3
"""The fuzz targets as a JSON list, for a workflow matrix.

`cargo fuzz list` prints the [[bin]] names of fuzz/Cargo.toml; this reads the
same entries without installing cargo-fuzz in the job that only plans the
matrix. tools/ci/fuzz-imports.sh then checks each name against `cargo fuzz
list` itself, so a disagreement fails instead of skipping a target.

Usage: fuzz-targets.py [MANIFEST, default fuzz/Cargo.toml]
"""
import json
from pathlib import Path
import re
import sys
import tomllib


def targets(manifest):
    names = [target["name"] for target in tomllib.loads(manifest).get("bin", [])]
    if not names or len(set(names)) != len(names):
        raise ValueError("the fuzz manifest must name at least one target, each once")
    for name in names:
        # The names become cache keys, artifact names and shell arguments.
        if not re.fullmatch(r"[a-z0-9_]+", name):
            raise ValueError(f"unsupported fuzz target name: {name}")
    return sorted(names)


if __name__ == "__main__":
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2] / "fuzz/Cargo.toml"
    print(json.dumps(targets(path.read_text(encoding="utf-8"))))

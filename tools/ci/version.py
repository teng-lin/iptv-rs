"""Validate tag/version agreement and stamp generated package metadata."""
import json
import os
from pathlib import Path
import tomllib

version = tomllib.loads(Path("Cargo.toml").read_text())["workspace"]["package"]["version"]
ref = os.environ["GITHUB_REF"]
if ref.startswith("refs/tags/") and ref != f"refs/tags/v{version}":
    raise SystemExit("Release tag must equal v + workspace package version")
path = Path("crates/iptv-app/nsh.json")
config = json.loads(path.read_text())
config["version"] = version
path.write_text(json.dumps(config, indent=2) + "\n")
with open(os.environ["GITHUB_ENV"], "a") as env:
    env.write(f"VERSION={version}\n")

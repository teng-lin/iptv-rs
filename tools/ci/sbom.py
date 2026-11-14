#!/usr/bin/env python3
"""Write a CycloneDX SBOM of the Rust dependency graph for a release.

Runs `cargo cyclonedx` (0.5.10) for the app crate with every target enabled,
so one document covers the Android, desktop, Windows and iPad dependency sets
resolved in Cargo.lock: normal and build dependencies, no dev-dependencies.
Build paths are replaced by paths relative to the repository, and the NSH
commit (a path dependency, so it has no registry checksum) is recorded.

Not covered here: the platform host crates of the generated NSH runners
(`runner-components.json` inside each package), and the native media libraries
(their inventories are inside the packages, their sources beside the release).

Usage: sbom.py OUTPUT.cdx.json
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def portable(text, root):
    """The document without the build machine's directories."""
    root = Path(root)
    for directory, relative in ((root, "."), (root.parent, "..")):
        text = text.replace(f"file://{directory}", f"file://{relative}")
    for directory in (root, root.parent):
        if str(directory) in text:
            raise ValueError("SBOM still names a build directory")
    return text


def finish(text, root, nsh_revision):
    document = json.loads(portable(text, root))
    if document.get("bomFormat") != "CycloneDX" or not document.get("components"):
        raise ValueError("cargo cyclonedx produced no component list")
    names = {component["name"] for component in document["components"]}
    workspace = {"iptv-core", "iptv-net", "iptv-store-sqlite", "nsh-core"}
    if not workspace <= names:
        raise ValueError(f"SBOM lacks workspace or NSH crates: {sorted(workspace - names)}")
    properties = document["metadata"].setdefault("properties", [])
    properties.append({"name": "patina:nsh-revision", "value": nsh_revision})
    properties.append({"name": "patina:scope", "value":
                       "Cargo.lock graph of iptv-app for all targets; normal and build dependencies"})
    return json.dumps(document, indent=2) + "\n"


def main(arguments):
    if len(arguments) != 1:
        raise SystemExit(__doc__)
    output = Path(arguments[0]).resolve()
    spec = importlib.util.spec_from_file_location("pins", Path(__file__).with_name("pins.py"))
    pins = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(pins)
    lock = (ROOT / "Cargo.lock").read_bytes()
    commit_time = subprocess.check_output(["git", "-C", str(ROOT), "log", "-1", "--format=%ct"], text=True).strip()
    generated = ROOT / "crates/iptv-app/iptv-app.cdx.json"
    try:
        # It writes one file beside every workspace manifest; only the app's is kept.
        subprocess.run(["cargo", "cyclonedx", "--manifest-path", str(ROOT / "crates/iptv-app/Cargo.toml"),
                        "--format", "json", "--spec-version", "1.5", "--target", "all"],
                       cwd=ROOT, check=True, env=dict(os.environ, SOURCE_DATE_EPOCH=commit_time))
        text = generated.read_text()
    finally:
        for leftover in ROOT.glob("crates/*/*.cdx.json"):
            leftover.unlink()
    # cargo cyclonedx has no --locked: make sure it described the committed lock.
    if (ROOT / "Cargo.lock").read_bytes() != lock:
        raise SystemExit("cargo cyclonedx changed Cargo.lock; the SBOM would not describe the release")
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(finish(text, ROOT, pins.nsh_revision()))
    print(output)


if __name__ == "__main__":
    main(sys.argv[1:])

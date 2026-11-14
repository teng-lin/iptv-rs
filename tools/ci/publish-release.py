#!/usr/bin/env python3
"""Publish a tagged release exactly once.

A public release is immutable: people have downloaded its files, and the
checksums and provenance attestations name those exact bytes. This script
therefore only ever writes to a release that is absent or still a draft, and
publishes it once every file is uploaded. An interrupted run leaves a draft,
which a rerun completes; a release that is already public is refused, and the
fix is a new version.

Usage (with GH_TOKEN and GH_REPO in the environment):
  publish-release.py checksums DIR TAG  refuse a public release, write DIR/SHA256SUMS
  publish-release.py publish DIR TAG    create or reuse the draft, upload DIR/*, publish
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

CHECKSUMS = "SHA256SUMS"


def gh(*arguments):
    return subprocess.run(["gh", *arguments], capture_output=True, text=True)


def describe(tag):
    """(absent, draft or public; the names of its files). Anything unclear is
    an error, never "absent"."""
    result = gh("release", "view", tag, "--json", "isDraft,tagName,assets")
    if result.returncode != 0:
        if "release not found" in result.stderr.lower():
            return "absent", []
        raise SystemExit(f"could not read the state of release {tag}: {result.stderr.strip()}")
    release = json.loads(result.stdout)
    files = release.get("assets", [])
    if (release.get("tagName") != tag or not isinstance(release.get("isDraft"), bool)
            or not isinstance(files, list) or not all(isinstance(f, dict) and isinstance(f.get("name"), str) for f in files)):
        raise SystemExit(f"unexpected description of release {tag}: {result.stdout.strip()}")
    return ("draft" if release["isDraft"] else "public"), [f["name"] for f in files]


def release_state(tag):
    return describe(tag)[0]


def refuse_public(tag):
    state = release_state(tag)
    if state == "public":
        raise SystemExit(
            f"release {tag} is already public: published files are never replaced. "
            "Release a new version instead.")
    return state


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def assets(directory):
    """Every file to release, by name, checksum file aside."""
    entries = sorted(directory.iterdir(), key=lambda path: path.name)
    if any(not path.is_file() or path.is_symlink() for path in entries):
        raise SystemExit(f"{directory} must hold only regular files")
    files = [path for path in entries if path.name != CHECKSUMS]
    if not files:
        raise SystemExit(f"no files to release in {directory}")
    return files


def write_checksums(directory):
    """The format `sha256sum --check` reads, names without a directory."""
    lines = [f"{digest(path)}  {path.name}\n" for path in assets(directory)]
    (directory / CHECKSUMS).write_text("".join(lines))
    return lines


def verify_checksums(directory):
    """The files about to be uploaded are exactly the ones that were attested."""
    listed = (directory / CHECKSUMS).read_text().splitlines(keepends=True)
    actual = [f"{digest(path)}  {path.name}\n" for path in assets(directory)]
    if listed != actual:
        raise SystemExit(f"{directory}/{CHECKSUMS} does not describe the files beside it")


def must(*arguments):
    result = gh(*arguments)
    if result.returncode != 0:
        raise SystemExit(f"gh {' '.join(arguments[:3])} failed: {result.stderr.strip()}")


def publish(directory, tag):
    state = refuse_public(tag)
    verify_checksums(directory)
    if state == "absent":
        must("release", "create", tag, "--verify-tag", "--draft", "--generate-notes")
    # Replacing is safe only here: the release was just seen to be a draft.
    files = [*assets(directory), directory / CHECKSUMS]
    must("release", "upload", tag, *(str(path) for path in files), "--clobber")
    state, present = describe(tag)
    if state != "draft":
        raise SystemExit(f"release {tag} stopped being a draft during the upload; not publishing")
    # An earlier attempt may have left files this one does not have: they are
    # in neither SHA256SUMS nor the attestations, so they must not go public.
    for name in sorted(set(present) - {path.name for path in files}):
        must("release", "delete-asset", tag, name, "--yes")
    must("release", "edit", tag, "--draft=false")


def main(arguments):
    if len(arguments) != 3 or arguments[0] not in ("checksums", "publish"):
        raise SystemExit(__doc__)
    command, directory, tag = arguments[0], Path(arguments[1]), arguments[2]
    if command == "checksums":
        refuse_public(tag)
        sys.stdout.writelines(write_checksums(directory))
    else:
        publish(directory, tag)
        print(f"published {tag}")


if __name__ == "__main__":
    main(sys.argv[1:])

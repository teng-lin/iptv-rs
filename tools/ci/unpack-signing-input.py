#!/usr/bin/env python3
"""Accept only the expected artifact subtree, never scripts from the build job."""
import os
import posixpath
from pathlib import Path
import shutil
import tarfile
import tempfile

def validate_artifact(expected):
    if expected.is_symlink() or not expected.is_dir():
        raise ValueError("Signing input must be a real artifact directory")
    expected = expected.resolve()
    for entry in expected.rglob("*"):
        if entry.is_symlink() and not entry.resolve().is_relative_to(expected):
            raise ValueError("Resolved signing input link escapes artifact subtree")


if __name__ == "__main__":
    subtree = {"android": "android-unsigned", "macos": "package", "ios": "ios"}[os.environ["PACKAGE_PLATFORM"]]
    runner_temp = Path(os.environ["RUNNER_TEMP"])
    with tempfile.TemporaryDirectory(dir=runner_temp) as directory:
        stage = Path(directory)
        with tarfile.open(runner_temp / "signing-download/signing-input.tar") as archive:
            for member in archive.getmembers():
                parts = Path(member.name).parts
                if parts[:2] != ("target", subtree) or ".." in parts:
                    raise ValueError("Unexpected path in signing input")
                if member.issym() or member.islnk():
                    link = member.linkname
                    if member.issym():
                        link = posixpath.join(posixpath.dirname(member.name), link)
                    link_parts = Path(posixpath.normpath(link)).parts
                    if link_parts[:2] != ("target", subtree):
                        raise ValueError("Signing input link escapes artifact subtree")
            archive.extractall(stage, filter="data")
        validate_artifact(stage / "target" / subtree)
        Path("target").mkdir(exist_ok=True)
        destination = Path("target") / subtree
        if destination.exists():
            raise ValueError("Signing destination already exists")
        shutil.move(str(stage / "target" / subtree), destination)

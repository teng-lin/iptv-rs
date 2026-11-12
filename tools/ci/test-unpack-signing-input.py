#!/usr/bin/env python3
import io
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("unpack-signing-input.py").resolve()

class SigningInputs(unittest.TestCase):
    def run_archive(self, name, link=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "signing-download").mkdir()
            with tarfile.open(root / "signing-download/signing-input.tar", "w") as archive:
                member = tarfile.TarInfo(name)
                if link:
                    member.type = tarfile.SYMTYPE
                    member.linkname = link
                    archive.addfile(member)
                else:
                    member.size = 4
                    archive.addfile(member, io.BytesIO(b"test"))
            return subprocess.run([sys.executable, str(SCRIPT)], cwd=root,
                env=dict(os.environ, RUNNER_TEMP=str(root), PACKAGE_PLATFORM="macos"),
                capture_output=True).returncode

    def test_prepared_artifact(self):
        self.assertEqual(self.run_archive("target/package/Patina.app/file"), 0)

    def test_script_overwrite_refused(self):
        self.assertNotEqual(self.run_archive("tools/ci/package-macos.sh"), 0)

    def test_chained_link_escape_refused(self):
        # The tar data filter can normalize link text. Exercise the independent
        # post-extraction containment guard with an on-disk chained link.
        import importlib.util
        spec = importlib.util.spec_from_file_location("unpack", SCRIPT)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            expected = Path(directory) / "target/package"
            expected.mkdir(parents=True)
            (expected / "d").symlink_to(".")
            (expected / "link").symlink_to("d/d/../../tools")
            with self.assertRaises(ValueError):
                module.validate_artifact(expected)

    def test_link_escape_refused(self):
        self.assertNotEqual(self.run_archive("target/package/Patina.app/link", "../../../tools"), 0)

if __name__ == "__main__":
    unittest.main()

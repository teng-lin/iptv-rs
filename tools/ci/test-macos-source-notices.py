#!/usr/bin/env python3
"""Exercise missing-bottle notices without Homebrew or network access."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("bundle", Path(__file__).parents[1] / "bundle-macos.py")
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)


class SourceNotices(unittest.TestCase):
    def run_source(self, bad_hash=False, wrong_version=False):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:xz") as archive:
            for name in ["glib-2.90.1/LICENSES/LGPL.txt", "glib-2.90.1/../../LICENSE-escape"]:
                entry = tarfile.TarInfo(name)
                data = b"synthetic licence text"
                entry.size = len(data)
                archive.addfile(entry, io.BytesIO(data))
        payload = buffer.getvalue()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prefix = root / "glib/2.90.1"
            (prefix / ".brew").mkdir(parents=True)
            checksum = "0" * 64 if bad_hash else hashlib.sha256(payload).hexdigest()
            version = "2.90.0" if wrong_version else "2.90.1"
            (prefix / ".brew/glib.rb").write_text(
                f'url "https://download.gnome.org/sources/glib/2.90/glib-{version}.tar.xz"\n'
                f'sha256 "{checksum}"\n')
            destination = root / "notices"
            destination.mkdir()
            with patch.object(bundle.urllib.request, "urlopen", return_value=io.BytesIO(payload)):
                bundle.glib_source_notices(prefix, destination)
            self.assertEqual((destination / "source/LICENSES/LGPL.txt").read_text(), "synthetic licence text")
            self.assertFalse((root / "LICENSE-escape").exists())
            self.assertTrue((destination / "source.json").exists())

    def test_matching_source_preserves_notices(self):
        self.run_source()

    def test_mismatched_checksum_refused(self):
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            self.run_source(bad_hash=True)

    def test_mismatched_version_refused(self):
        with self.assertRaisesRegex(ValueError, "matching source URL"):
            self.run_source(wrong_version=True)


if __name__ == "__main__":
    unittest.main()

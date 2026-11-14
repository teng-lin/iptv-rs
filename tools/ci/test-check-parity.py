#!/usr/bin/env python3
"""Every platform's check script covers every workspace crate.

tools/check.sh (Linux and macOS) is the reference. The Windows check repeats its
crate lists, so a crate added to the workspace must not be silently tested on
one platform only.
"""
from pathlib import Path
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = re.compile(r"-p\s+([a-z0-9-]+)")


def lines_with(path, word):
    return [line for line in (ROOT / path).read_text().splitlines() if word in line and not line.lstrip().startswith("#")]


def packages(lines):
    return {name for line in lines for name in PACKAGE.findall(line)}


class Parity(unittest.TestCase):
    def setUp(self):
        manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
        self.members = {Path(member).name for member in manifest["workspace"]["members"]}
        self.libraries = self.members - {"iptv-app"}
        check = (ROOT / "tools/check.sh").read_text()
        self.wasm = set(re.search(r"^for c in ([a-z0-9 -]+); do$", check, re.M)[1].split())

    def test_reference_script_tests_and_formats_every_crate(self):
        self.assertEqual(packages(lines_with("tools/check.sh", "cargo test")), self.members)
        self.assertEqual(packages(lines_with("tools/check.sh", "cargo fmt")), self.members)
        self.assertLess(self.wasm, self.libraries)

    def test_windows_check_runs_the_same_crates(self):
        tested = packages(lines_with("tools/build-windows.ps1", "cargo test"))
        self.assertEqual(tested - {"nsh-platform-windows"}, self.members)
        script = (ROOT / "tools/build-windows.ps1").read_text()
        loop = re.search(r"foreach \(\$crate in @\(([^)]*)\)\)", script)[1]
        self.assertEqual(set(re.findall(r"'([a-z0-9-]+)'", loop)), self.wasm)
        self.assertIn("pins.py\" check-nsh", script)


if __name__ == "__main__":
    unittest.main()

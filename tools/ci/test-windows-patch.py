"""Exercise fresh, patched and incompatible checkouts in both Windows shells."""
import difflib
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


HELPER = Path(__file__).with_name("apply-windows-pointer-patch.ps1").resolve()


class PatchTests(unittest.TestCase):
    def test_line_endings_and_idempotence(self):
        before = "fn synthetic() {\n    old_action();\n}\n"
        after = before.replace("old_action", "new_action")
        diff = "".join(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
                                             fromfile="a/synthetic.rs", tofile="b/synthetic.rs"))
        for shell in ("powershell.exe", "pwsh.exe"):
            self.assertIsNotNone(shutil.which(shell), shell)
            for ending in ("\n", "\r\n"):
                with self.subTest(shell=shell, ending=repr(ending)), tempfile.TemporaryDirectory() as temporary:
                    root = Path(temporary)
                    subprocess.run(["git", "init", "-q", str(root)], check=True)
                    source, patch = root / "synthetic.rs", root / "fix.patch"
                    source.write_bytes(before.replace("\n", ending).encode())
                    patch.write_bytes(diff.replace("\n", ending).encode())
                    command = [shell, "-NoProfile", "-File", str(HELPER),
                               "-Repository", str(root), "-Patch", str(patch)]
                    for _ in range(2):
                        result = subprocess.run(command, capture_output=True, text=True)
                        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                        self.assertEqual(source.read_text(), after)
                    source.write_text("unrelated content\n")
                    result = subprocess.run(command, capture_output=True, text=True)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(source.read_text(), "unrelated content\n")


if __name__ == "__main__":
    unittest.main()

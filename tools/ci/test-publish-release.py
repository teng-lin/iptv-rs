#!/usr/bin/env python3
"""A public release is never written to; a draft is completed and published once."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("publish-release.py")
TAG = "v9.8.7"

# A stand-in for the GitHub CLI: one release whose state lives in a file, and a
# log of every call. `fail` and `publish_during_upload` inject the awkward cases.
STUB = r'''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
home = Path(os.environ["STUB_HOME"])
state_file = home / "state.json"
state = json.loads(state_file.read_text())
arguments = sys.argv[1:]
with (home / "calls.log").open("a") as log:
    log.write(json.dumps(arguments) + "\n")
if arguments[:2] != ["release", arguments[1]] or arguments[2] != "v9.8.7":
    sys.exit("unexpected gh call")
action = arguments[1]
if state.get("fail") == action:
    sys.exit("HTTP 502: synthetic outage")
if action == "view":
    if state["release"] == "absent":
        sys.exit("release not found")
    print(json.dumps({"isDraft": state["release"] == "draft", "tagName": "v9.8.7"}))
elif action == "create":
    if state["release"] != "absent" or "--draft" not in arguments or "--verify-tag" not in arguments:
        sys.exit("a release for this tag already exists")
    state["release"] = "draft"
elif action == "upload":
    if state["release"] == "absent":
        sys.exit("release not found")
    state["uploaded"] = sorted(Path(a).name for a in arguments[3:] if not a.startswith("--"))
    state["clobber"] = "--clobber" in arguments
    if state.get("publish_during_upload"):
        state["release"] = "public"
elif action == "edit":
    if arguments[3:] != ["--draft=false"] or state["release"] != "draft":
        sys.exit("unexpected edit")
    state["release"] = "public"
    state["published"] = state.get("published", 0) + 1
state_file.write_text(json.dumps(state))
'''


class Publishing(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.home = Path(temporary.name)
        self.dist = self.home / "dist"
        self.dist.mkdir()
        (self.dist / "Patina-9.8.7-synthetic.apk").write_bytes(b"synthetic package")
        (self.dist / "Patina-9.8.7-synthetic-sources.tar").write_bytes(b"synthetic sources")
        bin_directory = self.home / "bin"
        bin_directory.mkdir()
        (bin_directory / "gh").write_text(STUB)
        (bin_directory / "gh").chmod(0o755)
        self.environment = dict(os.environ, STUB_HOME=str(self.home),
                                PATH=str(bin_directory) + os.pathsep + os.environ["PATH"])

    def release(self, state, **extra):
        (self.home / "state.json").write_text(json.dumps({"release": state, **extra}))
        (self.home / "calls.log").write_text("")

    def run_script(self, command):
        return subprocess.run([sys.executable, str(SCRIPT), command, str(self.dist), TAG],
                              env=self.environment, capture_output=True, text=True)

    def state(self):
        return json.loads((self.home / "state.json").read_text())

    def actions(self):
        return [json.loads(line)[1] for line in (self.home / "calls.log").read_text().splitlines()]

    def checksums(self):
        self.release("absent")
        self.assertEqual(self.run_script("checksums").returncode, 0)

    def test_absent_release_is_created_as_a_draft_then_published_once(self):
        self.checksums()
        self.release("absent")
        result = self.run_script("publish")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.actions(), ["view", "create", "upload", "view", "edit"])
        state = self.state()
        self.assertEqual((state["release"], state["published"]), ("public", 1))
        self.assertEqual(state["uploaded"], ["Patina-9.8.7-synthetic-sources.tar", "Patina-9.8.7-synthetic.apk", "SHA256SUMS"])

    def test_interrupted_draft_is_completed_without_creating_again(self):
        self.checksums()
        self.release("draft")
        result = self.run_script("publish")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.actions(), ["view", "upload", "view", "edit"])
        self.assertTrue(self.state()["clobber"])

    def test_public_release_is_refused_before_any_write(self):
        self.checksums()
        for command in ("checksums", "publish"):
            with self.subTest(command=command):
                self.release("public")
                result = self.run_script(command)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("already public", result.stderr)
                self.assertEqual(self.actions(), ["view"])
                self.assertEqual(self.state(), {"release": "public"})

    def test_unreadable_state_is_not_mistaken_for_absent(self):
        self.checksums()
        self.release("public", fail="view")
        result = self.run_script("publish")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("synthetic outage", result.stderr)
        self.assertEqual(self.actions(), ["view"])

    def test_release_published_during_upload_is_not_published_again(self):
        self.checksums()
        self.release("draft", publish_during_upload=True)
        result = self.run_script("publish")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("stopped being a draft", result.stderr)
        self.assertNotIn("edit", self.actions())

    def test_failed_upload_leaves_the_draft_unpublished(self):
        self.checksums()
        self.release("absent", fail="upload")
        self.assertNotEqual(self.run_script("publish").returncode, 0)
        self.assertEqual(self.actions(), ["view", "create", "upload"])
        self.assertEqual(self.state()["release"], "draft")

    def test_files_changed_after_the_checksums_are_refused(self):
        self.checksums()
        for change in ("modify", "add", "remove"):
            with self.subTest(change=change):
                self.setUp()
                self.checksums()
                self.release("absent")
                if change == "modify":
                    (self.dist / "Patina-9.8.7-synthetic.apk").write_bytes(b"different bytes")
                elif change == "add":
                    (self.dist / "unlisted.bin").write_bytes(b"unlisted")
                else:
                    (self.dist / "Patina-9.8.7-synthetic.apk").unlink()
                result = self.run_script("publish")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("does not describe", result.stderr)
                self.assertEqual(self.actions(), ["view"])

    def test_checksum_file_is_what_sha256sum_checks(self):
        self.checksums()
        text = (self.dist / "SHA256SUMS").read_text()
        self.assertEqual([line.split("  ", 1)[1] for line in text.splitlines()],
                         ["Patina-9.8.7-synthetic-sources.tar", "Patina-9.8.7-synthetic.apk"])
        # Rewriting does not list the checksum file itself.
        self.release("absent")
        self.assertEqual(self.run_script("checksums").returncode, 0)
        self.assertEqual((self.dist / "SHA256SUMS").read_text(), text)
        if shutil.which("sha256sum"):
            subprocess.run(["sha256sum", "--check", "--strict", "SHA256SUMS"], cwd=self.dist,
                           check=True, capture_output=True)

    def test_empty_directory_and_nested_directories_are_refused(self):
        self.release("absent")
        (self.dist / "nested").mkdir()
        self.assertNotEqual(self.run_script("checksums").returncode, 0)
        shutil.rmtree(self.dist)
        self.dist.mkdir()
        result = self.run_script("checksums")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no files to release", result.stderr)


if __name__ == "__main__":
    unittest.main()

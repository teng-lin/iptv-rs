#!/usr/bin/env python3
"""Regression for runner browsers carrying private copies of system SONAMEs."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("notices", Path(__file__).with_name("linux-native-notices.py"))
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)


class Owners(unittest.TestCase):
    def test_private_browser_copies_do_not_own_system_vulkan(self):
        result = notices.installed_owners("""google-chrome-stable: /opt/google/chrome/libvulkan.so.1
microsoft-edge-stable: /opt/microsoft/msedge/libvulkan.so.1
libvulkan1:amd64: /usr/lib/x86_64-linux-gnu/libvulkan.so.1
libvulkan1:i386: /usr/lib/i386-linux-gnu/libvulkan.so.1
""", "amd64")
        self.assertEqual(result, {"libvulkan1:amd64"})

    def test_merged_usr_and_diversions(self):
        self.assertEqual(notices.installed_owners("""libsynthetic1:amd64: /lib/x86_64-linux-gnu/libsynthetic.so.1
diversion by synthetic: /usr/lib/x86_64-linux-gnu/libsynthetic.so.1
local diversion from: /usr/lib/x86_64-linux-gnu/libsynthetic.so.1
""", "amd64"), {"libsynthetic1:amd64"})

    def test_private_only_and_ambiguous_candidates_fail_closed(self):
        with self.assertRaisesRegex(SystemExit, "no installed system package"):
            notices.select_owner("private: /opt/example/libexample.so", "amd64", "libexample.so")
        with self.assertRaisesRegex(SystemExit, "ambiguous"):
            notices.select_owner("first: /usr/lib/one/libexample.so\nsecond: /usr/lib/two/libexample.so", "amd64", "libexample.so")

    def test_real_ambiguity_is_preserved_for_caller_to_reject(self):
        self.assertEqual(notices.installed_owners("""first: /usr/lib/one/libexample.so
second: /usr/lib/two/libexample.so
private: /usr/library/libexample.so
""", "amd64"), {"first", "second"})


class PinnedRuntime(unittest.TestCase):
    def test_runtime_library_is_left_to_its_own_notices(self):
        import json
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / "runtime.json").write_text(json.dumps({"library": "libmpv.so.2"}))
            self.assertEqual(notices.runtime_libraries(directory), {"libmpv.so.2"})
        self.assertEqual(notices.runtime_libraries(None), set())


unittest.main()

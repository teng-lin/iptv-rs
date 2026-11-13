"""Source dependency additions must invalidate a stale generated runner lock."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "package_lock", Path(__file__).with_name("check-package-lock.py"))
checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checks)


class SourceIdentities(unittest.TestCase):
    def test_missing_new_name_version_and_checksum_are_rejected(self):
        app = {"name": "synthetic-app", "version": "1.0.0"}
        dependency = {"name": "synthetic-resource-compiler", "version": "2.0.0",
                      "source": "registry+https://example.test/index", "checksum": "a" * 64}
        for packages in ([app], [app, dict(dependency, version="1.0.0")],
                         [app, dict(dependency, checksum="b" * 64)]):
            with self.subTest(packages=packages):
                with self.assertRaisesRegex(SystemExit, "synthetic-resource-compiler 2.0.0"):
                    checks.validate_source_identities(packages, [app, dependency])

    def test_runner_only_packages_and_extra_versions_are_allowed(self):
        dependency = {"name": "synthetic-dependency", "version": "2.0.0"}
        checks.validate_source_identities(
            [dependency, dict(dependency, version="1.0.0"),
             {"name": "synthetic-runner", "version": "1.0.0"}], [dependency])


if __name__ == "__main__":
    unittest.main()

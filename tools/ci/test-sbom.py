#!/usr/bin/env python3
"""The released SBOM carries no build paths and records the NSH commit."""
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("sbom", Path(__file__).with_name("sbom.py"))
sbom = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sbom)

ROOT = "/synthetic/build/patina"


def document(names=("iptv-core", "iptv-net", "iptv-store-sqlite", "nsh-core", "synthetic-dependency")):
    return json.dumps({
        "bomFormat": "CycloneDX",
        "metadata": {"component": {"bom-ref": f"path+file://{ROOT}/crates/iptv-app#1.2.3"}},
        "components": [{"name": name, "bom-ref": f"path+file:///synthetic/build/nsh/crates/{name}#1.0.0"}
                       for name in names],
    })


class Sbom(unittest.TestCase):
    def test_build_directories_become_repository_relative(self):
        result = json.loads(sbom.finish(document(), ROOT, "a" * 40))
        self.assertEqual(result["metadata"]["component"]["bom-ref"], "path+file://./crates/iptv-app#1.2.3")
        self.assertEqual(result["components"][3]["bom-ref"], "path+file://../nsh/crates/nsh-core#1.0.0")
        self.assertNotIn("/synthetic/build", json.dumps(result))

    def test_nsh_commit_and_scope_are_recorded(self):
        properties = json.loads(sbom.finish(document(), ROOT, "a" * 40))["metadata"]["properties"]
        self.assertIn({"name": "patina:nsh-revision", "value": "a" * 40}, properties)
        self.assertTrue(any(item["name"] == "patina:scope" for item in properties))

    def test_remaining_build_path_is_refused(self):
        text = document().replace('"bomFormat"', f'"note": "built in {ROOT}", "bomFormat"')
        with self.assertRaisesRegex(ValueError, "build directory"):
            sbom.finish(text, ROOT, "a" * 40)

    def test_empty_or_partial_graph_is_refused(self):
        with self.assertRaisesRegex(ValueError, "no component list"):
            sbom.finish(document(names=()), ROOT, "a" * 40)
        with self.assertRaisesRegex(ValueError, "nsh-core"):
            sbom.finish(document(names=("iptv-core", "iptv-net", "iptv-store-sqlite")), ROOT, "a" * 40)


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("notices", Path(__file__).with_name("crypto-native-notices.py"))
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)


class NativeNotices(unittest.TestCase):
    def test_keeps_attribution_and_license_without_source_code(self):
        block = "/* Copyright synthetic ZETETIC test fixture. Redistribution test terms. */"
        result = notices.sqlcipher_notices(block + "\nint private_code;\n" + block)
        self.assertEqual(result, block + "\n")

    def test_source_without_required_upstream_notice_is_rejected(self):
        with self.assertRaises(ValueError):
            notices.sqlcipher_notices("/* Synthetic irrelevant comment. */ int code;")


unittest.main()

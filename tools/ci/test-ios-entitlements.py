#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('ios', Path(__file__).with_name('ios-entitlements.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class EntitlementsTests(unittest.TestCase):
    def test_exact_and_wildcard_profiles(self):
        for allowed in ['example.synthetic', 'example.*', '*']:
            profile = {'Entitlements': {'application-identifier': 'SYNTHETIC.' + allowed, 'get-task-allow': False}}
            result = module.entitlements(profile, {'CFBundleIdentifier': 'example.synthetic'}, {'id': 'example.synthetic'})
            self.assertEqual(result['application-identifier'], 'SYNTHETIC.example.synthetic')
            self.assertFalse(result['get-task-allow'])

    def test_keychain_wildcards_specialize_without_altering_named_groups(self):
        profile = {'Entitlements': {'application-identifier': 'SYNTHETIC.*', 'keychain-access-groups': ['SYNTHETIC.*', 'SYNTHETIC.shared']}}
        result = module.entitlements(profile, {'CFBundleIdentifier': 'example.synthetic'}, {'id': 'example.synthetic'})
        self.assertEqual(result['keychain-access-groups'], ['SYNTHETIC.example.synthetic', 'SYNTHETIC.shared'])
        self.assertEqual(profile['Entitlements']['keychain-access-groups'][0], 'SYNTHETIC.*')
        profile['Entitlements']['keychain-access-groups'] = ['OTHER.*']
        with self.assertRaisesRegex(ValueError, 'Keychain wildcard'):
            module.entitlements(profile, {'CFBundleIdentifier': 'example.synthetic'}, {'id': 'example.synthetic'})

    def test_wrong_profile_or_unvalidated_custom_entitlement_refused(self):
        for allowed, config in [('other.app', {'id': 'example.synthetic'}), ('*', {'id': 'example.synthetic', 'ios': {'entitlements': {'example.capability': True}}})]:
            with self.assertRaises(ValueError):
                module.entitlements({'Entitlements': {'application-identifier': 'SYNTHETIC.' + allowed}}, {'CFBundleIdentifier': 'example.synthetic'}, config)


if __name__ == '__main__':
    unittest.main()

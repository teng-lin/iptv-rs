#!/usr/bin/env python3
"""Bind a provisioning profile to the already-built application identifier."""
import json
import plistlib
import sys
from pathlib import Path


def entitlements(profile, info, config):
    requested = config.get('ios', {}).get('entitlements', {})
    if requested:
        raise ValueError('Custom iOS entitlements need explicit profile validation before signing')
    result = dict(profile['Entitlements'])
    app_id = result['application-identifier']
    team, separator, allowed = app_id.partition('.')
    bundle = info['CFBundleIdentifier']
    if not separator or bundle != config['id'] or not (allowed == bundle or (allowed.endswith('*') and bundle.startswith(allowed[:-1]))):
        raise ValueError('Provisioning profile does not match the application')
    signed_id = team + '.' + bundle
    result['application-identifier'] = signed_id
    if 'keychain-access-groups' in result:
        groups = []
        for group in result['keychain-access-groups']:
            if '*' in group:
                if not group.endswith('*') or '*' in group[:-1] or not signed_id.startswith(group[:-1]):
                    raise ValueError('Keychain wildcard does not match the application')
                group = signed_id
            if group not in groups:
                groups.append(group)
        result['keychain-access-groups'] = groups
    return result


if __name__ == '__main__':
    profile, info, config, output = map(Path, sys.argv[1:])
    result = entitlements(plistlib.loads(profile.read_bytes()), plistlib.loads(info.read_bytes()), json.loads(config.read_text()))
    output.write_bytes(plistlib.dumps(result))

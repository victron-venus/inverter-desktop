"""Propagate the release version into iOS and verify the packaged artifact."""

import argparse
import json
from pathlib import Path
import plistlib
import re
import zipfile

VERSION_KEYS = ('CFBundleShortVersionString', 'CFBundleVersion')


def validate_version(version, config):
    """Reject malformed or inconsistent versions before changing build inputs."""
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError(f'Unsupported iOS version: {version}')
    if json.loads(config.read_text(encoding='utf-8'))['version'] != version:
        raise ValueError('iOS version does not match tauri.conf.json')


def sync_plist(path, version):
    """Update only version fields, preserving application identity and settings."""
    with path.open('rb') as source:
        info = plistlib.load(source)
    for key in VERSION_KEYS:
        info[key] = version
    with path.open('wb') as destination:
        plistlib.dump(info, destination, sort_keys=False)


def verify_ipa(path, version):
    """Require the actual packaged app to declare the requested release version."""
    with zipfile.ZipFile(path) as archive:
        manifests = [name for name in archive.namelist()
                     if name.startswith('Payload/') and name.count('/') == 2
                     and name.endswith('.app/Info.plist')]
        if len(manifests) != 1:
            raise ValueError('Expected exactly one application Info.plist in IPA')
        info = plistlib.loads(archive.read(manifests[0]))
        for key in VERSION_KEYS:
            if info.get(key) != version:
                raise ValueError(f'Packaged {key}={info.get(key)!r}; expected {version}')


def main():
    """Apply or verify versions using explicit artifact paths."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('sync', 'verify'))
    parser.add_argument('path', type=Path)
    parser.add_argument('--version', required=True)
    parser.add_argument('--config', type=Path, default=Path('src-tauri/tauri.conf.json'))
    args = parser.parse_args()
    validate_version(args.version, args.config)
    if args.mode == 'sync':
        sync_plist(args.path, args.version)
    else:
        verify_ipa(args.path, args.version)
    print(f'iOS {args.mode} version {args.version}: PASS')


if __name__ == '__main__':
    main()

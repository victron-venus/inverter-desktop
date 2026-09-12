"""Exercise real plist synchronization and packaged iOS version validation."""

import json
from pathlib import Path
import plistlib
import tempfile
import unittest
import zipfile

from ios_version import sync_plist, validate_version, verify_ipa


class IOSVersionTests(unittest.TestCase):
    """Keep package metadata checks independent from Xcode and signing."""

    def test_sync_preserves_identity_and_round_trips_packaged_plist(self):
        """A stale generated plist becomes a valid package without identity changes."""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'Info.plist'
            original = {'CFBundleShortVersionString': '1.0.0',
                        'CFBundleVersion': '1.0.0', 'CFBundleIdentifier': 'com.example.app',
                        'Custom': ['preserved']}
            path.write_bytes(plistlib.dumps(original))
            sync_plist(path, '2.5.39')
            info = plistlib.loads(path.read_bytes())
            self.assertEqual(info['CFBundleIdentifier'], original['CFBundleIdentifier'])
            self.assertEqual(info['Custom'], original['Custom'])
            archive_path = Path(directory) / 'with spaces.ipa'
            with zipfile.ZipFile(archive_path, 'w') as archive:
                archive.write(path, 'Payload/Inverter Desktop.app/Info.plist')
            verify_ipa(archive_path, '2.5.39')

    def test_stale_or_missing_version_fields_fail(self):
        """Neither marketing nor build version may retain an old value."""
        for info in ({'CFBundleShortVersionString': '1.0.0', 'CFBundleVersion': '2.5.39'},
                     {'CFBundleShortVersionString': '2.5.39', 'CFBundleVersion': '1.0.0'},
                     {'CFBundleShortVersionString': '2.5.39'}):
            with self.subTest(info=info), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'app.ipa'
                with zipfile.ZipFile(path, 'w') as archive:
                    archive.writestr('Payload/App.app/Info.plist', plistlib.dumps(info))
                with self.assertRaises(ValueError):
                    verify_ipa(path, '2.5.39')

    def test_missing_or_multiple_applications_fail(self):
        """Reject missing app metadata and ambiguous packages before publication."""
        for names in ([], ['Payload/A.app/Info.plist', 'Payload/B.app/Info.plist']):
            with self.subTest(names=names), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'app.ipa'
                with zipfile.ZipFile(path, 'w') as archive:
                    for name in names:
                        archive.writestr(name, plistlib.dumps({}))
                with self.assertRaises(ValueError):
                    verify_ipa(path, '2.5.39')

    def test_version_must_match_config(self):
        """Reject mismatched metadata and values unsuitable for an iOS release."""
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / 'tauri.conf.json'
            config.write_text(json.dumps({'version': '2.5.39'}), encoding='utf-8')
            validate_version('2.5.39', config)
            for version in ('2.5.38', '2.5.39-beta', 'invalid'):
                with self.subTest(version=version), self.assertRaises(ValueError):
                    validate_version(version, config)


if __name__ == '__main__':
    unittest.main()

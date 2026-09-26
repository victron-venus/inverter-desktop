"""Local packaging uses real staging with simulated compiler output."""

import hashlib
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

from test_frigate_package import executable_fixture

CHECKOUT = Path(__file__).resolve().parents[1]
SCRIPTS = CHECKOUT / "scripts"
SPEC = importlib.util.spec_from_file_location("local_plugins", SCRIPTS / "build-local-plugins.py")
local = importlib.util.module_from_spec(SPEC)
with patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
    SPEC.loader.exec_module(local)


class LocalPluginPackageTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.target = "aarch64-apple-darwin"
        for plugin in local.plugin_package.PLUGINS:
            manifest = self.root / "desktop-plugins" / plugin / "Cargo.toml"
            manifest.parent.mkdir(parents=True)
            manifest.write_bytes((CHECKOUT / manifest.relative_to(self.root)).read_bytes())

    def simulate(self, root, arguments):
        self.assertEqual(root, self.root)
        args = list(map(str, arguments))
        if args[0] == "cargo":
            self.assertIn("--locked", args)
            self.assertIn("--release", args)
            if "--bin" in args:
                binary = args[args.index("--bin") + 1]
                target = args[args.index("--target") + 1]
                worker = self.root / "src-tauri/target" / target / "release" / binary
                worker.parent.mkdir(parents=True, exist_ok=True)
                worker.write_bytes(executable_fixture(target))
            return
        self.assertEqual(Path(args[0]).name, "plugin-package")
        self.assertIn("--unsigned", args)
        self.assertNotIn("--key-file", args)
        manifest = Path(args[args.index("--manifest") + 1])
        archive = Path(args[args.index("--output") + 1])
        with zipfile.ZipFile(archive, "x") as output:
            output.writestr("manifest.json", manifest.read_bytes())

    def build(self):
        with patch.object(local, "run", side_effect=self.simulate):
            return local.build_plugins(self.root, self.target)

    def test_complete_local_artifacts_without_release_plan_or_configuration_changes(self):
        destination = self.build()
        inventory = json.loads((destination / "local-plugin-artifacts.json").read_text())
        self.assertEqual(inventory["target"], self.target)
        self.assertEqual(len(inventory["packages"]), 4)
        self.assertEqual(len(list(destination.iterdir())), 9)
        self.assertNotIn("desktop_plugins", inventory)
        self.assertFalse((self.root / ".release-plan.json").exists())
        for package in inventory["packages"]:
            archive = destination / package["file"]
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(package["sha256"], digest)
            self.assertEqual((destination / (package["file"] + ".sha256")).read_text(),
                             f"{digest}  {package['file']}\n")
            with zipfile.ZipFile(archive) as payload:
                manifest = json.loads(payload.read("manifest.json"))
            self.assertEqual(manifest["plugin_id"], package["plugin_id"])
            self.assertEqual(manifest["version"], package["version"])
            self.assertEqual(manifest["target"], self.target)

    def test_rebuild_and_late_failure_preserve_previous_complete_artifacts(self):
        first = self.build()
        original = {path.name: path.read_bytes() for path in first.iterdir()}
        second = self.build()
        self.assertNotEqual(first, second)
        manifest = self.root / "desktop-plugins/ring/Cargo.toml"
        manifest.write_text(manifest.read_text().replace('version = "0.1.0"', 'version = "99.0.0"'))
        with self.assertRaisesRegex(ValueError, "version does not match"):
            self.build()
        self.assertEqual(set(first.parent.iterdir()), {first, second})
        self.assertEqual({path.name: path.read_bytes() for path in first.iterdir()}, original)

    def test_build_failure_and_unsupported_host_fail_before_publication(self):
        with patch.object(local, "run", side_effect=subprocess.CalledProcessError(1, "cargo")):
            with self.assertRaises(subprocess.CalledProcessError):
                local.build_plugins(self.root, self.target)
        self.assertEqual(list((self.root / "target/local-plugins" / self.target).iterdir()), [])
        with patch.object(local.subprocess, "check_output", return_value="host: aarch64-apple-ios\n"):
            with self.assertRaisesRegex(ValueError, "supported desktop"):
                local.compiler_host(self.root)
        with patch.object(local, "run") as command:
            with self.assertRaises(ValueError):
                local.build_plugins(self.root, "--config=unexpected")
            command.assert_not_called()

    def app_data(self, inventory):
        app = self.root / 'app-data'
        (app / 'plugins').mkdir(parents=True)
        records = {p['plugin_id']: {'active': {'sha256': 'a' * 64, 'archive_pin': True},
                                    'enabled': False}
                   for p in inventory['packages'] if p['plugin_id'] != 'inverter-desktop.ring'}
        (app / 'plugins/state.json').write_text(json.dumps({'schema_version': 1, 'plugins': records}))
        (app / 'config.json').write_bytes(b'encrypted configuration')
        (app / 'plugins/settings.enc').write_bytes(b'encrypted credentials')
        return app

    def test_install_selects_only_existing_packages_preserves_state_and_release_baseline(self):
        artifacts = self.build()
        inventory = local.read_json(artifacts / 'local-plugin-artifacts.json')
        app = self.app_data(inventory)
        before = {p: p.read_bytes() for p in app.rglob('*') if p.is_file()}
        expected = local.stage_install(artifacts, app, self.target)
        self.assertEqual(len(expected), 3)
        value = local.read_json(app / local.SELECTION)
        for package in value['packages']:
            self.assertEqual(package['baseline_sha256'], 'a' * 64)
            archive = app / 'local-plugin-builds' / value['build'] / package['file']
            self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(), package['sha256'])
        self.assertEqual(before, {p: p.read_bytes() for p in before})
        state = local.read_json(app / 'plugins/state.json')
        for identity, digest in expected.items():
            state['plugins'][identity]['active']['sha256'] = digest
        (app / 'plugins/state.json').write_text(json.dumps(state))
        local.stage_install(artifacts, app, self.target)
        self.assertTrue(all(p['baseline_sha256'] == 'a' * 64
                            for p in local.read_json(app / local.SELECTION)['packages']))

    def test_bad_archive_does_not_replace_selection_and_failed_activation_restores_it(self):
        artifacts = self.build()
        inventory = local.read_json(artifacts / 'local-plugin-artifacts.json')
        app = self.app_data(inventory)
        local.stage_install(artifacts, app, self.target)
        previous = (app / local.SELECTION).read_bytes()
        with patch.object(local, 'restart_app') as restart, \
                patch.object(local, 'wait_installed', side_effect=ValueError('not activated')):
            with self.assertRaisesRegex(ValueError, 'not activated'):
                local.install_plugins(artifacts, app, self.target)
            self.assertEqual(restart.call_count, 2)
        self.assertEqual((app / local.SELECTION).read_bytes(), previous)
        (artifacts / inventory['packages'][-1]['file']).write_bytes(b'corrupt')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            local.stage_install(artifacts, app, self.target)
        self.assertEqual((app / local.SELECTION).read_bytes(), previous)

    def test_restore_release_waits_for_original_pins(self):
        artifacts = self.build()
        inventory = local.read_json(artifacts / 'local-plugin-artifacts.json')
        app = self.app_data(inventory)
        selected = local.stage_install(artifacts, app, self.target)
        state = local.read_json(app / 'plugins/state.json')
        for identity, digest in selected.items():
            state['plugins'][identity]['active']['sha256'] = digest
        (app / 'plugins/state.json').write_text(json.dumps(state))
        with patch.object(local, 'restart_app'), patch.object(local, 'wait_installed') as wait:
            local.install_plugins(None, app, self.target, restore=True)
        wait.assert_called_once_with(app, {identity: 'a' * 64 for identity in selected})
        self.assertFalse((app / local.SELECTION).exists())

    @unittest.skipIf(sys.platform == 'win32', 'macOS installer uses POSIX file locks')
    def test_installer_creates_lock_and_rejects_concurrent_invocations(self):
        import fcntl
        app = self.root / 'lock-app'
        app.mkdir()
        with patch.object(local, 'install_plugins') as install:
            local.install_macos(None, app, self.target, restore=True)
        install.assert_called_once_with(None, app, self.target, True)
        with (app / '.local-plugin-install.lock').open('rb') as handle:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(ValueError, 'in progress'):
                local.install_macos(None, app, self.target, restore=True)


if __name__ == "__main__":
    unittest.main()

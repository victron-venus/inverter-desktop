"""Local packaging uses real staging with simulated compiler output."""

import contextlib
import copy
import hashlib
import io
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

    def test_print_path_returns_one_complete_artifact_directory_without_a_result_file(self):
        output = io.StringIO()
        with patch.object(sys, 'argv', ['build-local-plugins.py', '--print-path']), \
                patch.object(local, 'ROOT', self.root), \
                patch.object(local, 'compiler_host', return_value=self.target), \
                patch.object(local, 'run', side_effect=self.simulate), \
                contextlib.redirect_stdout(output):
            local.main()
        lines = output.getvalue().splitlines()
        self.assertEqual(len(lines), 1)
        destination = Path(lines[0])
        self.assertTrue(destination.is_relative_to(self.root / 'target/local-plugins' / self.target))
        self.assertEqual(len(local.read_artifacts(destination, self.target)), 4)
        self.assertEqual(set(self.root.iterdir()), {
            self.root / 'desktop-plugins', self.root / 'src-tauri', self.root / 'target'})

    def test_build_command_stdout_is_routed_to_diagnostics(self):
        diagnostics = self.root / 'diagnostics.txt'
        captured = io.StringIO()
        with diagnostics.open('w+', encoding='utf-8') as stream, \
                contextlib.redirect_stderr(stream), contextlib.redirect_stdout(captured):
            local.run(self.root, [sys.executable, '-c', "print('compiler output')"])
        self.assertEqual(captured.getvalue(), '')
        self.assertEqual(diagnostics.read_text().strip(), 'compiler output')

    def test_arbitrary_output_path_is_rejected_before_any_build_or_write(self):
        protected = self.root / 'protected.txt'
        protected.write_text('must stay unchanged', encoding='utf-8')
        for argument in [str(protected), str(self.root / 'child/../protected.txt')]:
            with self.subTest(argument=argument), \
                    patch.object(sys, 'argv', ['build-local-plugins.py', '--output-path', argument]), \
                    patch.object(local, 'compiler_host') as compiler, \
                    contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as error:
                    local.main()
                self.assertEqual(error.exception.code, 2)
                compiler.assert_not_called()
                self.assertEqual(protected.read_text(), 'must stay unchanged')

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

    def test_malformed_inventory_never_changes_an_existing_selection(self):
        artifacts = self.build()
        manifest = artifacts / 'local-plugin-artifacts.json'
        inventory = local.read_json(manifest)
        app = self.app_data(inventory)
        local.stage_install(artifacts, app, self.target)
        before = (app / local.SELECTION).read_bytes()
        builds = set((app / 'local-plugin-builds').iterdir())
        mutations = [
            [],
            {**inventory, 'packages': {}},
            {**inventory, 'target': '../another-target'},
            {**inventory, 'packages': inventory['packages'][:-1]},
            {**inventory, 'packages': [inventory['packages'][0]] * 4},
        ]
        for field, value in [('version', '../escape'), ('version', '１.２.３'),
                             ('version', 1), ('sha256', ['a' * 64]),
                             ('file', '../../protected.txt'), ('plugin_id', '../plugin')]:
            mutated = copy.deepcopy(inventory)
            mutated['packages'][0][field] = value
            mutations.append(mutated)
        malformed = copy.deepcopy(inventory)
        malformed['packages'][0] = None
        mutations.append(malformed)
        for mutated in mutations:
            with self.subTest(inventory=mutated):
                manifest.write_text(json.dumps(mutated), encoding='utf-8')
                with self.assertRaises(ValueError):
                    local.stage_install(artifacts, app, self.target)
                self.assertEqual((app / local.SELECTION).read_bytes(), before)
                self.assertEqual(set((app / 'local-plugin-builds').iterdir()), builds)

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

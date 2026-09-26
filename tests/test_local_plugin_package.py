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


if __name__ == "__main__":
    unittest.main()

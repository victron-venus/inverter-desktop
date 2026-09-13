"""Exercise this application's version projections and packaged metadata checks."""

import importlib.util
import json
from pathlib import Path
import plistlib
import platform
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from ios_version import sync_plist, verify_ipa
from version_plan import (
    check_base_versions,
    create_plan,
    projected_value,
    sync_versions,
)

ROOT = Path(__file__).resolve().parents[1]


def script(name):
    """Load a hyphenated CLI entry point for direct contract checks."""
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ReleaseVersionTests(unittest.TestCase):
    """Verify application projections and the metadata inside real packages."""

    def setUp(self):
        """Copy the declared version files into an isolated fixture checkout."""
        # unittest owns cleanup across the full test, including assertion failures.
        # pylint: disable-next=consider-using-with
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.policy = json.loads((ROOT / ".release-policy.json").read_bytes())
        self.base = json.loads((ROOT / "package.json").read_bytes())["version"]
        self.build = self.policy["versioning"]["build_number_floor"] + 1
        for name in {item["path"] for item in self.policy["versioning"]["files"]}:
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, target)
        (self.root / ".release-policy.json").write_text(json.dumps(self.policy))
        self.sha = "a" * 40

    def plan(self, channel="beta", sequence=3, number=None):
        """Create a candidate above this application's migration counter floor."""
        return create_plan(
            self.base, channel, sequence, self.sha, self.policy, number or self.build
        )

    def test_all_owned_manifests_and_lock_match_candidate_but_native_base_is_numeric(
        self,
    ):
        """Keep full candidate identity while projecting native numeric fields."""
        check_base_versions(self.root, self.policy)
        plan = self.plan()
        sync_versions(self.root, self.policy, plan)
        sync_versions(self.root, self.policy, plan, check=True)
        config = json.loads((self.root / "src-tauri/tauri.conf.json").read_bytes())
        self.assertEqual(config["version"], self.base)
        self.assertEqual(
            config["bundle"]["iOS"]["bundleVersion"],
            projected_value(plan, {"value": "apple-build"}),
        )
        self.assertEqual(
            json.loads((self.root / "package.json").read_bytes())["version"],
            plan["version"],
        )
        self.assertIn(
            f'version = "{self.base}-beta.3"',
            (self.root / "src-tauri/Cargo.lock").read_text(),
        )

    def test_stable_requires_a_plan_matching_source_and_channel(self):
        """Reject absent or stale stable plans and accept the exact planned source."""
        checker = script("check-release-version")
        with self.assertRaisesRegex(ValueError, "verified"):
            checker.check_release_version(self.root, self.base, "stable")
        path = self.root / ".release-plan.json"
        path.write_text(json.dumps(self.plan("stable", None)))
        with patch.object(checker.subprocess, "check_output", return_value=self.sha):
            checker.check_release_version(self.root, self.base, "stable")
        with patch.object(checker.subprocess, "check_output", return_value="b" * 40):
            with self.assertRaises(ValueError):
                checker.check_release_version(self.root, self.base, "stable")

    def test_ios_package_keeps_numeric_versions_and_full_candidate_identity(self):
        """Inspect the plist and embedded identity in an actual IPA archive."""
        plan = self.plan()
        build = projected_value(plan, {"value": "apple-build"})
        plist = self.root / "Info.plist"
        plist.write_bytes(plistlib.dumps({"CFBundleExecutable": "App"}))
        sync_plist(plist, plan["base_version"], build, plan["version"])
        ipa = self.root / "App.ipa"
        with zipfile.ZipFile(ipa, "w") as archive:
            archive.write(plist, "Payload/App.app/Info.plist")
            archive.writestr(
                "Payload/App.app/App", json.dumps(plan, separators=(",", ":"))
            )
        verify_ipa(ipa, plan["base_version"], build, plan["version"])
        verifier = script("verify-native-release")
        verifier.verify_apple(ipa, plan)
        with self.assertRaises(ValueError):
            verifier.verify_apple(ipa, self.plan("rc", 1, self.build + 1))
        with self.assertRaises(ValueError):
            verifier.verify_embedded_identity(b"old binary", plan)

    @unittest.skipUnless(shutil.which("dpkg-deb"), "dpkg-deb is not installed")
    def test_real_debian_archive_rejects_stale_package_metadata(self):
        """Reject an actual Debian archive after its native version becomes stale."""
        plan = self.plan()
        package = self.root / "deb"
        (package / "DEBIAN").mkdir(parents=True)
        (package / "usr/bin").mkdir(parents=True)
        control = package / "DEBIAN/control"
        architecture = "amd64" if platform.machine() == "x86_64" else "arm64"
        control.write_text(
            f"Package: inverter-dashboard\nVersion: {self.base}\nArchitecture: {architecture}\n"
            "Maintainer: Inverter Desktop\nDescription: Version validation fixture\n"
        )
        (package / "usr/bin/inverter-dashboard").write_text(
            json.dumps(plan, separators=(",", ":"))
        )
        artifact = self.root / "fixture.deb"
        subprocess.run(
            ["dpkg-deb", "--build", str(package), str(artifact)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        verifier = script("verify-native-release")
        verifier.verify_linux(artifact, plan)
        control.write_text(
            control.read_text().replace(
                f"Version: {self.base}",
                "Version: 0.0.0" if self.base != "0.0.0" else "Version: 0.0.1",
            )
        )
        subprocess.run(
            ["dpkg-deb", "--build", str(package), str(artifact)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        with self.assertRaisesRegex(ValueError, "Linux package version"):
            verifier.verify_linux(artifact, plan)


if __name__ == "__main__":
    unittest.main()

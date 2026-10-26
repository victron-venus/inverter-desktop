"""Native signing and archive regressions; no application is ever launched."""

import importlib.util
import json
import platform
import plistlib
import shutil
import subprocess
import tempfile
import traceback
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "macos_release", ROOT / "scripts/macos_release.py"
)
macos_release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(macos_release)
ATTACH_SPEC = importlib.util.spec_from_file_location(
    "macos_fixture_attach", ROOT / "tests/macos_fixture_attach.py"
)
fixture_attach = importlib.util.module_from_spec(ATTACH_SPEC)
ATTACH_SPEC.loader.exec_module(fixture_attach)


def run_fixture_tool(arguments):
    """Keep failed native fixture commands fatal and expose bounded diagnostics."""
    try:
        return subprocess.run(
            arguments, check=True, capture_output=True, timeout=60
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        for stream in ("stdout", "stderr"):
            output = getattr(error, stream) or b""
            tail = output[-4096:]
            if isinstance(tail, bytes):
                tail = tail.decode("utf-8", errors="replace")
            prefix = (
                "[truncated; last 4096 bytes/characters] "
                if len(output) > 4096
                else ""
            )
            error.add_note(f"Native fixture {stream}: {prefix}{tail or '[empty]'}")
        raise


class FixtureToolDiagnosticsTests(unittest.TestCase):
    """Failure reports retain native causes without retries or exception changes."""

    def test_native_failures_keep_the_original_exception_and_visible_output(self):
        arguments = ["/usr/bin/hdiutil", "create", "fixture.dmg"]
        for error in (
            subprocess.CalledProcessError(
                1, arguments, output=b"progress", stderr=b"create failed: resource busy"
            ),
            subprocess.TimeoutExpired(
                arguments, 60, output="partial progress", stderr="still creating"
            ),
        ):
            with self.subTest(error=type(error).__name__):
                with patch.object(subprocess, "run", side_effect=error) as run:
                    with self.assertRaises(type(error)) as caught:
                        run_fixture_tool(arguments)
                self.assertIs(caught.exception, error)
                self.assertEqual(run.call_count, 1)
                rendered = "".join(traceback.format_exception(error))
                for stream in ("stdout", "stderr"):
                    output = getattr(error, stream)
                    if isinstance(output, bytes):
                        output = output.decode("utf-8")
                    self.assertIn(f"Native fixture {stream}: {output}", rendered)

    def test_diagnostics_bound_large_output_and_handle_empty_or_invalid_text(self):
        error = subprocess.TimeoutExpired(
            ["/usr/bin/hdiutil"], 60, output=b"x" * 10000 + b"\xfffinal cause"
        )
        with patch.object(subprocess, "run", side_effect=error):
            with self.assertRaises(subprocess.TimeoutExpired):
                run_fixture_tool(error.cmd)
        self.assertEqual(len(error.stdout), 10012)
        self.assertLess(len(error.__notes__[0]), 4200)
        self.assertIn("[truncated; last 4096 bytes/characters]", error.__notes__[0])
        self.assertTrue(error.__notes__[0].endswith("\ufffdfinal cause"))
        self.assertEqual(error.__notes__[1], "Native fixture stderr: [empty]")


class MacOSSigningConfigurationTests(unittest.TestCase):
    """The default release profile seals the app without paid credentials."""

    def test_ad_hoc_identity_is_explicit(self):
        """A linker-only executable signature must not become the release seal."""
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_bytes())
        self.assertEqual(config["bundle"]["macOS"]["signingIdentity"], "-")


@unittest.skipUnless(platform.system() == "Darwin", "native macOS tools required")
class MacOSReleasePackageTests(unittest.TestCase):
    """Exercise real codesign, ditto and disk images with a tiny inert Mach-O."""

    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.app = self.root / "Inverter Desktop.app"
        contents = self.app / "Contents"
        (contents / "MacOS").mkdir(parents=True)
        (contents / "Resources").mkdir()
        self.resource = contents / "Resources/example.txt"
        self.resource.write_text("sealed resource")
        self.plist = contents / "Info.plist"
        self.plist.write_bytes(
            plistlib.dumps(
                {
                    "CFBundleIdentifier": macos_release.BUNDLE_ID,
                    "CFBundleExecutable": "inverter-dashboard",
                    "CFBundlePackageType": "APPL",
                    "CFBundleShortVersionString": "2.5.52",
                    "CFBundleVersion": "201.52.41",
                }
            )
        )
        source = self.root / "fixture.c"
        source.write_text("int main(void) { return 0; }\n")
        subprocess.run(
            [
                "/usr/bin/clang",
                str(source),
                "-o",
                str(contents / "MacOS/inverter-dashboard"),
            ],
            check=True,
            capture_output=True,
            timeout=60,
        )

    def sign(self, app=None):
        """Seal only this test fixture, never a production release artifact."""
        subprocess.run(
            ["/usr/bin/codesign", "--force", "--sign", "-", str(app or self.app)],
            check=True,
            capture_output=True,
            timeout=60,
        )

    def archive(self):
        """Use the same ZIP writer as the release collector."""
        archive = self.root / "fixture.app.zip"
        subprocess.run(
            [
                "/usr/bin/ditto",
                "-c",
                "-k",
                "--sequesterRsrc",
                "--keepParent",
                str(self.app),
                str(archive),
            ],
            check=True,
            capture_output=True,
            timeout=60,
        )
        return archive

    def disk_image(self, app):
        """Create a small real DMG containing the selected fixture app."""
        image = self.root / "fixture.dmg"
        run_fixture_tool(
            [
                "/usr/bin/hdiutil",
                "create",
                "-srcfolder",
                str(app),
                "-volname",
                "Inverter fixture",
                "-format",
                "UDZO",
                str(image),
            ]
        )
        self.enterContext(
            patch.object(
                macos_release,
                "subprocess",
                fixture_attach.FixtureSubprocess(self.root, image),
            )
        )
        return image

    def assert_fixture_released(self, mounts, error=None):
        """An expected payload rejection must not conceal a cleanup failure."""
        if error is not None:
            self.assertFalse(
                getattr(error, "__notes__", []), "Unexpected verification cleanup error"
            )
        for mount in mounts:
            self.assertFalse(mount.is_mount(), "Fixture volume is still mounted")
            self.assertFalse(mount.parent.exists(), "Fixture mount directory remains")

    def cleanup_fixture_mounts(self, mounts, run):
        """Bound emergency cleanup and never delete through a remaining mount."""
        for mount in mounts:
            if mount.is_mount():
                run(["/usr/bin/hdiutil", "detach", str(mount)], check=True, timeout=60)
            self.assertFalse(mount.is_mount(), "Refusing to delete a mounted fixture")
            shutil.rmtree(mount.parent, ignore_errors=True)

    def test_linker_signature_is_rejected(self):
        """Reproduce the original incomplete bundle signature failure."""
        with self.assertRaises(subprocess.CalledProcessError):
            macos_release.verify_bundle(self.app)

    def test_sealed_bundle_and_zip_pass(self):
        """A full ad-hoc seal and its exact ZIP payload pass verification."""
        self.sign()
        macos_release.verify_bundle(self.app)
        macos_release.verify_zip(self.archive(), self.app)

    def test_resource_mutation_is_rejected(self):
        """Native verification rejects changed resource bytes after signing."""
        self.sign()
        self.resource.write_text("changed after signing")
        with self.assertRaises(subprocess.CalledProcessError):
            macos_release.verify_bundle(self.app)

    def test_plist_mutation_is_rejected(self):
        """Native verification binds plist metadata, including the version."""
        self.sign()
        info = plistlib.loads(self.plist.read_bytes())
        info["CFBundleVersion"] = "999.1.1"
        self.plist.write_bytes(plistlib.dumps(info))
        with self.assertRaises(subprocess.CalledProcessError):
            macos_release.verify_bundle(self.app)

    def test_zip_rejects_unexpected_member(self):
        """The archive cannot add files absent from the verified source app."""
        self.sign()
        archive = self.archive()
        with zipfile.ZipFile(archive, "a") as package:
            package.writestr("Inverter Desktop.app/Contents/extra", b"unexpected")
        with self.assertRaisesRegex(ValueError, "Unexpected"):
            macos_release.verify_zip(archive, self.app)

    def test_zip_rejects_missing_seal(self):
        """A valid source app cannot conceal an archive missing its seal."""
        self.sign()
        original = self.archive()
        truncated = self.root / "missing-seal.app.zip"
        with (
            zipfile.ZipFile(original) as source,
            zipfile.ZipFile(truncated, "w") as target,
        ):
            for item in source.infolist():
                if "/_CodeSignature/" not in item.filename:
                    target.writestr(item, source.read(item))
        with self.assertRaisesRegex(ValueError, "differs"):
            macos_release.verify_zip(truncated, self.app)

    def test_sealed_disk_image_passes(self):
        """The read-only mounted DMG contains the exact sealed app."""
        self.sign()
        macos_release.verify_dmg(self.disk_image(self.app), self.app)

    def test_different_sealed_disk_image_is_rejected(self):
        """A separately valid signature cannot hide a substituted payload."""
        self.assert_different_sealed_image_rejected(inject_eagain=False)

    def test_injected_eagain_still_rejects_different_sealed_disk_image(self):
        """Inject one EAGAIN; real retry and verification must reject other bytes."""
        self.assert_different_sealed_image_rejected(inject_eagain=True)

    def assert_different_sealed_image_rejected(self, *, inject_eagain):
        self.sign()
        other = self.root / "other/Inverter Desktop.app"
        shutil.copytree(self.app, other)
        (other / "Contents/Resources/example.txt").write_text("different valid app")
        self.sign(other)
        image = self.disk_image(other)
        proxy = macos_release.subprocess
        raw_run = proxy.runner
        real_run = macos_release.subprocess.run
        mounts = []
        activity = {"attach": 0, "info": 0, "injected": 0}

        def inject_first_attach(arguments, **kwargs):
            if arguments[:2] == ["/usr/bin/hdiutil", "attach"]:
                activity["attach"] += 1
                if inject_eagain and activity["attach"] == 1:
                    activity["injected"] += 1
                    raise subprocess.CalledProcessError(
                        1, arguments, stderr=fixture_attach.EAGAIN + b"\n"
                    )
            if arguments == ["/usr/bin/hdiutil", "info", "-plist"]:
                activity["info"] += 1
            return raw_run(arguments, **kwargs)

        def record_attach(arguments, **kwargs):
            if arguments[:2] == ["/usr/bin/hdiutil", "attach"]:
                mounts.append(Path(arguments[arguments.index("-mountpoint") + 1]))
            return real_run(arguments, **kwargs)

        try:
            with (
                patch.object(proxy, "runner", inject_first_attach),
                patch.object(macos_release.subprocess, "run", record_attach),
                self.assertRaisesRegex(ValueError, "differs") as caught,
            ):
                macos_release.verify_dmg(image, self.app)
            self.assertEqual(len(mounts), 1)
            self.assert_fixture_released(mounts, caught.exception)
            if inject_eagain:
                self.assertEqual(activity["injected"], 1)
                self.assertGreaterEqual(activity["attach"], 2)
                self.assertLessEqual(activity["attach"], 3)
                self.assertEqual(activity["info"], 2 * (activity["attach"] - 1))
        finally:
            self.cleanup_fixture_mounts(mounts, real_run)

    def test_attach_failure_after_real_mount_releases_owned_volume(self):
        """A failed or timed-out attach still cleans a mount it established."""
        self.sign()
        image = self.disk_image(self.app)
        real_run = macos_release.subprocess.run
        for error_type in (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            mounts = []

            def fail_after_attach(
                arguments, _mounts=mounts, _error_type=error_type, **kwargs
            ):
                result = real_run(arguments, **kwargs)
                if arguments[:2] == ["/usr/bin/hdiutil", "attach"]:
                    _mounts.append(Path(arguments[arguments.index("-mountpoint") + 1]))
                    if _error_type is subprocess.TimeoutExpired:
                        raise subprocess.TimeoutExpired(arguments, 60)
                    raise subprocess.CalledProcessError(1, arguments)
                return result

            with self.subTest(error=error_type.__name__):
                try:
                    with (
                        patch.object(
                            macos_release.subprocess, "run", fail_after_attach
                        ),
                        self.assertRaises(error_type),
                    ):
                        macos_release.verify_dmg(image, self.app)
                    self.assertEqual(len(mounts), 1)
                    self.assertFalse(mounts[0].is_mount())
                    self.assertFalse(mounts[0].parent.exists())
                finally:
                    self.cleanup_fixture_mounts(mounts, real_run)

    def test_detach_failure_preserves_primary_error_and_owned_mount(self):
        """A cleanup failure must not hide the verification failure or erase a mount."""
        self.sign()
        image = self.disk_image(self.app)
        real_run = macos_release.subprocess.run
        mounts = []

        def fail_detach(arguments, **kwargs):
            if arguments[:2] == ["/usr/bin/hdiutil", "detach"]:
                raise subprocess.CalledProcessError(1, arguments)
            result = real_run(arguments, **kwargs)
            if arguments[:2] == ["/usr/bin/hdiutil", "attach"]:
                mounts.append(Path(arguments[arguments.index("-mountpoint") + 1]))
            return result

        try:
            with (
                patch.object(macos_release.subprocess, "run", fail_detach),
                patch.object(
                    macos_release, "app_files", side_effect=ValueError("primary")
                ),
                self.assertRaisesRegex(ValueError, "primary") as caught,
            ):
                macos_release.verify_dmg(image, self.app)
            self.assertIn("Cleanup of owned mount", caught.exception.__notes__[0])
            self.assertEqual(len(mounts), 1)
            self.assertTrue(mounts[0].is_mount())
            self.assertTrue(mounts[0].parent.exists())
            with self.assertRaisesRegex(
                AssertionError, "Unexpected verification cleanup"
            ):
                self.assert_fixture_released(mounts, caught.exception)
            with self.assertRaisesRegex(AssertionError, "still mounted"):
                self.assert_fixture_released(mounts)
            with self.assertRaisesRegex(AssertionError, "Refusing to delete"):
                self.cleanup_fixture_mounts(mounts, lambda *_args, **_kwargs: None)
            self.assertTrue(mounts[0].parent.exists())
        finally:
            self.cleanup_fixture_mounts(mounts, real_run)


if __name__ == "__main__":
    unittest.main()

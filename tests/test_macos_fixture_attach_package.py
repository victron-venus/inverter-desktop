"""Portable fake-process checks; no native command or application is executed."""

import importlib.util
import io
import os
import plistlib
import stat
import subprocess
import tempfile
import traceback
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "fixture_attach_tests", Path(__file__).with_name("macos_fixture_attach.py")
)
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)


class FixtureAttachRetryTests(unittest.TestCase):
    """Only exact EAGAIN on an unchanged owned fixture permits another attach."""

    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.parent = Path(
            self.enterContext(tempfile.TemporaryDirectory(prefix="inverter-dmg-check-"))
        ).resolve()
        self.mount = self.parent / "volume"
        self.mount.mkdir()
        self.image = self.root / "fixture.dmg"
        self.image.write_bytes(b"inert test image")
        self.arguments = fixture.ATTACH_PREFIX + [str(self.mount), str(self.image)]
        self.options = {"check": True, "timeout": 60, "stdout": subprocess.DEVNULL}
        self.now, self.sleeps, self.calls = 0.0, [], []
        self.failures, self.registries = [], []
        self.attach_durations, self.info_durations = [], []
        self.after_attach = None
        self.after_info = None
        self.disk_infos, self.disk_info_durations = [], []
        self.after_disk_info = None
        self.log = io.StringIO()
        self.proxy = fixture.FixtureSubprocess(
            self.root, self.image, runner=self.run_fake,
            clock=lambda: self.now, sleep=self.sleep, log=self.log,
        )

    def sleep(self, duration):
        self.sleeps.append(duration)
        self.now += duration

    def error(self, stderr=fixture.EAGAIN + b"\n", returncode=1):
        return subprocess.CalledProcessError(returncode, self.arguments, stderr=stderr)

    def run_fake(self, arguments, **options):
        self.calls.append((list(arguments), dict(options), self.now))
        if arguments[:3] == ["/usr/sbin/diskutil", "info", "-plist"]:
            self.now += self.disk_info_durations.pop(0) if self.disk_info_durations else 0.01
            value = (self.disk_infos.pop(0) if self.disk_infos
                     else self.apfs_disk_info(arguments[-1]))
            if isinstance(value, BaseException):
                raise value
            options["stdout"].write(plistlib.dumps(value))
            options["stdout"].flush()
            if self.after_disk_info is not None:
                self.after_disk_info()
            return subprocess.CompletedProcess(arguments, 0)
        if arguments == ["/usr/bin/hdiutil", "info", "-plist"]:
            self.now += self.info_durations.pop(0) if self.info_durations else 0.01
            value = self.registries.pop(0) if self.registries else {"images": []}
            if isinstance(value, BaseException):
                raise value
            options["stdout"].write(value if isinstance(value, bytes) else plistlib.dumps(value))
            options["stdout"].flush()
            if self.after_info is not None:
                self.after_info()
            return subprocess.CompletedProcess(arguments, 0)
        self.now += self.attach_durations.pop(0) if self.attach_durations else 0.01
        if self.after_attach is not None:
            callback, self.after_attach = self.after_attach, None
            callback()
        failure = self.failures.pop(0) if self.failures else None
        if failure is not None:
            raise failure
        return subprocess.CompletedProcess(arguments, 0, stderr=b"")

    def calls_for(self, operation):
        return [row for row in self.calls if row[0][1] == operation]

    def invoke(self):
        return self.proxy.run(self.arguments, **self.options)

    def test_success_has_one_call_and_no_registry_or_delay(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0)
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(self.sleeps, [])

    def test_exact_error_recovers_once_and_logs_original_failure(self):
        failure = self.error()
        self.failures = [failure]
        self.assertEqual(self.invoke().returncode, 0)
        self.assertEqual(len(self.calls_for("attach")), 2)
        self.assertEqual(len(self.calls_for("info")), 2)
        self.assertEqual(self.sleeps, [0.2])
        self.assertIn("Resource temporarily unavailable", self.log.getvalue())
        self.assertIn("recovered on attempt 2", self.log.getvalue())
        self.assertEqual(failure.stderr, fixture.EAGAIN + b"\n")
        for args, options, at in self.calls:
            self.assertLessEqual(options["timeout"], 60 - at)
            if args[1] == "attach":
                self.assertEqual(args, self.arguments)
                self.assertTrue(options["check"])
                self.assertEqual(options["stdout"], subprocess.DEVNULL)

    def test_three_failures_stop_and_every_attempt_remains_visible(self):
        failures = [self.error() for _ in range(3)]
        self.failures = failures.copy()
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.invoke()
        self.assertIs(caught.exception, failures[-1])
        self.assertEqual(len(self.calls_for("attach")), 3)
        self.assertEqual(len(self.calls_for("info")), 4)
        self.assertEqual(self.log.getvalue().count("Resource temporarily unavailable"), 3)
        self.assertIn("3 attempts", caught.exception.__notes__[0])

    def test_recovery_on_third_attempt_keeps_both_failures_visible(self):
        self.failures = [self.error(), self.error()]
        self.assertEqual(self.invoke().returncode, 0)
        self.assertEqual(len(self.calls_for("attach")), 3)
        self.assertEqual(len(self.calls_for("info")), 4)
        self.assertEqual(self.sleeps, [0.2, 0.4])
        self.assertEqual(self.log.getvalue().count("Resource temporarily unavailable"), 2)
        self.assertIn("recovered on attempt 3", self.log.getvalue())

    def test_exact_text_and_crlf_errors_allow_recovery_with_unrelated_valid_image(self):
        registry = {"images": [{"image-path": str(self.root / "unrelated.dmg"),
                                "system-entities": [{"dev-entry": "/dev/disk99"}]}]}
        for stderr in (fixture.EAGAIN.decode(), fixture.EAGAIN + b"\r\n"):
            with self.subTest(stderr=stderr):
                self.calls = []
                self.failures, self.registries = [self.error(stderr)], [registry, registry]
                self.assertEqual(self.invoke().returncode, 0)
                self.assertEqual(len(self.calls_for("attach")), 2)
                self.assertNotIn("unrelated.dmg", self.log.getvalue())

    def test_unrelated_errors_and_timeout_never_retry(self):
        failures = [self.error(b"other failure"), self.error(returncode=2),
                    self.error(b" " + fixture.EAGAIN), self.error(fixture.EAGAIN + b"\nextra"),
                    subprocess.TimeoutExpired(self.arguments, 60, stderr=fixture.EAGAIN)]
        for failure in failures:
            with self.subTest(failure=repr(failure)):
                self.calls, self.sleeps, self.failures = [], [], [failure]
                with self.assertRaises(type(failure)) as caught:
                    self.invoke()
                self.assertIs(caught.exception, failure)
                self.assertEqual(len(self.calls), 1)
                self.assertEqual(self.sleeps, [])

    def test_later_unrelated_failure_is_not_replaced_by_first_error(self):
        final = self.error(b"disk image not recognized")
        self.failures = [self.error(), final]
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.invoke()
        self.assertIs(caught.exception, final)
        self.assertEqual(len(self.calls_for("attach")), 2)

    def test_shared_deadline_includes_attach_registry_and_backoff(self):
        for attach_duration, info_durations, infos, sleeps in (
            (60, [], 0, []), (58.9, [1], 1, []), (59.6, [0.01, 0.3], 2, [0.2])
        ):
            with self.subTest(attach_duration=attach_duration):
                self.now, self.calls, self.sleeps = 0.0, [], []
                failure = self.error()
                self.failures = [failure]
                self.attach_durations = [attach_duration]
                self.info_durations = info_durations.copy()
                with self.assertRaises(subprocess.CalledProcessError) as caught:
                    self.invoke()
                self.assertIs(caught.exception, failure)
                self.assertEqual(len(self.calls_for("attach")), 1)
                self.assertEqual(len(self.calls_for("info")), infos)
                self.assertEqual(self.sleeps, sleeps)

    def test_registry_errors_and_malformed_or_ambiguous_state_stop_original_error(self):
        other = str(self.root / "unrelated.dmg")
        entity = {"dev-entry": "/dev/disk99"}
        valid_other = {"image-path": other, "system-entities": [entity]}
        duplicate_device = {
            "image-path": str(self.root / "other.dmg"), "system-entities": [entity]
        }
        cases = [
            subprocess.CalledProcessError(1, ["info"]),
            subprocess.TimeoutExpired(["info"], 5),
            b"not a plist", b"x" * (fixture.MAX_REGISTRY + 1), {}, {"images": {}},
            {"images": [{}]}, {"images": [{"image-path": other}]},
            {"images": [{"image-path": other, "system-entities": [{}]}]},
            {"images": [valid_other, valid_other]},
            {"images": [valid_other, duplicate_device]},
            b'<plist><dict><key>images</key><array/><key>images</key><array/></dict></plist>',
        ]
        for registry in cases:
            with self.subTest(registry=type(registry).__name__):
                self.calls, self.sleeps = [], []
                failure = self.error()
                self.failures, self.registries = [failure], [registry]
                with self.assertRaises(subprocess.CalledProcessError) as caught:
                    self.invoke()
                self.assertIs(caught.exception, failure)
                self.assertEqual(len(self.calls_for("attach")), 1)
                self.assertEqual(self.sleeps, [])
                self.assertIn("retry refused", caught.exception.__notes__[0])
                self.assertNotIn(other, self.log.getvalue())

    def test_registry_exception_does_not_expose_unrelated_path(self):
        failure = self.error()
        self.failures = [failure]
        private_path = "/unrelated/private-image.dmg"
        self.registries = [OSError(2, "image disappeared", private_path)]
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.invoke()
        self.assertIs(caught.exception, failure)
        self.assertEqual(len(self.calls_for("attach")), 1)
        rendered = "".join(traceback.format_exception(caught.exception))
        self.assertNotIn(private_path, rendered + self.log.getvalue())
        self.assertIn("retry refused: FileNotFoundError", rendered)

    def test_owned_image_alias_or_registered_mount_prevents_retry(self):
        alias = str(self.root / "nested" / ".." / "fixture.dmg")
        cases = [
            {"images": [{"image-path": alias, "system-entities": []}]},
            {"images": [{"image-path": str(self.root / "other.dmg"), "system-entities": [
                {"dev-entry": "/dev/disk99", "mount-point": str(self.mount)}]}]},
        ]
        for registry in cases:
            with self.subTest(registry=registry):
                self.calls = []
                failure = self.error()
                self.failures, self.registries = [failure], [registry]
                with self.assertRaises(subprocess.CalledProcessError) as caught:
                    self.invoke()
                self.assertIs(caught.exception, failure)
                self.assertEqual(len(self.calls_for("attach")), 1)

    def test_new_partial_attachment_during_backoff_stops_before_retry(self):
        failure = self.error()
        self.failures = [failure]
        self.registries = [{"images": []}, {"images": [
            {"image-path": str(self.image), "system-entities": []}]}]
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.invoke()
        self.assertIs(caught.exception, failure)
        self.assertEqual(len(self.calls_for("attach")), 1)
        self.assertEqual(self.sleeps, [0.2])

    def test_image_replaced_during_second_info_stops_before_retry(self):
        failure = self.error()
        self.failures = [failure]

        def replace_on_second_info():
            if len(self.calls_for("info")) == 2:
                self.image.write_bytes(b"replaced during registry read")

        self.after_info = replace_on_second_info
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.invoke()
        self.assertIs(caught.exception, failure)
        self.assertEqual(len(self.calls_for("attach")), 1)
        self.assertEqual(len(self.calls_for("info")), 2)
        self.assertIn("changed during registry", caught.exception.__notes__[0])

    def test_late_success_cannot_exceed_the_shared_deadline(self):
        self.attach_durations = [60.01]
        with self.assertRaises(subprocess.TimeoutExpired) as caught:
            self.invoke()
        self.assertEqual(caught.exception.cmd, self.arguments)
        self.assertEqual(caught.exception.timeout, 60)
        self.assertIn("beyond the shared deadline", caught.exception.__notes__[0])
        self.assertEqual(len(self.calls), 1)

    def test_mounted_flag_after_failed_attach_stops_without_registry(self):
        failure = self.error()
        self.failures = [failure]
        original = Path.is_mount
        def is_mounted(path):
            return (path == self.mount and bool(self.calls)) or original(path)
        with patch.object(Path, "is_mount", is_mounted):
            with self.assertRaises(subprocess.CalledProcessError) as caught:
                self.invoke()
        self.assertIs(caught.exception, failure)
        self.assertEqual(len(self.calls), 1)

    def test_partial_mount_or_changed_image_prevents_registry_and_retry(self):
        for change in (lambda: (self.mount / "partial").write_bytes(b"partial"),
                       lambda: self.image.write_bytes(b"replaced image bytes")):
            with self.subTest(change=change):
                self.calls = []
                self.proxy = fixture.FixtureSubprocess(
                    self.root, self.image, runner=self.run_fake,
                    clock=lambda: self.now, sleep=self.sleep, log=self.log,
                )
                failure = self.error()
                self.failures, self.after_attach = [failure], change
                with self.assertRaises(subprocess.CalledProcessError) as caught:
                    self.invoke()
                self.assertIs(caught.exception, failure)
                self.assertEqual(len(self.calls), 1)
                (self.mount / "partial").unlink(missing_ok=True)

    def test_non_owned_or_non_attach_calls_are_forwarded_once_unchanged(self):
        cases = [
            (fixture.ATTACH_PREFIX + [str(self.mount), str(self.root / "other.dmg")],
             self.options),
            (fixture.ATTACH_PREFIX + [str(self.root / "outside"), str(self.image)],
             self.options),
            (["/usr/bin/hdiutil", "create", str(self.image)], self.options),
            (["/usr/bin/hdiutil", "detach", str(self.mount)], self.options),
            (["/usr/bin/codesign", "--verify", "inert.app"], self.options),
            (self.arguments, {**self.options, "timeout": 30}),
        ]
        for arguments, options in cases:
            with self.subTest(arguments=arguments):
                self.calls = []
                failure = self.error()
                self.failures = [failure]
                with self.assertRaises(subprocess.CalledProcessError) as caught:
                    self.proxy.run(arguments, **options)
                self.assertIs(caught.exception, failure)
                self.assertEqual(len(self.calls), 1)
                self.assertEqual(self.calls[0][:2], (arguments, options))

    def owned_registry(self, *, mounted=False):
        entity = {"dev-entry": "/dev/disk99s1"}
        if mounted:
            entity["mount-point"] = str(self.mount)
        return {"images": [{"image-path": str(self.image), "system-entities": [
            {"dev-entry": "/dev/disk99"}, entity]}]}

    def apfs_registry(self):
        return {"images": [{"image-path": str(self.image), "system-entities": [
            {"dev-entry": "/dev/disk99", "content-hint": "GUID_partition_scheme"},
            {"dev-entry": "/dev/disk99s1", "content-hint": "7C3457EF-0000-11AA-AA11-00306543ECAC"},
            {"dev-entry": "/dev/disk100", "content-hint": "EF57347C-0000-11AA-AA11-00306543ECAC"},
            {"dev-entry": "/dev/disk100s1", "content-hint": "41504653-0000-11AA-AA11-00306543ECAC"},
        ]}]}

    def apfs_disk_info(self, device):
        identifier = device.removeprefix("/dev/")
        result = {"DeviceNode": device, "DeviceIdentifier": identifier,
                  "ParentWholeDisk": identifier, "VirtualOrPhysical": "Virtual",
                  "BusProtocol": "Disk Image", "Content": "GUID_partition_scheme"}
        if device == "/dev/disk100":
            result.update(Content="EF57347C-0000-11AA-AA11-00306543ECAC",
                          APFSContainerReference=identifier,
                          APFSPhysicalStores=[{"APFSPhysicalStore": "disk99s1"}])
        return result

    def test_partial_apfs_cleanup_uses_verified_backing_not_synthesized_container(self):
        self.registries = [self.apfs_registry(), self.apfs_registry(), {"images": []}]
        self.proxy.cleanup_owned_image()
        self.assertEqual([c[0] for c in self.calls_for("detach")],
                         [["/usr/bin/hdiutil", "detach", "/dev/disk99"]])
        self.assertTrue(self.proxy.cleanup_detach_attempted)
        self.assertFalse(self.proxy.recovery_detach_attempted)

    def test_eagain_partial_apfs_recovers_once_within_original_deadline(self):
        failure = self.error()
        self.failures = [failure]
        self.registries = [self.apfs_registry()] * 3 + [{"images": []}] * 2
        self.assertEqual(self.invoke().returncode, 0)
        self.assertEqual(len(self.calls_for("attach")), 2)
        self.assertEqual(len(self.calls_for("detach")), 1)
        self.assertTrue(self.proxy.recovery_detach_attempted)
        self.assertFalse(self.proxy.cleanup_detach_attempted)
        self.assertIn("recovered on attempt 2", self.log.getvalue())
        for _, options, at in self.calls:
            self.assertLessEqual(options["timeout"], 60 - at)

    def test_partial_apfs_rejects_unbound_multistore_physical_or_changed_devices(self):
        for change in (lambda d: d.update(APFSPhysicalStores=[]),
                       lambda d: d.update(APFSPhysicalStores=[{"APFSPhysicalStore": "disk88s1"}]),
                       lambda d: d["APFSPhysicalStores"].append({"APFSPhysicalStore": "disk88s1"}),
                       lambda d: d.update(VirtualOrPhysical="Physical"),
                       lambda d: d.update(DeviceNode="/dev/disk88")):
            with self.subTest(change=change):
                self.calls = []
                info = self.apfs_disk_info("/dev/disk100"); change(info)
                self.disk_infos = [self.apfs_disk_info("/dev/disk99"), info]
                self.registries = [self.apfs_registry()]
                with self.assertRaises(fixture.UnsafeRetry):
                    self.proxy.cleanup_owned_image()
                self.assertEqual(self.calls_for("detach"), [])

    def test_partial_apfs_rejects_shared_devices_and_registry_drift(self):
        shared = self.apfs_registry()
        shared["images"].append({"image-path": str(self.root / "other.dmg"),
                                 "system-entities": [{"dev-entry": "/dev/disk99s2"}]})
        for registries in ([shared], [self.apfs_registry(), shared]):
            with self.subTest(registries=len(registries)):
                self.calls, self.registries = [], registries.copy()
                with self.assertRaises(fixture.UnsafeRetry):
                    self.proxy.cleanup_owned_image()
                self.assertEqual(self.calls_for("detach"), [])

    def test_partial_apfs_binding_is_rechecked_before_detach(self):
        bad = self.apfs_disk_info("/dev/disk100")
        bad["APFSPhysicalStores"] = [{"APFSPhysicalStore": "disk88s1"}]
        self.disk_infos = [self.apfs_disk_info("/dev/disk99"),
                           self.apfs_disk_info("/dev/disk100"),
                           self.apfs_disk_info("/dev/disk99"), bad]
        self.registries = [self.apfs_registry()] * 2
        with self.assertRaises(fixture.UnsafeRetry):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def test_partial_apfs_image_change_prevents_detach(self):
        self.registries = [self.apfs_registry()] * 2
        self.after_disk_info = lambda: self.image.write_bytes(b"modified owned image")
        with self.assertRaises(fixture.UnsafeRetry):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def test_partial_apfs_recovery_cannot_extend_attach_budget(self):
        self.failures, self.attach_durations = [self.error()], [59.99]
        self.registries = [self.apfs_registry()] * 3
        with self.assertRaises(subprocess.CalledProcessError):
            self.invoke()
        self.assertEqual(len(self.calls_for("attach")), 1)
        self.assertEqual(self.calls_for("detach"), [])

    def test_retry_allows_ctime_only_change_with_exact_bytes(self):
        before = self.image.stat()
        self.failures = [self.error()]
        self.after_attach = lambda: os.utime(
            self.image, ns=(before.st_atime_ns + 1, before.st_mtime_ns))
        self.assertEqual(self.invoke().returncode, 0)

    def test_retry_rejects_changed_permissions_with_exact_bytes(self):
        self.failures = [self.error()]
        permission_changed = False
        real_lstat = Path.lstat

        def change_permission():
            nonlocal permission_changed
            permission_changed = True

        def controlled_lstat(path, *args, **kwargs):
            info = real_lstat(path, *args, **kwargs)
            if path != self.image or not permission_changed:
                return info
            fields = list(info)
            fields[stat.ST_MODE] ^= stat.S_IWUSR
            return os.stat_result(fields, {
                "st_atime_ns": info.st_atime_ns,
                "st_mtime_ns": info.st_mtime_ns,
                "st_ctime_ns": info.st_ctime_ns,
            })

        # Windows chmod does not model POSIX permission bits. Change only the
        # reported permission bit; real image bytes and identity stay intact.
        self.after_attach = change_permission
        with patch.object(Path, "lstat", controlled_lstat):
            with self.assertRaises(subprocess.CalledProcessError):
                self.invoke()
        self.assertEqual(len(self.calls_for("attach")), 1)

    def test_retry_rejects_changed_bytes_even_with_same_inode_size_and_mtime(self):
        before = self.image.stat()
        def change():
            self.image.write_bytes(b"x" * before.st_size)
            os.utime(self.image, ns=(before.st_atime_ns, before.st_mtime_ns))
        self.failures, self.after_attach = [self.error()], change
        with self.assertRaises(subprocess.CalledProcessError):
            self.invoke()
        self.assertEqual(len(self.calls_for("attach")), 1)

    def test_failed_recovery_detach_or_postcondition_is_never_repeated(self):
        for failure in (self.error(b"detach failed"), None):
            with self.subTest(failure=failure):
                self.calls, self.now = [], 0.0
                self.proxy = fixture.FixtureSubprocess(
                    self.root, self.image, runner=self.run_fake,
                    clock=lambda: self.now, sleep=self.sleep, log=self.log)
                self.failures = [self.error(), failure]
                self.registries = [self.apfs_registry()] * 4
                with self.assertRaises(subprocess.CalledProcessError):
                    self.invoke()
                with self.assertRaises(Exception):
                    self.proxy.cleanup_owned_image()
                self.assertEqual(len(self.calls_for("detach")), 1)

    def test_registered_eagain_retains_original_error_and_observed_topology(self):
        registry = self.owned_registry()
        registry["images"][0]["system-entities"].append({"dev-entry": "/dev/disk100"})
        failure = self.error()
        self.failures, self.registries = [failure], [registry]
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.invoke()
        self.assertIs(caught.exception, failure)
        self.assertEqual(failure.stderr, fixture.EAGAIN + b"\n")
        self.assertEqual([call[0][1] for call in self.calls], ["attach", "info"])
        self.assertEqual(self.sleeps, [])
        self.assertFalse(self.proxy.cleanup_detach_attempted)
        self.assertIn("entities=3, device_roots=2, whole_devices=2, mount_entries=0",
                      failure.__notes__[0])

    def test_partial_cleanup_diagnostic_distinguishes_absent_and_stale_mount(self):
        for mounted in (False, True):
            with self.subTest(registered_mount=mounted):
                self.calls = []
                registry = self.owned_registry(mounted=mounted)
                registry["images"][0]["system-entities"].append(
                    {"dev-entry": "/dev/disk100"})
                self.proxy.attempted_mounts.add(self.mount)
                self.registries = [registry]
                with patch.object(Path, "is_mount", return_value=False) as is_mount:
                    with self.assertRaisesRegex(fixture.UnsafeRetry, "requires one owned device") as caught:
                        self.proxy.cleanup_owned_image()
                self.assertEqual(is_mount.call_count, 2 if mounted else 0)
                self.assertEqual([call[0][1] for call in self.calls], ["info"])
                self.assertFalse(self.proxy.cleanup_detach_attempted)
                self.assertIn(f"mount_entries={int(mounted)}, registered_mounts={int(mounted)}, "
                              "actual_owned_mount=no", str(caught.exception))

    def test_owned_diagnostic_never_serializes_paths_devices_or_extra_registry_fields(self):
        registry = self.owned_registry(mounted=True)
        registry["images"][0]["system-entities"][0].update(
            {"content-hint": "private-content-hint", "volume-name": "private-volume-name"})
        registry["images"].append({"image-path": str(self.root / "private-other.dmg"),
            "system-entities": [{"dev-entry": "/dev/disk101", "private-key": "private-value"}]})
        self.registries = [registry]
        with self.assertRaises(fixture.UnsafeRetry) as caught:
            self.proxy.registry_clear(self.mount, 60)
        message = str(caught.exception)
        self.assertIn("entities=2, device_roots=1, whole_devices=1, mount_entries=1", message)
        for private in (str(self.root), str(self.mount), "/dev/disk", "private-"):
            self.assertNotIn(private, message)
        self.assertEqual([call[0][1] for call in self.calls], ["info"])

    def test_largest_valid_owned_record_has_a_constant_size_diagnostic(self):
        self.registries = [{"images": [{"image-path": str(self.image), "system-entities": [
            {"dev-entry": f"/dev/disk{number}"} for number in range(128)]}]}]
        with self.assertRaises(fixture.UnsafeRetry) as caught:
            self.proxy.registry_clear(self.mount, 60)
        message = str(caught.exception)
        self.assertIn("entities=128, device_roots=128, whole_devices=128, mount_entries=0", message)
        self.assertLess(len(message), 160)
        self.assertEqual([call[0][1] for call in self.calls], ["info"])

    def test_invalid_owned_registry_remains_validation_failure_without_summary(self):
        registry = self.owned_registry()
        registry["images"][0]["system-entities"][0]["dev-entry"] = "/private/not-a-device"
        self.failures, self.registries = [self.error()], [registry]
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.invoke()
        message = caught.exception.__notes__[0]
        self.assertIn("registry device identity is missing or malformed", message)
        self.assertNotIn("entities=", message)
        self.assertNotIn("/private/not-a-device", message)
        self.assertEqual([call[0][1] for call in self.calls], ["attach", "info"])

    def test_cleanup_detaches_only_owned_partial_image_once_then_checks_absence(self):
        for mounted in (False, True):
            with self.subTest(mounted=mounted):
                self.calls = []
                self.proxy = fixture.FixtureSubprocess(
                    self.root, self.image, runner=self.run_fake,
                    clock=lambda: self.now, sleep=self.sleep, log=self.log,
                )
                self.proxy.attempted_mounts.add(self.mount)
                self.registries = [self.owned_registry(mounted=mounted), {"images": []}]
                self.proxy.cleanup_owned_image()
                self.assertEqual(len(self.calls_for("detach")), 1)
                self.assertEqual(self.calls_for("detach")[0][0],
                                 ["/usr/bin/hdiutil", "detach", "/dev/disk99"])
                self.assertEqual(len(self.calls_for("info")), 2)
                self.assertEqual(self.calls_for("attach"), [])
                self.assertEqual(self.sleeps, [])

    def test_apfs_cleanup_uses_unique_actual_owned_mount_not_inferred_disk(self):
        registry = self.owned_registry()
        registry["images"][0]["system-entities"] += [
            {"dev-entry": "/dev/disk100"},
            {"dev-entry": "/dev/disk100s1", "mount-point": str(self.mount)}]
        self.proxy.attempted_mounts.add(self.mount)
        self.registries = [registry, {"images": []}]
        with patch.object(Path, "is_mount", side_effect=[True, False]):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach")[0][0],
                         ["/usr/bin/hdiutil", "detach", str(self.mount)])
        self.assertEqual(len(self.calls_for("detach")), 1)

    def test_cleanup_refuses_another_image_claiming_the_same_owned_mount(self):
        registry = self.owned_registry(mounted=True)
        registry["images"].append({"image-path": str(self.root / "other.dmg"),
            "system-entities": [{"dev-entry": "/dev/disk101", "mount-point": str(self.mount)}]})
        self.proxy.attempted_mounts.add(self.mount)
        self.registries = [registry]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "mount is shared"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def test_apfs_without_actual_owned_mount_is_not_detached(self):
        registry = self.owned_registry()
        registry["images"][0]["system-entities"] += [{"dev-entry": "/dev/disk100"}]
        self.registries = [registry]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "requires one owned device"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def test_cleanup_requires_explicit_whole_device_not_only_partition(self):
        registry = self.owned_registry()
        registry["images"][0]["system-entities"] = [{"dev-entry": "/dev/disk99s1"}]
        self.registries = [registry]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "whole-device identity"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def completed_normal_detach(self):
        """Use the real proxy call path for one successful attach and detach."""
        self.invoke()
        self.proxy.run(["/usr/bin/hdiutil", "detach", str(self.mount)], **self.options)

    def partition_only_registry(self):
        value = self.owned_registry()
        value["images"][0]["system-entities"] = [{"dev-entry": "/dev/disk99s1"}]
        return value

    def test_successful_detach_waits_for_residual_partition_to_disappear(self):
        self.completed_normal_detach()
        partial = self.partition_only_registry()
        self.registries = [partial, partial, {"images": []}]
        self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls_for("detach")), 1)
        self.assertEqual(len(self.calls_for("info")), 3)
        self.assertEqual(self.sleeps, [0.1, 0.2])
        self.assertFalse(self.proxy.cleanup_detach_attempted)

    def test_completed_detach_persistent_partial_record_is_bounded_and_fatal(self):
        self.completed_normal_detach()
        self.registries = [self.partition_only_registry()] * 6
        with self.assertRaisesRegex(fixture.UnsafeRetry, "persists after completed detach"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls_for("detach")), 1)
        self.assertEqual(len(self.calls_for("info")), 5)
        self.assertEqual(self.sleeps, [0.1, 0.2, 0.4, 0.8])

    def test_failed_normal_detach_never_authorizes_registry_settling(self):
        self.invoke()
        self.failures = [self.error(b"detach failed")]
        with self.assertRaises(subprocess.CalledProcessError):
            self.proxy.run(["/usr/bin/hdiutil", "detach", str(self.mount)], **self.options)
        self.registries = [self.partition_only_registry(), {"images": []}]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "whole-device identity"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls_for("detach")), 1)
        self.assertEqual(self.sleeps, [])

    def test_completed_detach_never_repeats_detach_for_complete_registration(self):
        self.completed_normal_detach()
        self.registries = [self.owned_registry()]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "unexpected registry topology"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls_for("detach")), 1)
        self.assertEqual(self.sleeps, [])

    def test_completed_detach_refuses_changed_image_before_settling(self):
        self.completed_normal_detach()
        self.image.write_bytes(b"different")
        self.registries = [self.partition_only_registry()]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "image changed"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls_for("detach")), 1)
        self.assertEqual(self.sleeps, [])

    def test_completed_detach_refuses_mounted_path_or_malformed_registry(self):
        self.completed_normal_detach()
        self.registries = [self.partition_only_registry()]
        with patch.object(Path, "is_mount", return_value=True):
            with self.assertRaisesRegex(fixture.UnsafeRetry, "actual mounted volume"):
                self.proxy.cleanup_owned_image()
        self.registries = [{"images": [{}]}]
        with self.assertRaises(fixture.UnsafeRetry):
            self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls_for("detach")), 1)
        self.assertEqual(self.sleeps, [])

    def test_completed_detach_refuses_shared_or_foreign_mount_claim(self):
        self.completed_normal_detach()
        partial = self.partition_only_registry()
        for entity in ({"dev-entry": "/dev/disk99s2"},
                       {"dev-entry": "/dev/disk100", "mount-point": str(self.mount)}):
            with self.subTest(entity=entity):
                self.registries = [{"images": partial["images"] + [{
                    "image-path": str(self.root / "other.dmg"), "system-entities": [entity]}]}]
                with self.assertRaisesRegex(fixture.UnsafeRetry, "ambiguous registry owner"):
                    self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls_for("detach")), 1)
        self.assertEqual(self.sleeps, [])

    def test_completed_detach_observation_keeps_existing_deadline(self):
        self.completed_normal_detach()
        self.registries = [self.partition_only_registry()]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "no deadline budget"):
            self.proxy.cleanup_owned_image(deadline=self.now + 0.05)
        self.assertEqual(self.sleeps, [])
        self.assertEqual(len(self.calls_for("detach")), 1)

    def test_new_attach_cannot_reuse_an_earlier_completed_detach(self):
        self.completed_normal_detach()
        self.invoke()
        self.registries = [self.partition_only_registry()]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "whole-device identity"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.sleeps, [])

    def test_second_cleanup_preserves_timeout_without_repeating_detach(self):
        self.registries = [self.owned_registry()]
        timeout = subprocess.TimeoutExpired(["detach"], 60)
        self.failures = [timeout]
        for _ in range(2):
            with self.assertRaises(subprocess.TimeoutExpired) as caught:
                self.proxy.cleanup_owned_image()
            self.assertIs(caught.exception, timeout)
        self.assertEqual(len(self.calls_for("detach")), 1)

    def test_cleanup_absent_image_leaves_unrelated_device_untouched(self):
        self.registries = [{"images": [{"image-path": str(self.root / "other.dmg"),
                            "system-entities": [{"dev-entry": "/dev/disk77"}]}]}]
        self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(self.calls_for("detach"), [])

    def test_absent_image_cleanup_needs_no_image_mutation_authority(self):
        self.image.write_bytes(b"changed fixture with no active registration")
        self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def test_absent_image_cannot_hide_registered_attempted_mount(self):
        self.proxy.attempted_mounts.add(self.mount)
        self.registries = [{"images": [{"image-path": str(self.root / "other.dmg"),
            "system-entities": [{"dev-entry": "/dev/disk77", "mount-point": str(self.mount)}]}]}]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "mount remains registered"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def test_absent_image_cannot_hide_mounted_attempted_path(self):
        self.proxy.attempted_mounts.add(self.mount)
        with patch.object(Path, "is_mount", return_value=True):
            with self.assertRaisesRegex(fixture.UnsafeRetry, "volume is still mounted"):
                self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def test_cleanup_refuses_ambiguous_shared_or_unexpected_mounts(self):
        unexpected = self.owned_registry(mounted=True)
        shared = self.owned_registry()
        shared["images"].append({"image-path": str(self.root / "other.dmg"),
                                 "system-entities": [{"dev-entry": "/dev/disk99s2"}]})
        ambiguous = self.owned_registry()
        ambiguous["images"][0]["system-entities"].append({"dev-entry": "/dev/disk100"})
        empty = self.owned_registry()
        empty["images"][0]["system-entities"] = []
        for registry in (unexpected, shared, ambiguous, empty, {"images": [{}]}):
            with self.subTest(registry=registry):
                self.calls = []
                self.registries = [registry]
                with self.assertRaises(fixture.UnsafeRetry):
                    self.proxy.cleanup_owned_image()
                self.assertEqual(self.calls_for("detach"), [])

    def test_cleanup_refuses_image_changed_during_observation(self):
        self.registries = [self.owned_registry()]
        self.after_info = lambda: self.image.write_bytes(b"changed")
        with self.assertRaisesRegex(fixture.UnsafeRetry, "changed before"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def test_cleanup_accepts_ctime_only_change_but_binds_complete_image_bytes(self):
        self.image.chmod(0o600)
        self.registries = [self.owned_registry(), {"images": []}]
        self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls_for("detach")), 1)

    def test_cleanup_rejects_changed_bytes_even_with_same_inode_size_and_mtime(self):
        original = self.image.stat()
        self.image.write_bytes(b"x" * original.st_size)
        os.utime(self.image, ns=(original.st_atime_ns, original.st_mtime_ns))
        self.registries = [self.owned_registry()]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "changed before cleanup"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(self.calls_for("detach"), [])

    def test_cleanup_detach_failure_is_fatal_and_never_retried(self):
        self.registries = [self.owned_registry()]
        failure = subprocess.CalledProcessError(1, ["detach"])
        self.failures = [failure]
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.proxy.cleanup_owned_image()
        self.assertIs(caught.exception, failure)
        self.assertEqual(len(self.calls_for("detach")), 1)
        self.assertEqual(self.sleeps, [])

    def test_cleanup_requires_registry_clear_after_successful_detach(self):
        self.registries = [self.owned_registry(), self.owned_registry()]
        with self.assertRaisesRegex(fixture.UnsafeRetry, "owned image remains"):
            self.proxy.cleanup_owned_image()
        self.assertEqual(len(self.calls_for("detach")), 1)

    def test_failed_attach_records_attempt_before_native_failure(self):
        failure = self.error(b"unrelated failure")
        self.failures = [failure]
        with self.assertRaises(subprocess.CalledProcessError):
            self.invoke()
        self.assertEqual(self.proxy.attempted_mounts, {self.mount})

    def test_private_proxy_does_not_change_global_run_or_swallow_injection(self):
        original = subprocess.run
        resilient_run = self.proxy.run
        injected = self.error()
        def fail_after_attach(arguments, **options):
            resilient_run(arguments, **options)
            raise injected
        with patch.object(self.proxy, "run", fail_after_attach):
            with self.assertRaises(subprocess.CalledProcessError) as caught:
                self.invoke()
        self.assertIs(caught.exception, injected)
        self.assertIs(subprocess.run, original)
        self.assertIs(self.proxy.CalledProcessError, subprocess.CalledProcessError)
        self.assertEqual(len(self.calls), 1)


if __name__ == "__main__":
    unittest.main()

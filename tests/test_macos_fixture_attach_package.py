"""Portable fake-process checks; no native command or application is executed."""

import importlib.util
import io
import plistlib
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

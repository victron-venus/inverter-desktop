"""Exercise the native-probe supervisor using inert Python children only."""

import contextlib
import importlib.util
import io
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).parent / "tls-policy/run.py"
SPEC = importlib.util.spec_from_file_location("tls_watchdog", SCRIPT)
WATCHDOG = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WATCHDOG)


@unittest.skipUnless(os.name == "posix", "The Apple watchdog uses POSIX groups")
class WatchdogTests(unittest.TestCase):
    def execute(self, code, *args, timeout=5):
        with contextlib.redirect_stderr(io.StringIO()) as log:
            result = WATCHDOG.supervise(
                [sys.executable, "-c", code, *map(str, args)],
                timeout=timeout,
                grace=1,
            )
        self.assertTrue(result["process_group_clean"], log.getvalue())
        self.assertTrue(result["scratch_removed"], log.getvalue())
        self.assertIn("TLS_POLICY_WATCHDOG ", log.getvalue())
        return result

    def test_success_and_nonzero_are_preserved(self):
        self.assertEqual(self.execute("pass")["returncode"], 0)
        self.assertEqual(self.execute("raise SystemExit(7)")["returncode"], 7)

    def test_silent_child_has_one_deadline_and_is_reaped(self):
        start = time.monotonic()
        result = self.execute("import time; time.sleep(60)", timeout=2)
        self.assertEqual(result["reason"], "timeout")
        self.assertEqual(result["returncode"], 124)
        self.assertLess(time.monotonic() - start, 5)

    def test_term_ignoring_child_is_killed(self):
        with tempfile.TemporaryDirectory() as outside:
            ready = Path(outside) / "ready"
            result = self.execute(
                "import pathlib,signal,sys,time;"
                "signal.signal(signal.SIGTERM,signal.SIG_IGN);"
                "pathlib.Path(sys.argv[1]).write_text('ready');time.sleep(60)",
                ready, timeout=3,
            )
            self.assertEqual(ready.read_text(), "ready")
            self.assertEqual(result["returncode"], 124)
            self.assertEqual(result["child_returncode"], -signal.SIGKILL)

    def test_owned_fixture_directory_removed(self):
        with tempfile.TemporaryDirectory() as outside:
            record = Path(outside) / "path"
            self.execute(
                "import os,pathlib,sys; p=pathlib.Path(os.environ['TMPDIR']);"
                "(p/'key').write_text('synthetic');pathlib.Path(sys.argv[1]).write_text(str(p))",
                record,
            )
            self.assertFalse(Path(record.read_text()).exists())

    def assert_gone(self, pid):
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)

    def test_timeout_reaps_descendant_and_leader(self):
        with tempfile.TemporaryDirectory() as outside:
            pidfile = Path(outside) / "pid"
            code = (
                "import pathlib,signal,subprocess,sys,time;"
                "p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)']);"
                "pathlib.Path(sys.argv[1]).write_text(str(p.pid));"
                "signal.signal(signal.SIGTERM,lambda *_:(p.wait(),sys.exit(0)));"
                "time.sleep(60)"
            )
            result = self.execute(code, pidfile, timeout=3)
            self.assertEqual(result["returncode"], 124)
            self.assert_gone(int(pidfile.read_text()))

    @unittest.skipUnless(sys.platform == "darwin", "Apple orphan reaping contract")
    def test_successful_leader_cannot_leave_a_descendant(self):
        with tempfile.TemporaryDirectory() as outside:
            pidfile = Path(outside) / "pid"
            result = self.execute(
                "import pathlib,subprocess,sys;"
                "p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)']);"
                "pathlib.Path(sys.argv[1]).write_text(str(p.pid))",
                pidfile,
            )
            self.assertTrue(result["orphaned_children"])
            self.assertEqual(result["returncode"], 1)
            self.assert_gone(int(pidfile.read_text()))

    def test_external_cancellation_cleans_group(self):
        with tempfile.TemporaryDirectory() as outside:
            pidfile = Path(outside) / "pid"
            process = subprocess.Popen(
                [sys.executable, str(SCRIPT), "--", sys.executable, "-c",
                 "import os,pathlib,sys,time;pathlib.Path(sys.argv[1]).write_text(str(os.getpid()));time.sleep(60)",
                 str(pidfile)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                text=True,
            )
            try:
                deadline = time.monotonic() + 5
                while not pidfile.exists() and time.monotonic() < deadline:
                    time.sleep(.02)
                self.assertTrue(pidfile.exists())
                process.send_signal(signal.SIGTERM)
                _, error = process.communicate(timeout=12)
                self.assertEqual(process.returncode, 128 + signal.SIGTERM, error)
                result = json.loads(error.split("TLS_POLICY_WATCHDOG ")[-1])
                self.assertTrue(result["process_group_clean"])
                self.assertTrue(result["scratch_removed"])
                self.assert_gone(int(pidfile.read_text()))
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
                    try:
                        process.communicate(timeout=12)
                    except subprocess.TimeoutExpired:
                        # The child is a fresh session leader recorded by our
                        # inert fixture. Never leave it alive if the supervisor
                        # itself is broken and must be killed by this test.
                        try:
                            self.clean_recorded_group(pidfile)
                        finally:
                            process.kill()
                            process.communicate(timeout=2)
                self.clean_recorded_group(pidfile)

    def clean_recorded_group(self, pidfile):
        if not pidfile.exists():
            return
        pid = int(pidfile.read_text())
        deadline = time.monotonic() + 2
        while True:
            try:
                self.assertEqual(os.getpgid(pid), pid)
                break
            except ProcessLookupError:
                return
            except PermissionError:
                if time.monotonic() >= deadline:
                    self.fail("owned child group identity remains unavailable")
                time.sleep(.02)
        for sig in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.killpg(pid, sig)
            except ProcessLookupError:
                return
            except PermissionError:
                pass
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                if WATCHDOG.group_state(pid) is False:
                    return
                time.sleep(.02)
        self.fail("owned inert child group survived bounded cleanup")

    def test_cleanup_error_restores_signals_and_preserves_other_paths(self):
        previous = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM)}
        with tempfile.TemporaryDirectory() as outside:
            owned = Path(outside) / "owned"
            owned.mkdir()
            other = Path(outside) / "unrelated"
            other.write_text("keep")
            with (
                mock.patch.object(WATCHDOG.tempfile, "mkdtemp", return_value=str(owned)),
                mock.patch.object(WATCHDOG.shutil, "rmtree", side_effect=OSError("injected cleanup error")),
                contextlib.redirect_stderr(io.StringIO()),
                self.assertRaisesRegex(OSError, "injected cleanup error"),
            ):
                WATCHDOG.supervise([sys.executable, "-c", "pass"])
            self.assertEqual({sig: signal.getsignal(sig) for sig in previous}, previous)
            self.assertEqual(other.read_text(), "keep")
            self.assertTrue(owned.is_dir())

    def test_launch_error_preserves_exception_and_removes_scratch(self):
        with (
            contextlib.redirect_stderr(io.StringIO()) as log,
            self.assertRaises(FileNotFoundError),
        ):
            WATCHDOG.supervise(["/does-not-exist/tls-probe"])
        result = json.loads(log.getvalue().split("TLS_POLICY_WATCHDOG ")[-1])
        self.assertTrue(result["scratch_removed"])

    def test_permission_denied_never_means_group_absent(self):
        with mock.patch.object(WATCHDOG.os, "killpg", side_effect=PermissionError):
            self.assertIsNone(WATCHDOG.group_state(12345))
            process = mock.Mock(pid=12345)
            process.poll.return_value = 0
            self.assertEqual(WATCHDOG.stop_group(process, grace=.03), (False, False))

    def test_exited_child_unknown_then_absent_is_not_an_orphan(self):
        with mock.patch.object(WATCHDOG, "group_state", side_effect=[None, False]):
            result = self.execute("pass")
        self.assertEqual(result["returncode"], 0)
        self.assertFalse(result["orphaned_children"])

    def test_exited_child_unknown_then_present_is_an_orphan(self):
        with mock.patch.object(WATCHDOG, "group_state", side_effect=[None, True, False]):
            result = self.execute("pass")
        self.assertEqual(result["returncode"], 1)
        self.assertTrue(result["orphaned_children"])

    def test_transient_permission_denied_observation_can_settle(self):
        with mock.patch.object(
            WATCHDOG.os, "killpg", side_effect=[PermissionError, ProcessLookupError]
        ):
            self.assertIsNone(WATCHDOG.group_state(12345))
            self.assertFalse(WATCHDOG.group_state(12345))


if __name__ == "__main__":
    unittest.main()

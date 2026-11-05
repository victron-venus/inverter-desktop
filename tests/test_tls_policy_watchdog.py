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
                 ("import os,pathlib,signal,sys,time;signal.alarm(10);"
                  "pathlib.Path(sys.argv[1]).write_text(str(os.getpid()));time.sleep(60)"),
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
                self.assertEqual(result["child_returncode"], -signal.SIGTERM)
                self.assertTrue(result["scratch_removed"])
                self.assert_gone(int(pidfile.read_text()))
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
                    try:
                        process.communicate(timeout=12)
                    except subprocess.TimeoutExpired:
                        # Only this directly owned Popen may be signalled here.
                        # The fixture self-expires; its recorded PID is never
                        # authority to signal a potentially reused identity.
                        process.kill()
                        process.communicate(timeout=2)
                if pidfile.exists():
                    deadline = time.monotonic() + 12
                    while True:
                        try:
                            os.kill(int(pidfile.read_text()), 0)
                        except ProcessLookupError:
                            break
                        if time.monotonic() >= deadline:
                            self.fail("self-expiring fixture remains observable")
                        time.sleep(.02)

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

    def test_signals_finish_before_reaping(self):
        events = []
        process = mock.Mock(pid=12345, returncode=None)
        process.wait.side_effect = lambda **_: events.append("reap")
        snapshots = [[{"pid": 12345, "state": "S"}], [], []]
        with (
            mock.patch.object(WATCHDOG, "observed_exit", return_value=object()),
            mock.patch.object(WATCHDOG, "group_members", side_effect=snapshots),
            mock.patch.object(WATCHDOG.os, "killpg", side_effect=lambda *_: events.append("signal")),
        ):
            self.assertEqual(WATCHDOG.stop_group(process, .1), (True, True))
        self.assertEqual(events, ["signal", "reap"])
        process.poll.assert_not_called()

    def test_zombie_leader_is_reaped_without_group_signal(self):
        process = mock.Mock(pid=12345, returncode=None)
        with (
            mock.patch.object(WATCHDOG, "observed_exit", return_value=object()),
            mock.patch.object(WATCHDOG, "group_members", side_effect=[[{"pid": 12345, "state": "Z"}], []]),
            mock.patch.object(WATCHDOG.os, "killpg") as kill,
        ):
            self.assertEqual(WATCHDOG.stop_group(process, .1), (True, False))
        kill.assert_not_called()
        process.wait.assert_called_once()
        process.poll.assert_not_called()

    def test_denied_signal_cannot_reap_live_group_or_report_cleanup(self):
        process = mock.Mock(pid=12345, returncode=None)
        with (
            mock.patch.object(WATCHDOG, "observed_exit", return_value=object()),
            mock.patch.object(WATCHDOG, "group_members", return_value=[{"pid": 12345, "state": "S"}]),
            mock.patch.object(WATCHDOG.os, "killpg", side_effect=PermissionError),
        ):
            self.assertEqual(WATCHDOG.stop_group(process, .03), (False, True))
        process.wait.assert_not_called()
        process.poll.assert_not_called()

    def test_already_reaped_leader_refuses_signals(self):
        process = mock.Mock(pid=12345, returncode=0)
        with (
            mock.patch.object(WATCHDOG.os, "killpg") as kill,
            self.assertRaisesRegex(RuntimeError, "already reaped"),
        ):
            WATCHDOG.stop_group(process, .03)
        kill.assert_not_called()

    def test_post_reap_observation_never_sends_another_signal(self):
        process = mock.Mock(pid=12345, returncode=None)
        with (
            mock.patch.object(WATCHDOG, "observed_exit", return_value=object()),
            mock.patch.object(WATCHDOG, "group_members", side_effect=[[], [{"pid": 12345, "state": "S"}], []]),
            mock.patch.object(WATCHDOG.os, "killpg") as kill,
        ):
            self.assertEqual(WATCHDOG.stop_group(process, .1), (True, False))
        kill.assert_not_called()
        process.wait.assert_called_once()

    def test_unknown_unrelated_state_does_not_hide_owned_zombie(self):
        raw = f"{os.getpid()} 1 ?\n12345 12345 Z\n".encode()
        with mock.patch.object(WATCHDOG.subprocess, "check_output", return_value=raw):
            self.assertEqual(WATCHDOG.group_members(12345, time.monotonic() + 1),
                             [{"pid": 12345, "state": "Z"}])

    def test_unknown_owned_state_retries_without_extending_deadline(self):
        prefix = f"{os.getpid()} 1 S\n"
        deadline = time.monotonic() + 1
        snapshot = WATCHDOG.group_snapshot
        with (
            mock.patch.object(WATCHDOG, "group_snapshot", wraps=snapshot) as observe,
            mock.patch.object(WATCHDOG.subprocess, "check_output", side_effect=[
                (prefix + "12345 12345 ?Es\n").encode(),
                (prefix + "12345 12345 Z\n").encode(),
            ]) as read,
        ):
            self.assertEqual(WATCHDOG.group_members(12345, deadline),
                             [{"pid": 12345, "state": "Z"}])
        self.assertEqual(read.call_count, 2)
        self.assertEqual(observe.call_args_list, [mock.call(12345, deadline)] * 2)

    def test_persistent_unknown_state_never_reaps_or_signals(self):
        process = mock.Mock(pid=12345, returncode=None)
        raw = f"{os.getpid()} 1 S\n12345 12345 ?Es\n".encode()
        with (
            mock.patch.object(WATCHDOG, "observed_exit", return_value=object()),
            mock.patch.object(WATCHDOG.subprocess, "check_output", return_value=raw),
            mock.patch.object(WATCHDOG.os, "killpg") as kill,
            self.assertRaises(TimeoutError),
        ):
            WATCHDOG.stop_group(process, .03)
        kill.assert_not_called()
        process.wait.assert_not_called()

    def test_lost_child_reservation_refuses_signals(self):
        process = mock.Mock(pid=12345, returncode=None)
        with (
            mock.patch.object(WATCHDOG.os, "waitid", side_effect=ChildProcessError),
            mock.patch.object(WATCHDOG.os, "killpg") as kill,
            self.assertRaises(ChildProcessError),
        ):
            WATCHDOG.stop_group(process, .03)
        kill.assert_not_called()
        process.wait.assert_not_called()

    def test_nondefault_child_handler_fails_before_launch(self):
        original = signal.getsignal(signal.SIGCHLD)
        try:
            signal.signal(signal.SIGCHLD, signal.SIG_IGN)
            with mock.patch.object(WATCHDOG.subprocess, "Popen") as spawn:
                with self.assertRaisesRegex(RuntimeError, "SIGCHLD"):
                    WATCHDOG.supervise([sys.executable, "-c", "pass"])
                spawn.assert_not_called()
        finally:
            signal.signal(signal.SIGCHLD, original)


if __name__ == "__main__":
    unittest.main()

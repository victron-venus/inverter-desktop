"""Bound the POSIX native TLS test process and its disposable child processes."""

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# The successful Apple probe step took 251 seconds on the reviewed source.
# Compilation already ran in the preceding lib TLS test step; keep ample margin
# without allowing a stalled probe to exhaust the 30-minute CI job.
PROBE_SECONDS = 600
GRACE_SECONDS = 5


def group_state(pid):
    try:
        os.killpg(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        # Darwin can report EPERM while a group is exiting. It is not proof of
        # absence: keep observing within the original cleanup deadline.
        return None
    return True


def stop_group(process, grace):
    """Return cleanup certainty and whether the group was confirmed present."""
    present = False
    for sig in (signal.SIGTERM, signal.SIGKILL):
        process.poll()
        state = group_state(process.pid)
        present |= state is True
        if state is False:
            return process.poll() is not None, present
        try:
            os.killpg(process.pid, sig)
        except (ProcessLookupError, PermissionError):
            pass
        deadline = time.monotonic() + grace
        while time.monotonic() < deadline:
            process.poll()
            state = group_state(process.pid)
            present |= state is True
            if state is False:
                return process.poll() is not None, present
            time.sleep(0.02)
    return False, present


def supervise(command, *, timeout=PROBE_SECONDS, grace=GRACE_SECONDS):
    if os.name != "posix":
        raise RuntimeError("The TLS process-group watchdog requires POSIX")
    if not command or timeout <= 0 or grace <= 0:
        raise ValueError("A command and positive deadlines are required")
    root = Path(tempfile.mkdtemp(prefix="desktop-tls-watchdog-"))
    env = os.environ.copy()
    env.update(TMPDIR=str(root), TMP=str(root), TEMP=str(root))
    received = []

    def cancelled(signum, _frame):
        received.append(signum)

    signals = (signal.SIGINT, signal.SIGTERM)
    previous = {sig: signal.getsignal(sig) for sig in signals}
    process = None
    cleanup = True
    cleanup_attempted = False
    reason = "launch-error"
    started = time.monotonic()
    result = {}
    try:
        for sig in signals:
            signal.signal(sig, cancelled)
        # Stream both logs directly. A full captured pipe must never stop tests.
        # No terminal input is needed by these disposable loopback probes.
        process = subprocess.Popen(
            command, env=env, stdin=subprocess.DEVNULL, start_new_session=True
        )
        cleanup = False
        deadline = started + timeout
        while process.poll() is None:
            if received:
                reason = "cancelled"
                break
            if time.monotonic() >= deadline:
                reason = "timeout"
                break
            time.sleep(0.02)
        else:
            reason = "exited"
        # Even an exited leader can leave a helper behind. Clean the group on
        # every path and never count orphan cleanup as a successful test run.
        leader_exited = process.poll() is not None
        cleanup_attempted = True
        cleanup, group_was_present = stop_group(process, grace)
        orphaned = leader_exited and group_was_present
        code = process.poll()
        if received:
            returncode = 128 + received[0]
        elif reason == "timeout":
            returncode = 124
        elif not cleanup or orphaned:
            returncode = 1
        else:
            returncode = code if code is not None and code >= 0 else 1
        result = {
            "reason": reason,
            "returncode": returncode,
            "child_returncode": code,
            "orphaned_children": orphaned,
            "process_group_clean": cleanup,
            "elapsed_seconds": round(time.monotonic() - started, 3),
            "deadline_seconds": timeout,
        }
    finally:
        try:
            if process is not None and not cleanup_attempted:
                cleanup, _ = stop_group(process, grace)
            if cleanup:
                shutil.rmtree(root)
        finally:
            # Preserve the caller's signal policy even if filesystem cleanup fails.
            for sig in signals:
                signal.signal(sig, previous[sig])
            if result:
                result["scratch_removed"] = not root.exists()
                result["process_group_clean"] = cleanup
                if not cleanup or root.exists():
                    result["returncode"] = 1
                    result["retained_scratch"] = str(root)
                elif received:
                    result["returncode"] = 128 + received[0]
            # No fixture contents, tokens or unrelated environment values.
            print("TLS_POLICY_WATCHDOG " + json.dumps(result or {
                "reason": reason, "process_group_clean": cleanup,
                "scratch_removed": not root.exists(),
            }), file=sys.stderr, flush=True)
    return result


def main():
    if len(sys.argv) < 3 or sys.argv[1] != "--":
        raise SystemExit("usage: run.py -- COMMAND [ARG ...]")
    raise SystemExit(supervise(sys.argv[2:])["returncode"])


if __name__ == "__main__":
    main()

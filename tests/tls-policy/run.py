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


def observed_exit(process):
    """Observe exit without releasing the leader PID/PGID reservation."""
    if process.returncode is not None:
        raise RuntimeError("Leader already reaped; refusing group signals")
    return os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)


def group_snapshot(pgid, deadline):
    """Read numeric identities, validating states only for the owned group."""
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError("Process cleanup observation deadline")
    raw = subprocess.check_output(
        ["/bin/ps", "-ax", "-o", "pid=,pgid=,stat="], timeout=min(1, remaining)
    )
    if not raw or len(raw) > 2 * 1024**2:
        raise RuntimeError("Invalid process registry snapshot")
    rows, seen = [], set()
    for line in raw.decode("ascii").splitlines():
        fields = line.split()
        if (len(fields) != 3 or not fields[0].isdigit() or not fields[1].isdigit()
                or int(fields[0]) <= 0 or int(fields[1]) < 0):
            raise RuntimeError("Malformed process registry snapshot: " + repr(line[:128]))
        pid, group = int(fields[0]), int(fields[1])
        if pid in seen:
            raise RuntimeError("Duplicate process identity")
        seen.add(pid)
        if group == pgid:
            if not fields[2][0].isalpha() or len(fields[2]) > 16:
                raise RuntimeError("Owned process state is unknown: " + repr(line[:128]))
            rows.append({"pid": pid, "state": fields[2]})
    if os.getpid() not in seen:
        raise RuntimeError("Incomplete process registry snapshot")
    return rows


def group_members(pgid, deadline):
    """Retry transient unknown snapshots without extending cleanup time."""
    while True:
        try:
            return group_snapshot(pgid, deadline)
        except (RuntimeError, OSError, subprocess.SubprocessError) as error:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Cannot confirm owned process state") from error
            time.sleep(min(.02, remaining))


def stop_group(process, grace):
    """Signal before reaping; never signal a potentially reused group ID."""
    observed_exit(process)
    start = time.monotonic()
    deadline = start + 2 * grace
    present = False
    for index, sig in enumerate((signal.SIGTERM, signal.SIGKILL), start=1):
        members = group_members(process.pid, deadline)
        live = any(row["state"][0] != "Z" for row in members)
        present |= live
        if not live:
            break
        observed_exit(process)
        try:
            os.killpg(process.pid, sig)
        except (ProcessLookupError, PermissionError):
            # Denial is never proof of cleanup. The registry must confirm it.
            pass
        phase_deadline = start + index * grace
        while time.monotonic() < phase_deadline:
            members = group_members(process.pid, deadline)
            live = any(row["state"][0] != "Z" for row in members)
            if not live:
                break
            time.sleep(min(.02, max(0, phase_deadline - time.monotonic())))
        if not live:
            break
    if live or observed_exit(process) is None:
        return False, present
    # All possible signals are finished. Only now release the reserved PID.
    process.wait(timeout=max(.001, deadline - time.monotonic()))
    while group_members(process.pid, deadline):
        if time.monotonic() >= deadline:
            return False, present
        time.sleep(.02)
    return True, present


def supervise(command, *, timeout=PROBE_SECONDS, grace=GRACE_SECONDS):
    if os.name != "posix":
        raise RuntimeError("The TLS process-group watchdog requires POSIX")
    if not all(hasattr(os, name) for name in ("waitid", "P_PID", "WEXITED", "WNOHANG", "WNOWAIT")):
        raise RuntimeError("Non-reaping child observation is required")
    if signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL:
        raise RuntimeError("Default SIGCHLD handling is required")
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
        while observed_exit(process) is None:
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
        leader_exited = observed_exit(process) is not None
        cleanup_attempted = True
        cleanup, group_was_present = stop_group(process, grace)
        orphaned = leader_exited and group_was_present
        code = process.returncode
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

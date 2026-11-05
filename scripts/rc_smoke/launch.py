"""Run one published Linux application in a fresh, offline user namespace."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time

TIMEOUT = 180
GRACE = 5
MAX_SCRATCH = 256 * 1024**2


def require(value, message):
    if not value:
        raise RuntimeError(message)


def scratch_size(root, limit=MAX_SCRATCH):
    total, count = 0, 0
    def failed(error):
        raise error
    for base, dirs, files in os.walk(root, followlinks=False, onerror=failed):
        count += len(dirs) + len(files)
        require(count <= 50000, 'Scratch entry limit')
        for name in files:
            try:
                total += (Path(base) / name).lstat().st_size
            except FileNotFoundError:
                continue  # The owned writer removed this file during observation.
            require(total <= limit, 'Scratch byte limit')
    return total


def sandbox_command(root, package, evidence, sync_fd, uid, gid, host_net, host_mnt):
    command = ['/usr/bin/bwrap', '--unshare-user', '--unshare-ipc', '--unshare-pid',
               '--unshare-net', '--unshare-uts', '--uid', str(uid), '--gid', str(gid),
               '--die-with-parent', '--new-session', '--clearenv', '--sync-fd', str(sync_fd),
               '--ro-bind', '/usr', '/usr', '--ro-bind', '/lib', '/lib',
               '--symlink', 'usr/bin', '/bin', '--symlink', 'usr/sbin', '/sbin',
               '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/run',
               '--dir', '/etc', '--dir', '/home', '--dir', '/package', '--dir', '/check',
               '--bind', str(root / 'home'), '/home/smoke',
               '--bind', str(root / 'tmp'), '/tmp',
               '--bind', str(evidence), '/evidence',
               '--ro-bind', str(package), '/package',
               '--ro-bind', str(Path(__file__).resolve().parent), '/check']
    if Path('/lib64').exists():
        command += ['--ro-bind', '/lib64', '/lib64']
    for name in ('passwd', 'group', 'nsswitch.conf', 'hosts', 'machine-id'):
        command += ['--ro-bind', str(root / 'etc' / name), '/etc/' + name]
    for name in ('fonts', 'ssl/certs', 'ld.so.cache', 'ld.so.conf', 'ld.so.conf.d', 'os-release'):
        path = Path('/etc') / name
        if path.exists():
            command += ['--ro-bind', str(path.resolve()), '/etc/' + name]
    values = {'PATH': '/usr/bin:/bin', 'HOME': '/home/smoke', 'LANG': 'C.UTF-8',
              'PYTHONDONTWRITEBYTECODE': '1', 'SMOKE_HOST_NET_NS': host_net,
              'SMOKE_HOST_MNT_NS': host_mnt}
    for key, value in values.items():
        command += ['--setenv', key, value]
    command += ['--chdir', '/home/smoke', '--remount-ro', '/', '--',
                '/usr/bin/dbus-run-session', '--', '/usr/bin/python3', '-B', '/check/inside.py']
    return command


def lifetime_closed(fd, deadline):
    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], max(0, deadline - time.monotonic()))
        if ready:
            data = os.read(fd, 1)
            require(not data, 'Unexpected lifetime pipe data')
            return True
    return False


def supervise(command, sync_read, sync_write, root, watchdog, *, timeout=TIMEOUT, grace=GRACE, evidence=None):
    require(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL, 'Default child ownership required')
    require(hasattr(os, 'WNOWAIT'), 'Non-reaping observation required')
    received = []
    previous = {s: signal.getsignal(s) for s in (signal.SIGINT, signal.SIGTERM)}
    process, group_clean, closed, completed_cleanup = None, False, False, False
    started = time.monotonic()
    deadline = started + timeout
    result = {'success': False, 'deadline_seconds': timeout, 'cleanup_grace_seconds': 3 * grace}
    try:
        for sig in previous:
            signal.signal(sig, lambda s, _: received.append(s))
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, start_new_session=True,
                                   pass_fds=(sync_write,), close_fds=True,
                                   env={'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8'})
        os.close(sync_write)
        sync_write = -1
        reason = 'exited'
        peak = 0
        while watchdog.observed_exit(process) is None:
            if received:
                reason = 'cancelled'
                break
            if time.monotonic() >= deadline:
                reason = 'timeout'
                break
            peak = max(peak, scratch_size(root))
            if evidence is not None:
                scratch_size(evidence, 8 * 1024**2)
            time.sleep(.1)
        completed_cleanup = True
        group_clean, _ = watchdog.stop_group(process, grace)
        closed = lifetime_closed(sync_read, time.monotonic() + grace)
        result.update(reason=reason, child_returncode=process.returncode,
                      success=reason == 'exited' and process.returncode == 0 and group_clean and closed,
                      peak_scratch_bytes=peak)
    finally:
        try:
            if sync_write >= 0:
                os.close(sync_write)
            if process is not None and not completed_cleanup:
                group_clean, _ = watchdog.stop_group(process, grace)
                closed = lifetime_closed(sync_read, time.monotonic() + grace)
            os.close(sync_read)
        finally:
            for sig, handler in previous.items():
                signal.signal(sig, handler)
            result.update(process_group_clean=group_clean, sandbox_lifetime_closed=closed,
                          cancelled=bool(received), elapsed_seconds=round(time.monotonic() - started, 3))
            if received:
                result['success'] = False
    return result


def main():
    require(sys.platform == 'linux' and os.geteuid() != 0, 'Unprivileged Linux runner required')
    binding = json.loads(Path(sys.argv[1]).read_text())
    package, output = Path(sys.argv[2]).resolve(), Path(sys.argv[3]).resolve()
    require(package.is_dir() and not package.is_symlink(), 'Prepared package required')
    require(package.name == 'package' and package.parent == Path(sys.argv[1]).resolve().parent,
            'Package cleanup scope differs')
    executable = package / 'usr/bin/inverter-dashboard'
    require(executable.is_file() and not executable.is_symlink() and
            hashlib.sha256(executable.read_bytes()).hexdigest() == binding['executable_sha256'],
            'Prepared executable changed before launch')
    output.mkdir(exist_ok=False, parents=True)
    repo = Path(__file__).resolve().parents[2]
    helper = repo / 'tests/tls-policy/run.py'
    require(hashlib.sha256(helper.read_bytes()).hexdigest() == binding['watchdog_sha256'],
            'Reviewed lifetime helper changed')
    spec = importlib.util.spec_from_file_location('rc_smoke_watchdog', helper)
    watchdog = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(watchdog)
    root = Path(tempfile.mkdtemp(prefix='desktop-rc-smoke-'))
    for name in ('home', 'tmp', 'etc'):
        (root / name).mkdir(mode=0o700)
    uid, gid = os.geteuid(), os.getegid()
    etc = {'passwd': f'smoke:x:{uid}:{gid}:Smoke:/home/smoke:/bin/false\n',
           'group': f'smoke:x:{gid}:\n', 'nsswitch.conf': 'passwd: files\ngroup: files\nhosts: files\n',
           'hosts': '127.0.0.1 localhost\n::1 localhost\n', 'machine-id': os.urandom(16).hex() + '\n'}
    for name, value in etc.items():
        (root / 'etc' / name).write_text(value)
    rfd, wfd = os.pipe()
    command = sandbox_command(root, package, output, wfd, uid, gid,
                              os.readlink('/proc/self/ns/net'), os.readlink('/proc/self/ns/mnt'))
    result = {'success': False, 'source': binding['source'], 'tag': binding['tag'],
              'asset_id': binding['asset_id'], 'asset_sha256': binding['sha256']}
    try:
        result.update(supervise(command, rfd, wfd, root, watchdog, evidence=output))
        require(result['success'], 'Offline sandbox execution failed')
        scratch_size(output, 8 * 1024**2)
        inside = json.loads((output / 'inside-result.json').read_text())
        require(inside['ready'] and inside['app_reaped'] and inside['nested_namespace_preflight'],
                'Rendered application proof incomplete')
        require(inside['executable_sha256'] == binding['executable_sha256'], 'Executed binary changed')
        result['inside'] = inside
        result['evidence_sha256'] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                                     for p in sorted(output.iterdir()) if p.is_file()}
    finally:
        if result.get('process_group_clean') and result.get('sandbox_lifetime_closed'):
            shutil.rmtree(root)
            shutil.rmtree(package)
        result['package_removed'] = not package.exists()
        result['scratch_removed'] = not root.exists()
        if root.exists():
            result['retained_scratch'] = str(root)
            result['success'] = False
        (output / 'report.json').write_text(json.dumps(result, indent=2) + '\n')
    require(result['success'] and result['scratch_removed'], 'Smoke or cleanup not accepted')


if __name__ == '__main__':
    main()

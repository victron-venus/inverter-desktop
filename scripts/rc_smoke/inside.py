"""Observe the actual packaged app inside a disposable offline Linux namespace."""
import hashlib
import io
import json
import os
from pathlib import Path
import resource
import select
import signal
import stat
import struct
import subprocess
import sys
import time
import threading

EXPECTED = ("Welcome — Setup", "Setup sections", "Connection mode")
EVIDENCE = Path("/evidence")
MAX_RUNTIME_FILE = 256 * 1024**2
MAX_RETAINED_LOG = 1024**2
SCREENSHOT_BINARY = Path("/usr/bin/import-im6.q16")


def require(value, message):
    if not value:
        raise RuntimeError(message)


def screenshot_command():
    # Use the packaged executable; /usr/bin/import follows /etc/alternatives.
    require(SCREENSHOT_BINARY.is_file() and not SCREENSHOT_BINARY.is_symlink()
            and os.access(SCREENSHOT_BINARY, os.X_OK),
            "Packaged screenshot executable unavailable")
    return [str(SCREENSHOT_BINARY), '-display', ':99', '-window', 'root',
            'png:/evidence/rendered.png']


def child_limits():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_FSIZE, (MAX_RUNTIME_FILE, MAX_RUNTIME_FILE))


def visible_setup(rows):
    """Require named, visible, on-screen production content, not a blank window."""
    return all(any(text in (row["name"], row["text"]) and row["showing"]
                   and row["visible"] and row["width"] > 0 and row["height"] > 0
                   and row["x"] < 1280 and row["y"] < 900
                   and row["x"] + row["width"] > 0 and row["y"] + row["height"] > 0
                   for row in rows) for text in EXPECTED)


def visible_contrast(colors):
    if not colors or len(colors) < 8:
        return False
    brightness = [sum(color) for _, color in colors]
    return max(brightness) - min(brightness) >= 120


def probe(pid):
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi
    desktop = Atspi.get_desktop(0)
    roots = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    roots = [root for root in roots if root.get_process_id() == pid]
    require(len(roots) == 1, "Exact application accessibility root unavailable")
    pending, rows = roots[:], []
    while pending:
        require(len(rows) + len(pending) <= 512, "Accessibility tree bound")
        node = pending.pop()
        state = node.get_state_set()
        component = node.get_component_iface()
        rect = component.get_extents(Atspi.CoordType.SCREEN) if component else None
        text = node.get_text_iface()
        rows.append({"name": node.get_name()[:512],
                     "text": text.get_text(0, min(text.get_character_count(), 512)) if text else "",
                     "visible": state.contains(Atspi.StateType.VISIBLE),
                     "showing": state.contains(Atspi.StateType.SHOWING),
                     "x": rect.x if rect else -1, "y": rect.y if rect else -1,
                     "width": rect.width if rect else 0, "height": rect.height if rect else 0})
        pending.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
    raw = json.dumps({"pid": pid, "ready": visible_setup(rows), "rows": rows})
    require(len(raw.encode()) <= 512 * 1024, "Accessibility output bound")
    print(raw)


def stop(process):
    # Popen retains direct-child ownership. Never signal a recorded raw PID/PGID.
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)
    return process.returncode


def exit_status(returncode):
    result = {'returncode': returncode, 'signal': None}
    if returncode is not None and returncode < 0:
        try:
            result['signal'] = signal.Signals(-returncode).name
        except ValueError:
            result['signal'] = 'UNKNOWN'
    return result


def memfd_limit_probe(timeout):
    """Observe the unchanged per-process limit; never gate the application on it."""
    actual_limit = resource.getrlimit(resource.RLIMIT_FSIZE)
    code = ('import os,signal; signal.signal(signal.SIGXFSZ, signal.SIG_DFL); '
            'fd=os.memfd_create("smoke-limit-probe", os.MFD_CLOEXEC); '
            'os.ftruncate(fd, 2*1024*1024); os.close(fd)')
    try:
        result = subprocess.run([sys.executable, '-B', '-c', code],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                timeout=timeout)
        return dict(exit_status(result.returncode), requested_bytes=2*1024*1024,
                    file_size_limit=actual_limit[0], hard_file_size_limit=actual_limit[1],
                    gates_application=False)
    except (OSError, subprocess.SubprocessError) as error:
        return {'error_type': type(error).__name__, 'gates_application': False}


def profile_log_evidence(root):
    """Read only regular, singly linked logs through retained nofollow directories."""
    result = {'files': [], 'visited': 0, 'bytes': 0, 'truncated': False, 'errors': []}
    def visit(directory, prefix):
        with os.scandir(directory) as rows:
            for row in rows:
                if result['visited'] >= 256 or len(result['files']) >= 8:
                    result['truncated'] = True
                    return
                result['visited'] += 1
                name = prefix + row.name
                fd = None
                try:
                    info = row.stat(follow_symlinks=False)
                    if stat.S_ISDIR(info.st_mode):
                        fd = os.open(row.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                                     dir_fd=directory)
                        visit(fd, name + '/')
                    elif stat.S_ISREG(info.st_mode) and '.log' in row.name:
                        fd = os.open(row.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                                     dir_fd=directory)
                        current = os.fstat(fd)
                        if not stat.S_ISREG(current.st_mode) or current.st_nlink != 1:
                            continue
                        budget = min(16*1024, 64*1024-result['bytes'])
                        if budget <= 0:
                            result['truncated'] = True
                            return
                        # Last bounded bytes often contain the startup failure.
                        os.lseek(fd, max(0, current.st_size-budget), os.SEEK_SET)
                        raw = os.read(fd, budget)
                        result['bytes'] += len(raw)
                        result['files'].append({'path': name[:512], 'size': current.st_size,
                                                'tail_bytes': len(raw),
                                                'tail': raw.decode('utf-8', errors='replace')})
                except OSError as error:
                    if len(result['errors']) < 8:
                        result['errors'].append({'path': name[:512], 'errno': error.errno})
                finally:
                    if fd is not None:
                        os.close(fd)
    fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        visit(fd, '')
    finally:
        os.close(fd)
    return result


def start_log_reader(source, sink):
    """Retain a bounded prefix while draining all remaining output without blocking it."""
    state = {'observed_bytes': 0, 'retained_bytes': 0, 'discarded_bytes': 0,
             'eof': False, 'stop_at': None, 'error': None}
    def close_streams():
        for owned in (source, sink):
            try:
                owned.close()
            except (OSError, ValueError) as error:
                state['error'] = type(error).__name__
    def drain():
        try:
            fd = source.fileno()
            while state['stop_at'] is None or time.monotonic() < state['stop_at']:
                ready, _, _ = select.select([fd], [], [], .05)
                if not ready:
                    continue
                data = os.read(fd, 64*1024)
                if not data:
                    state['eof'] = True
                    break
                keep = data[:max(0, MAX_RETAINED_LOG-state['retained_bytes'])]
                if keep:
                    sink.write(keep)
                    sink.flush()
                state['observed_bytes'] += len(data)
                state['retained_bytes'] += len(keep)
                state['discarded_bytes'] += len(data)-len(keep)
        except (OSError, ValueError) as error:
            state['error'] = type(error).__name__
        finally:
            close_streams()
    thread = threading.Thread(target=drain, daemon=True)
    try:
        thread.start()
    except BaseException:
        close_streams()
        raise
    return thread, state


def stop_log_readers(readers, grace=1):
    # One shared shutdown deadline, including a pipe held open by a late child.
    deadline = time.monotonic() + grace
    for _, state in readers:
        state['stop_at'] = deadline
    for thread, _ in readers:
        thread.join(max(0, deadline+.2-time.monotonic()))
    return all(not thread.is_alive() for thread, _ in readers)


def main():
    require(os.geteuid() != 0, "Application must run as an unprivileged user")
    interfaces = {row.split(':', 1)[0].strip() for row in Path('/proc/net/dev').read_text().splitlines() if ':' in row}
    require(interfaces == {'lo'}, 'Unexpected network interface')
    status = Path('/proc/self/status').read_text().splitlines()
    require(next(row.split()[1] for row in status if row.startswith('CapEff:')) == '0000000000000000',
            'Runtime has effective capabilities')
    require(os.environ.get('HOME') == '/home/smoke', "Private HOME required")
    require(not list(Path('/home/smoke').iterdir()), "Profile is not fresh")
    require('GH_TOKEN' not in os.environ and 'GITHUB_TOKEN' not in os.environ,
            "Runtime must not receive GitHub credentials")
    for name in ('config', 'cache', 'data', 'runtime'):
        Path('/home/smoke', name).mkdir(mode=0o700)
    os.environ.update(XDG_CONFIG_HOME='/home/smoke/config', XDG_CACHE_HOME='/home/smoke/cache',
                      XDG_DATA_HOME='/home/smoke/data', XDG_RUNTIME_DIR='/home/smoke/runtime',
                      DISPLAY=':99', NO_AT_BRIDGE='0', GTK_A11Y='always', GTK_MODULES='atk-bridge')
    deadline = time.monotonic() + 120
    # Set inherited limits before any output-drain thread exists. No threaded preexec.
    child_limits()
    children, streams, readers = [], [], []
    app = None
    result = {"ready": False, "external_network": False, "app_reaped": False,
              'uid': os.geteuid(), 'interfaces': sorted(interfaces),
              'runtime_file_limit_bytes': resource.getrlimit(resource.RLIMIT_FSIZE)[0],
              'retained_log_limit_bytes': MAX_RETAINED_LOG,
              'net_namespace': os.readlink('/proc/self/ns/net'),
              'mount_namespace': os.readlink('/proc/self/ns/mnt')}
    require(result['net_namespace'] != os.environ['SMOKE_HOST_NET_NS'], 'Host network namespace retained')
    require(result['mount_namespace'] != os.environ['SMOKE_HOST_MNT_NS'], 'Host mount namespace retained')
    require(not Path('/run/dbus/system_bus_socket').exists(), 'Host system bus is visible')
    require(not any(key.startswith(('GH_', 'GITHUB_', 'WEBKIT_DISABLE_')) for key in os.environ),
            'Forbidden runtime environment')

    def left(cap):
        remaining = deadline - time.monotonic()
        require(remaining > 0, "Original application deadline exceeded")
        return min(cap, remaining)

    def launch(args, name, **kwargs):
        log = (EVIDENCE / name).open('wb')
        streams.append(log)
        process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   bufsize=0, **kwargs)
        children.append(process)
        readers.append(start_log_reader(process.stdout, log))
        return process

    try:
        result['phase'] = 'screenshot-runtime-preflight'
        screenshot_args = screenshot_command()
        result['screenshot_binary'] = screenshot_args[0]
        result['phase'] = 'namespace-preflight'
        # Test the same namespace facilities WebKit needs; never disable its sandbox.
        nested = subprocess.run(['/usr/bin/bwrap', '--unshare-user', '--unshare-pid', '--unshare-net',
                                 '--ro-bind', '/', '/', '--proc', '/proc', '--',
                                 sys.executable, '-c',
                                 'import os; print(os.readlink("/proc/self/ns/net"))'],
                                capture_output=True, timeout=left(8), check=True)
        require(nested.stdout.decode().strip() != result['net_namespace'], 'Nested network namespace unavailable')
        result['nested_namespace_preflight'] = True
        result['phase'] = 'private-display-and-keyring'
        xvfb = launch(['Xvfb', ':99', '-screen', '0', '1280x900x24', '-nolisten', 'tcp'], 'xvfb.log')
        while not Path('/tmp/.X11-unix/X99').exists():
            require(xvfb.poll() is None, 'Xvfb exited')
            left(1)
            time.sleep(.05)
        keyring = launch(['gnome-keyring-daemon', '--foreground', '--unlock', '--components=secrets',
                          '--control-directory=/home/smoke/runtime/keyring'], 'keyring.log',
                         stdin=subprocess.PIPE)
        keyring.stdin.write(os.urandom(24).hex().encode())
        keyring.stdin.close()
        # Confirm only the disposable session's Secret Service. No host bus is mounted.
        subprocess.run(['gdbus', 'wait', '--session', '--timeout', '10', 'org.freedesktop.secrets'],
                       check=True, timeout=left(12), stdout=subprocess.DEVNULL)
        executable = Path('/package/usr/bin/inverter-dashboard')
        require(executable.is_file() and not executable.is_symlink(), 'Exact package executable missing')
        result['memfd_limit_probe'] = memfd_limit_probe(left(5))
        result['phase'] = 'application-readiness'
        app = launch([str(executable)], 'application.log', stdin=subprocess.DEVNULL,
                     cwd='/home/smoke')
        while True:
            status = app.poll()
            result['app_before_cleanup'] = exit_status(status)
            require(status is None, 'Packaged application exited before readiness')
            try:
                found = subprocess.run([sys.executable, '-B', __file__, '--probe', str(app.pid)],
                                       capture_output=True, timeout=left(4))
                require(len(found.stdout) <= 512 * 1024 and len(found.stderr) <= 64 * 1024,
                        'Probe output bound')
                result['last_accessibility_probe'] = dict(exit_status(found.returncode),
                    stderr_tail=found.stderr[-4096:].decode('utf-8', errors='replace'))
                evidence = json.loads(found.stdout) if found.returncode == 0 else None
            except (subprocess.TimeoutExpired, json.JSONDecodeError) as error:
                result['last_accessibility_probe'] = {'error_type': type(error).__name__}
                evidence = None
            if evidence and evidence['pid'] == app.pid and evidence['ready']:
                break
            left(1)
            time.sleep(.25)
        result['phase'] = 'rendered-evidence'
        (EVIDENCE / 'accessibility.json').write_text(json.dumps(evidence, indent=2) + '\n')
        subprocess.run(screenshot_args,
                       check=True, timeout=left(8), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        screenshot = (EVIDENCE / 'rendered.png').read_bytes()
        require(0 < len(screenshot) <= 4 * 1024 * 1024 and screenshot[:8] == b'\x89PNG\r\n\x1a\n',
                'Screenshot is missing or outside bounds')
        require(struct.unpack('>II', screenshot[16:24]) == (1280, 900), 'Screenshot dimensions differ')
        from PIL import Image
        with Image.open(io.BytesIO(screenshot)) as image:
            require(image.size == (1280, 900), 'Decoded screenshot dimensions differ')
            heading = next(row for row in evidence['rows'] if EXPECTED[0] in (row['name'], row['text'])
                           and row['visible'] and row['showing'] and row['width'] > 0 and row['height'] > 0)
            left, top = max(0, heading['x']), max(0, heading['y'])
            right, bottom = min(1280, heading['x'] + heading['width']), min(900, heading['y'] + heading['height'])
            require(left < right and top < bottom, 'Heading screenshot crop is outside viewport')
            colors = image.crop((left, top, right, bottom)).convert('RGB').getcolors(1280 * 900)
            require(visible_contrast(colors), 'Heading screenshot is blank, unpainted or lacks contrast')
        require(app.poll() is None, 'Application exited during rendered evidence')
        result.update(ready=True, rendered_heading_colors=len(colors), expected_text=list(EXPECTED),
                      executable_sha256=hashlib.sha256(executable.read_bytes()).hexdigest(),
                      screenshot_sha256=hashlib.sha256(screenshot).hexdigest())
    except Exception as error:
        result['failure'] = {'type': type(error).__name__, 'message': str(error)[:512]}
        raise
    finally:
        if app is not None:
            result['app_before_cleanup'] = exit_status(app.poll())
        for process in reversed(children):
            stop(process)
        result['log_readers_stopped'] = stop_log_readers(readers)
        result['log_captures'] = [{k: v for k, v in state.items() if k != 'stop_at'}
                                  for _, state in readers]
        if not result['log_readers_stopped']:
            result['ready'] = False
        for stream in streams:
            if not stream.closed and result['log_readers_stopped']:
                stream.close()
        if app is not None:
            result['app_after_cleanup'] = exit_status(app.returncode)
        if not result['ready']:
            try:
                result['profile_log_evidence'] = profile_log_evidence('/home/smoke')
            except OSError as error:
                result['profile_log_evidence'] = {'error_type': type(error).__name__, 'errno': error.errno}
        result['app_reaped'] = bool(children) and all(p.returncode is not None for p in children)
        result['elapsed_seconds'] = round(120 - (deadline - time.monotonic()), 3)
        result['deadline_seconds'] = 120
        (EVIDENCE / 'inside-result.json').write_text(json.dumps(result, indent=2) + '\n')
    require(result['ready'] and result['app_reaped'] and result['log_readers_stopped']
            and all(row['error'] is None for row in result['log_captures']),
            'Rendered application acceptance failed')


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--probe':
        probe(int(sys.argv[2]))
    else:
        main()

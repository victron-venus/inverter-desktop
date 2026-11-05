"""Observe the actual packaged app inside a disposable offline Linux namespace."""
import hashlib
import io
import json
import os
from pathlib import Path
import resource
import signal
import struct
import subprocess
import sys
import time

EXPECTED = ("Welcome — Setup", "Setup sections", "Connection mode")
EVIDENCE = Path("/evidence")


def require(value, message):
    if not value:
        raise RuntimeError(message)


def child_limits():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_FSIZE, (1024 * 1024, 1024 * 1024))


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
    children, streams = [], []
    result = {"ready": False, "external_network": False, "app_reaped": False,
              'uid': os.geteuid(), 'interfaces': sorted(interfaces),
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
        process = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT,
                                   preexec_fn=child_limits, **kwargs)
        children.append(process)
        return process

    try:
        # Test the same namespace facilities WebKit needs; never disable its sandbox.
        nested = subprocess.run(['/usr/bin/bwrap', '--unshare-user', '--unshare-pid', '--unshare-net',
                                 '--ro-bind', '/', '/', '--proc', '/proc', '--',
                                 sys.executable, '-c',
                                 'import os; print(os.readlink("/proc/self/ns/net"))'],
                                capture_output=True, timeout=left(8), check=True)
        require(nested.stdout.decode().strip() != result['net_namespace'], 'Nested network namespace unavailable')
        result['nested_namespace_preflight'] = True
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
        app = launch([str(executable)], 'application.log', stdin=subprocess.DEVNULL,
                     cwd='/home/smoke')
        while True:
            require(app.poll() is None, 'Packaged application exited before readiness')
            try:
                found = subprocess.run([sys.executable, '-B', __file__, '--probe', str(app.pid)],
                                       capture_output=True, timeout=left(4))
                require(len(found.stdout) <= 512 * 1024 and len(found.stderr) <= 64 * 1024,
                        'Probe output bound')
                evidence = json.loads(found.stdout) if found.returncode == 0 else None
            except (subprocess.TimeoutExpired, json.JSONDecodeError):
                evidence = None
            if evidence and evidence['pid'] == app.pid and evidence['ready']:
                break
            left(1)
            time.sleep(.25)
        (EVIDENCE / 'accessibility.json').write_text(json.dumps(evidence, indent=2) + '\n')
        subprocess.run(['import', '-display', ':99', '-window', 'root', 'png:/evidence/rendered.png'],
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
    finally:
        for process in reversed(children):
            stop(process)
        for stream in streams:
            stream.close()
        result['app_reaped'] = bool(children) and all(p.returncode is not None for p in children)
        result['elapsed_seconds'] = round(120 - (deadline - time.monotonic()), 3)
        result['deadline_seconds'] = 120
        (EVIDENCE / 'inside-result.json').write_text(json.dumps(result, indent=2) + '\n')
    require(result['ready'] and result['app_reaped'], 'Rendered application acceptance failed')


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--probe':
        probe(int(sys.argv[2]))
    else:
        main()

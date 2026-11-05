"""Offline contracts for extraction, visible readiness, isolation and lifetime."""
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tarfile
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


inside = module('smoke_inside', ROOT / 'scripts/rc_smoke/inside.py')
launch = module('smoke_launch', ROOT / 'scripts/rc_smoke/launch.py')
prepare = module('smoke_prepare', ROOT / 'scripts/rc_smoke/prepare.py')
watchdog = module('smoke_watchdog', Path(os.environ.get('RC_SMOKE_TEST_WATCHDOG',
                  ROOT / 'tests/tls-policy/run.py')))


def deb(entries, *, pax=None, tail=b''):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w', format=tarfile.PAX_FORMAT, pax_headers=pax) as archive:
        for name, payload, kind in entries:
            row = tarfile.TarInfo(name)
            row.mode = 0o755 if name.endswith('inverter-dashboard') else 0o644
            row.type = kind
            row.linkname = '/tmp/foreign' if kind == tarfile.SYMTYPE else ''
            row.size = len(payload) if kind == tarfile.REGTYPE else 0
            archive.addfile(row, io.BytesIO(payload) if kind == tarfile.REGTYPE else None)
    output = b'!<arch>\n'
    for name, payload in [('debian-binary', b'2.0\n'), ('data.tar.gz', prepare.gzip.compress(buffer.getvalue() + tail))]:
        header = f'{name + "/":<16}{0:<12}{0:<6}{0:<6}{100644:<8}{len(payload):<10}`\n'.encode()
        output += header + payload + (b'\n' if len(payload) % 2 else b'')
    return output


class Extraction(unittest.TestCase):
    def test_actual_bytes_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'package'
            digest, count, size = prepare.extract(deb([('usr/bin/inverter-dashboard', b'fixture', tarfile.REGTYPE)]), target)
            self.assertEqual((target / 'usr/bin/inverter-dashboard').read_bytes(), b'fixture')
            self.assertEqual((count, size), (1, 7))
            self.assertEqual(digest, prepare.hashlib.sha256(b'fixture').hexdigest())

    def test_traversal_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, 'Unsafe package'):
                prepare.extract(deb([('../escape', b'x', tarfile.REGTYPE)]), Path(directory) / 'package')
            self.assertFalse((Path(directory) / 'escape').exists())

    def test_link_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, 'Links'):
                prepare.extract(deb([('usr/link', b'', tarfile.SYMTYPE)]), Path(directory) / 'package')

    def test_duplicate_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            rows = [('usr/bin/inverter-dashboard', b'x', tarfile.REGTYPE)] * 2
            with self.assertRaisesRegex(RuntimeError, 'Duplicate'):
                prepare.extract(deb(rows), Path(directory) / 'package')

    def test_expanded_size_rejected(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(prepare, 'MAX_EXPANDED', 2):
            with self.assertRaisesRegex(RuntimeError, 'Expanded archive cap'):
                prepare.extract(deb([('usr/bin/inverter-dashboard', b'abc', tarfile.REGTYPE)]), Path(directory) / 'package')

    def test_pax_metadata_counts_against_decompression_cap(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(prepare, 'MAX_EXPANDED', 16384):
            archive = deb([('usr/bin/inverter-dashboard', b'x', tarfile.REGTYPE)], pax={'comment': 'x' * 20000})
            with self.assertRaisesRegex(RuntimeError, 'Expanded archive cap'):
                prepare.extract(archive, Path(directory) / 'package')

    def test_hidden_archive_tail_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = deb([('usr/bin/inverter-dashboard', b'x', tarfile.REGTYPE)], tail=b'hidden' + bytes(506))
            with self.assertRaisesRegex(RuntimeError, 'Non-padding tar tail'):
                prepare.extract(archive, Path(directory) / 'package')

    def test_truncated_end_marker_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'second tar end'):
            prepare.validate_tar_end(bytes(512))

    def test_bounded_tar_supported_compressions(self):
        raw = bytes(1024)
        variants = [('data.tar', raw), ('data.tar.gz', prepare.gzip.compress(raw)),
                    ('data.tar.bz2', prepare.bz2.compress(raw)),
                    ('data.tar.xz', prepare.lzma.compress(raw, preset=0))]
        for name, data in variants:
            with self.subTest(name=name):
                self.assertEqual(prepare.bounded_tar((name, data)), raw)

    def test_auth_not_forwarded(self):
        request = prepare.urllib.request.Request('https://api.github.com/repos/x', headers={'Authorization': 'Bearer inert'})
        result = prepare.PublicAssetRedirect().redirect_request(request, None, 302, '', {},
                      'https://release-assets.githubusercontent.com/inert')
        self.assertIsNone(result.get_header('Authorization'))
        with self.assertRaisesRegex(RuntimeError, 'Unexpected asset'):
            prepare.PublicAssetRedirect().redirect_request(request, None, 302, '', {}, 'https://example.invalid/a')


class RenderedReadiness(unittest.TestCase):
    def rows(self):
        return [dict(name=t, text='', showing=True, visible=True, x=10, y=10, width=100, height=20)
                for t in inside.EXPECTED]

    def test_three_visible_production_labels(self):
        self.assertTrue(inside.visible_setup(self.rows()))

    def test_rendered_heading_contrast(self):
        self.assertFalse(inside.visible_contrast([(100, (0, 0, 0))]))
        self.assertFalse(inside.visible_contrast([(1, (i, i, i)) for i in range(8)]))
        self.assertTrue(inside.visible_contrast([(1, (i * 30, i * 30, i * 30)) for i in range(8)]))

    def test_blank_window_rejected(self):
        self.assertFalse(inside.visible_setup([]))

    def test_hidden_or_offscreen_rejected(self):
        for field, value in [('showing', False), ('visible', False), ('width', 0), ('x', -200), ('y', 1000)]:
            rows = self.rows()
            rows[0][field] = value
            self.assertFalse(inside.visible_setup(rows), field)


class Isolation(unittest.TestCase):
    def test_explicit_namespaces_and_no_host_profile(self):
        args = launch.sandbox_command(Path('/owned'), Path('/package-input'), Path('/output'), 7, 1001, 1001, 'net:[1]', 'mnt:[1]')
        for flag in ('--unshare-user', '--unshare-ipc', '--unshare-pid', '--unshare-net', '--unshare-uts',
                     '--clearenv', '--die-with-parent', '--new-session', '--sync-fd'):
            self.assertIn(flag, args)
        for bad in ('--disable-userns', '--unshare-user-try', '--share-net', '--bind-try', '/run/user', '--dev-bind'):
            self.assertNotIn(bad, args)
        self.assertEqual(args[args.index('--uid') + 1], '1001')
        self.assertEqual(args[args.index('--remount-ro') + 1], '/')
        binds = [args[i + 1:i + 3] for i, value in enumerate(args) if value in ('--bind', '--ro-bind')]
        self.assertNotIn(['/', '/'], binds)
        self.assertNotIn(['/home', '/home'], binds)
        self.assertNotIn(['/run', '/run'], binds)
        self.assertIn(['/owned/home', '/home/smoke'], binds)

    def test_lifetime_eof_required(self):
        read_fd, write_fd = os.pipe()
        try:
            self.assertFalse(launch.lifetime_closed(read_fd, launch.time.monotonic() + .02))
            os.close(write_fd)
            write_fd = -1
            self.assertTrue(launch.lifetime_closed(read_fd, launch.time.monotonic() + .1))
        finally:
            os.close(read_fd)
            if write_fd >= 0:
                os.close(write_fd)


class Lifetime(unittest.TestCase):
    def run_child(self, code, timeout=2):
        with tempfile.TemporaryDirectory() as directory:
            read_fd, write_fd = os.pipe()
            return launch.supervise([sys.executable, '-B', '-c', code], read_fd, write_fd,
                                    Path(directory), watchdog, timeout=timeout, grace=1)

    def test_success_and_signal_handlers_restored(self):
        before = {s: signal.getsignal(s) for s in (signal.SIGINT, signal.SIGTERM)}
        result = self.run_child('pass')
        self.assertTrue(result['success'])
        self.assertTrue(result['process_group_clean'] and result['sandbox_lifetime_closed'])
        self.assertEqual(before, {s: signal.getsignal(s) for s in before})

    def test_timeout_cleans_actual_child(self):
        result = self.run_child('import time; time.sleep(5)', timeout=.2)
        self.assertFalse(result['success'])
        self.assertEqual(result['reason'], 'timeout')
        self.assertTrue(result['process_group_clean'] and result['sandbox_lifetime_closed'])
        self.assertLess(result['elapsed_seconds'], 4)

    def test_descendant_cleanup(self):
        code = 'import subprocess,sys,time; subprocess.Popen([sys.executable,"-c","import time; time.sleep(5)"]); time.sleep(5)'
        result = self.run_child(code, timeout=1)
        self.assertFalse(result['success'])
        self.assertTrue(result['process_group_clean'] and result['sandbox_lifetime_closed'])

    def test_nonzero_fails(self):
        result = self.run_child('raise SystemExit(7)')
        self.assertFalse(result['success'])
        self.assertEqual(result['child_returncode'], 7)


class FailureDiagnostics(unittest.TestCase):
    def test_natural_signal_and_cleanup_status_are_distinct(self):
        self.assertEqual(inside.exit_status(-signal.SIGXFSZ),
                         {'returncode': -signal.SIGXFSZ, 'signal': 'SIGXFSZ'})
        self.assertEqual(inside.exit_status(None), {'returncode': None, 'signal': None})
        self.assertEqual(inside.exit_status(7), {'returncode': 7, 'signal': None})

    def test_memfd_probe_reports_limit_without_gating_app(self):
        with patch.object(inside.subprocess, 'run', return_value=subprocess.CompletedProcess([], -signal.SIGXFSZ)) as run:
            result = inside.memfd_limit_probe(2)
        self.assertEqual(result['signal'], 'SIGXFSZ')
        self.assertFalse(result['gates_application'])
        self.assertEqual(run.call_args.kwargs['timeout'], 2)
        self.assertNotIn('preexec_fn', run.call_args.kwargs)
        self.assertEqual(result['file_size_limit'], inside.resource.getrlimit(inside.resource.RLIMIT_FSIZE)[0])

    def test_memfd_probe_failure_remains_diagnostic(self):
        with patch.object(inside.subprocess, 'run', side_effect=subprocess.TimeoutExpired('probe', 1)):
            self.assertEqual(inside.memfd_limit_probe(1),
                             {'error_type': 'TimeoutExpired', 'gates_application': False})

    def test_profile_evidence_does_not_follow_links_or_read_nonlogs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'home'; root.mkdir()
            outside = Path(directory) / 'outside.log'; outside.write_text('outside-secret')
            (root / 'linked.log').symlink_to(outside)
            (root / 'linked-dir').symlink_to(outside.parent, target_is_directory=True)
            (root / 'config.json').write_text('not-a-log')
            (root / 'app.log').write_text('native-startup')
            result = inside.profile_log_evidence(root)
        self.assertEqual([f['path'] for f in result['files']], ['app.log'])
        self.assertEqual(result['files'][0]['tail'], 'native-startup')
        self.assertNotIn('outside-secret', json.dumps(result))

    def test_profile_evidence_byte_and_count_bounds(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for i in range(12):
                (root / f'{i}.log').write_bytes(b'x' * 20000)
            result = inside.profile_log_evidence(root)
        self.assertLessEqual(result['bytes'], 64*1024)
        self.assertLessEqual(len(result['files']), 8)
        self.assertTrue(result['truncated'])
        self.assertTrue(all(f['tail_bytes'] <= 16*1024 for f in result['files']))

    def test_profile_root_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); (root / 'real').mkdir()
            (root / 'link').symlink_to(root / 'real', target_is_directory=True)
            with self.assertRaises(OSError):
                inside.profile_log_evidence(root / 'link')


class RuntimeLimits(unittest.TestCase):
    def file_probe(self, size):
        code = ('import os,resource,signal,tempfile; '
                f'resource.setrlimit(resource.RLIMIT_FSIZE, ({inside.MAX_RUNTIME_FILE}, {inside.MAX_RUNTIME_FILE})); '
                'signal.signal(signal.SIGXFSZ, signal.SIG_DFL); '
                'f=tempfile.TemporaryFile(); '
                f'os.ftruncate(f.fileno(), {size})')
        return subprocess.run([sys.executable, '-B', '-c', code],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)

    def test_runtime_file_within_cap_succeeds(self):
        self.assertEqual(self.file_probe(2*1024*1024).returncode, 0)

    def test_runtime_file_above_cap_is_rejected(self):
        self.assertEqual(self.file_probe(inside.MAX_RUNTIME_FILE+1).returncode, -signal.SIGXFSZ)

    def test_flood_is_drained_but_retained_output_is_capped(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'output.log'
            process = subprocess.Popen([sys.executable, '-B', '-c',
                'import sys; sys.stdout.buffer.write(b"x" * (3*1024*1024))'],
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, bufsize=0)
            reader = inside.start_log_reader(process.stdout, path.open('wb'))
            try:
                self.assertEqual(process.wait(timeout=5), 0)
                reader[0].join(timeout=2)
                self.assertTrue(inside.stop_log_readers([reader]))
                state = reader[1]
                self.assertTrue(state['eof'])
                self.assertIsNone(state['error'])
                self.assertEqual(state['observed_bytes'], 3*1024*1024)
                self.assertEqual(state['retained_bytes'], inside.MAX_RETAINED_LOG)
                self.assertEqual(state['discarded_bytes'], 2*1024*1024)
                self.assertEqual(path.stat().st_size, inside.MAX_RETAINED_LOG)
            finally:
                if process.poll() is None:
                    process.kill(); process.wait(timeout=2)
                inside.stop_log_readers([reader])

    def close_error_case(self, which):
        read_fd, write_fd = os.pipe()
        os.close(write_fd)
        source = os.fdopen(read_fd, 'rb', buffering=0)
        sink = tempfile.TemporaryFile()
        wrappers = [Mock(wraps=source), Mock(wraps=sink)]
        actual = (source, sink)[which]
        def failing_close():
            actual.close()
            raise OSError('inert close-only failure')
        wrappers[which].close.side_effect = failing_close
        reader = inside.start_log_reader(*wrappers)
        try:
            reader[0].join(timeout=2)
            self.assertTrue(inside.stop_log_readers([reader]))
            self.assertEqual(reader[1]['error'], 'OSError')
            self.assertTrue(source.closed and sink.closed)
            for wrapper in wrappers:
                wrapper.close.assert_called_once()
        finally:
            source.close(); sink.close()
            inside.stop_log_readers([reader])

    def test_source_close_only_error_is_recorded_and_sink_closed(self):
        self.close_error_case(0)

    def test_sink_close_only_error_is_recorded_and_source_closed(self):
        self.close_error_case(1)

    def test_late_pipe_writer_cannot_extend_log_shutdown(self):
        read_fd, write_fd = os.pipe()
        with tempfile.TemporaryDirectory() as directory:
            source = os.fdopen(read_fd, 'rb', buffering=0)
            sink = (Path(directory)/'output.log').open('wb')
            reader = inside.start_log_reader(source, sink)
            started = time.monotonic()
            try:
                self.assertTrue(inside.stop_log_readers([reader], grace=.1))
                self.assertLess(time.monotonic()-started, 2)
                self.assertTrue(source.closed and sink.closed)
                self.assertFalse(reader[1]['eof'])
            finally:
                os.close(write_fd)
                inside.stop_log_readers([reader])


if __name__ == '__main__':
    unittest.main()

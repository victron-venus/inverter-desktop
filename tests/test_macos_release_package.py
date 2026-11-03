"""Native signing and archive regressions; no application is ever launched."""

import importlib.util
import json
import subprocess
import traceback
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "macos_release", ROOT / "scripts/macos_release.py"
)
macos_release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(macos_release)
ATTACH_SPEC = importlib.util.spec_from_file_location(
    "macos_fixture_attach", ROOT / "tests/macos_fixture_attach.py"
)
fixture_attach = importlib.util.module_from_spec(ATTACH_SPEC)
ATTACH_SPEC.loader.exec_module(fixture_attach)


def run_fixture_tool(arguments):
    """Keep failed native fixture commands fatal and expose bounded diagnostics."""
    try:
        return subprocess.run(
            arguments, check=True, capture_output=True, timeout=60
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        for stream in ("stdout", "stderr"):
            output = getattr(error, stream) or b""
            tail = output[-4096:]
            if isinstance(tail, bytes):
                tail = tail.decode("utf-8", errors="replace")
            prefix = (
                "[truncated; last 4096 bytes/characters] "
                if len(output) > 4096
                else ""
            )
            error.add_note(f"Native fixture {stream}: {prefix}{tail or '[empty]'}")
        raise


class FixtureToolDiagnosticsTests(unittest.TestCase):
    """Failure reports retain native causes without retries or exception changes."""

    def test_native_failures_keep_the_original_exception_and_visible_output(self):
        arguments = ["/usr/bin/hdiutil", "create", "fixture.dmg"]
        for error in (
            subprocess.CalledProcessError(
                1, arguments, output=b"progress", stderr=b"create failed: resource busy"
            ),
            subprocess.TimeoutExpired(
                arguments, 60, output="partial progress", stderr="still creating"
            ),
        ):
            with self.subTest(error=type(error).__name__):
                with patch.object(subprocess, "run", side_effect=error) as run:
                    with self.assertRaises(type(error)) as caught:
                        run_fixture_tool(arguments)
                self.assertIs(caught.exception, error)
                self.assertEqual(run.call_count, 1)
                rendered = "".join(traceback.format_exception(error))
                for stream in ("stdout", "stderr"):
                    output = getattr(error, stream)
                    if isinstance(output, bytes):
                        output = output.decode("utf-8")
                    self.assertIn(f"Native fixture {stream}: {output}", rendered)

    def test_diagnostics_bound_large_output_and_handle_empty_or_invalid_text(self):
        error = subprocess.TimeoutExpired(
            ["/usr/bin/hdiutil"], 60, output=b"x" * 10000 + b"\xfffinal cause"
        )
        with patch.object(subprocess, "run", side_effect=error):
            with self.assertRaises(subprocess.TimeoutExpired):
                run_fixture_tool(error.cmd)
        self.assertEqual(len(error.stdout), 10012)
        self.assertLess(len(error.__notes__[0]), 4200)
        self.assertIn("[truncated; last 4096 bytes/characters]", error.__notes__[0])
        self.assertTrue(error.__notes__[0].endswith("\ufffdfinal cause"))
        self.assertEqual(error.__notes__[1], "Native fixture stderr: [empty]")


class MacOSSigningConfigurationTests(unittest.TestCase):
    """The default release profile seals the app without paid credentials."""

    def test_ad_hoc_identity_is_explicit(self):
        """A linker-only executable signature must not become the release seal."""
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_bytes())
        self.assertEqual(config["bundle"]["macOS"]["signingIdentity"], "-")




if __name__ == "__main__":
    unittest.main()

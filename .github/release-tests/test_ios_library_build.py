"""Exercise the production iOS build order and fail-closed artifact staging."""

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


BUILDER = Path(__file__).resolve().parents[2] / "scripts/ci-build-ios-library.sh"
STAGED = "src-tauri/gen/apple/Externals/arm64/release/libapp.a"


class IosLibraryBuildTests(unittest.TestCase):
    """Run the real shell entry point with disposable build commands."""

    def run_builder(self, frontend_status=0, rust_status=0, skip_frontend=False, xcode_major=26):
        """Return the process result and staged bytes without invoking toolchains."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            commands = root / "commands"
            commands.mkdir()
            (commands / "xcodebuild").write_text(
                f"#!{sys.executable}\nimport os, sys\n"
                "assert sys.argv[1:] == ['-version']\n"
                "print('Xcode ' + os.environ['IOS_TEST_XCODE_MAJOR'] + '.0')\n"
            )
            (commands / "xcrun").write_text(
                f"#!{sys.executable}\nimport sys\nfrom pathlib import Path\n"
                "assert sys.argv[1:] == ['--find', 'swift']\n"
                "print(Path(__file__).with_name('swift'))\n"
            )
            (commands / "swift").write_text(
                f"#!{sys.executable}\nimport os, sys\n"
                "expected = ['build', '--configuration', 'release']\n"
                "if int(os.environ['IOS_TEST_XCODE_MAJOR']) >= 27:\n"
                "    expected = ['build', '--build-system', 'native', "
                "'--configuration', 'release']\n"
                "assert sys.argv[1:] in (expected, ['--version']), sys.argv\n"
            )
            pnpm = commands / "pnpm"
            pnpm.write_text(
                f"#!{sys.executable}\n"
                "import os, sys\n"
                "from pathlib import Path\n"
                "assert sys.argv[1:] == ['run', 'build:mobile']\n"
                "status = int(os.environ['IOS_TEST_FRONTEND_STATUS'])\n"
                "if status: sys.exit(status)\n"
                "if os.environ['IOS_TEST_SKIP_FRONTEND'] != '1':\n"
                "    Path('dist').mkdir()\n"
                "    Path('dist/index.html').write_text('production assets')\n"
            )
            cargo = commands / "cargo"
            cargo.write_text(
                f"#!{sys.executable}\n"
                "import os, sys, subprocess\n"
                "from pathlib import Path\n"
                "Path('cargo-ran').touch()\n"
                "assert Path('dist/index.html').read_text() == 'production assets'\n"
                "assert '--locked' in sys.argv and '--release' in sys.argv\n"
                "assert sys.argv[sys.argv.index('--features') + 1] == 'tauri/custom-protocol'\n"
                "assert sys.argv[sys.argv.index('--target') + 1] == 'aarch64-apple-ios'\n"
                "subprocess.run(['swift', 'build', '--configuration', 'release'], check=True)\n"
                "subprocess.run(['swift', '--version'], check=True)\n"
                "status = int(os.environ['IOS_TEST_RUST_STATUS'])\n"
                "if status: sys.exit(status)\n"
                "path = Path('src-tauri/target/aarch64-apple-ios/release') "
                "/ 'libinverter_dashboard_lib.a'\n"
                "path.parent.mkdir(parents=True)\n"
                "path.write_bytes(b'production native library')\n"
            )
            pnpm.chmod(0o755)
            cargo.chmod(0o755)
            for name in ("xcodebuild", "xcrun", "swift"):
                (commands / name).chmod(0o755)
            result = subprocess.run(
                ["bash", str(BUILDER)],
                cwd=root,
                env={
                    **os.environ,
                    "PATH": str(commands) + os.pathsep + os.environ["PATH"],
                    "IOS_TEST_FRONTEND_STATUS": str(frontend_status),
                    "IOS_TEST_RUST_STATUS": str(rust_status),
                    "IOS_TEST_SKIP_FRONTEND": "1" if skip_frontend else "0",
                    "IOS_TEST_XCODE_MAJOR": str(xcode_major),
                    "TMPDIR": str(root),
                },
                capture_output=True,
                text=True,
                check=False,
            )
            staged = root / STAGED
            self.assertEqual(
                list(root.glob("inverter-ios-swift.*")), [], "Swift shim must be removed"
            )
            return (
                result,
                staged.read_bytes() if staged.exists() else None,
                (root / "cargo-ran").exists(),
            )

    def test_embeds_production_assets_before_staging_library(self):
        """The validation library has the same asset protocol as the release."""
        result, staged, cargo_ran = self.run_builder()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(staged, b"production native library")
        self.assertTrue(cargo_ran)

    def test_frontend_failure_stops_before_rust(self):
        """Preserve a failed frontend status rather than packaging stale output."""
        result, staged, cargo_ran = self.run_builder(frontend_status=37)
        self.assertEqual(result.returncode, 37, result.stderr)
        self.assertIsNone(staged)
        self.assertFalse(cargo_ran)

    def test_missing_frontend_stops_before_rust(self):
        """A successful command without the entry point cannot produce an IPA."""
        result, staged, cargo_ran = self.run_builder(skip_frontend=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(staged)
        self.assertFalse(cargo_ran)

    def test_rust_failure_is_not_staged(self):
        """Preserve native compilation failures and leave Xcode without a library."""
        result, staged, cargo_ran = self.run_builder(rust_status=42)
        self.assertEqual(result.returncode, 42, result.stderr)
        self.assertIsNone(staged)
        self.assertTrue(cargo_ran)

    def test_xcode_27_uses_native_backend_only_for_build_and_cleans_up(self):
        """Portable stubs verify actual wrapper arguments on success and Rust failure."""
        for status in (0, 42):
            with self.subTest(rust_status=status):
                result, staged, cargo_ran = self.run_builder(rust_status=status, xcode_major=27)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assertTrue(cargo_ran)
                self.assertEqual(staged, b"production native library" if status == 0 else None)


if __name__ == "__main__":
    unittest.main()

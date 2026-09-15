"""Exercise local Android build failure handling without a real SDK or build."""

import os
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "build-android-local.sh"
MOCK_TOOL = r'''
import os
import pathlib
import shutil
import sys

tool = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
step = os.environ.get("FAIL_STEP", "")
root = pathlib.Path.cwd()
with (root / "calls.log").open("a") as log:
    log.write(tool + " " + " ".join(args) + "\n")

def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value)

if tool == "java":
    print('openjdk version "21"')
elif tool == "rustup":
    print("aarch64-linux-android\narmv7-linux-androideabi\n"
          "x86_64-linux-android\ni686-linux-android")
elif tool == "sdkmanager":
    if step == "sdk":
        sys.exit(43)
    sdk = pathlib.Path(os.environ["ANDROID_HOME"])
    for package in args:
        if package.startswith("platforms;"):
            write(sdk / "platforms" / package.split(";")[1] / "android.jar", "sdk")
        elif package.startswith("build-tools;"):
            for name in ("zipalign", "apksigner"):
                dest = sdk / "build-tools" / package.split(";")[1] / name
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(root / "mock-bin" / "java", dest)
        elif package.startswith("ndk;"):
            (sdk / "ndk" / package.split(";")[1]).mkdir(parents=True)
elif tool == "pnpm":
    if args[0] == "install" and step == "dependencies":
        sys.exit(41)
    if args[:3] == ["tauri", "android", "build"]:
        if step == "build":
            sys.exit(42)
        if step != "no-output":
            outputs = root / "src-tauri/gen/android/app/build/outputs"
            if "--debug" in args:
                write(outputs / "apk/universal/debug/app-universal-debug.apk", "new debug")
            else:
                write(outputs / "apk/universal/release/app-universal-release-unsigned.apk", "new apk")
                if step != "missing-aab":
                    write(outputs / "bundle/universalRelease/app-universal-release.aab", "new aab")
elif tool == "zipalign":
    if step == "alignment":
        sys.exit(44)
    if "-c" not in args:
        shutil.copyfile(args[-2], args[-1])
elif tool == "apksigner":
    if step == "signing":
        sys.exit(45)
    if args[0] == "sign":
        shutil.copyfile(args[-1], args[args.index("--out") + 1])
elif tool == "brew":
    raise RuntimeError("Unexpected system package installation")
else:
    raise RuntimeError("Unexpected tool: " + tool)
'''


class AndroidLocalBuildTests(unittest.TestCase):
    """Run the real shell script against disposable tool and output fixtures."""

    def setUp(self):
        """Prepare an isolated SDK and executables for each script invocation."""
        # TestCase owns the context until its cleanup, including setup failures.
        # pylint: disable-next=consider-using-with
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        shutil.copy2(SCRIPT, self.root / SCRIPT.name)
        (self.root / "src-tauri/gen/android").mkdir(parents=True)
        (self.root / "src-tauri/Cargo.toml").write_text('version = "2.5.42"\n')
        (self.root / "pnpm-lock.yaml").write_text("retained lockfile\n")
        self.bin = self.root / "mock-bin"
        self.bin.mkdir()
        mock = f"#!{sys.executable}\n" + textwrap.dedent(MOCK_TOOL)
        for tool in ("pnpm", "java", "rustup", "brew"):
            self.make_tool(self.bin / tool, mock)
        self.sdk = self.root / "mock-sdk"
        self.make_tool(self.sdk / "cmdline-tools/latest/bin/sdkmanager", mock)
        for tool in ("zipalign", "apksigner"):
            self.make_tool(self.sdk / "build-tools/36.0.0" / tool, mock)
        platform = self.sdk / "platforms/android-36/android.jar"
        platform.parent.mkdir(parents=True)
        platform.write_text("sdk")
        (self.sdk / "ndk/28.2.13676358").mkdir(parents=True)
        self.env = {
            **os.environ,
            "PATH": f"{self.bin}:/usr/bin:/bin",
            "ANDROID_HOME": str(self.sdk),
            "NDK_HOME": "",
            "SIGN_APK": "false",
            "ANDROID_KEYSTORE_PATH": str(self.root / "test.keystore"),
            "ANDROID_KEYSTORE_PASSWORD": "fixture",
            "ANDROID_KEY_ALIAS": "fixture",
            "ANDROID_KEY_PASSWORD": "fixture",
        }
        (self.root / "test.keystore").write_text("mock-only")

    @staticmethod
    def make_tool(path, content):
        """Create an executable fixture at the tool's expected location."""
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        path.chmod(0o755)

    def run_build(self, *args, fail_step=""):
        """Run the copied script with optional arguments and one injected failure."""
        return subprocess.run(
            ["/bin/bash", str(self.root / SCRIPT.name), *args],
            cwd=self.root,
            env={**self.env, "FAIL_STEP": fail_step},
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            check=False,
            timeout=15,
        )

    def seed_stale_artifacts(self):
        """Seed previous native outputs and return the collected release path."""
        for artifact in (
            "src-tauri/gen/android/app/build/outputs/apk/universal/release/"
            "app-universal-release-unsigned.apk",
            "src-tauri/gen/android/app/build/outputs/bundle/universalRelease/"
            "app-universal-release.aab",
            "src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk",
        ):
            path = self.root / artifact
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("stale native output")
        dest = self.root / "dist/android/Inverter.Desktop_2.5.42.aab"
        dest.parent.mkdir(parents=True)
        dest.write_text("previous collected release")
        return dest

    def assert_failed_without_success(self, result):
        """Require both a failing exit code and the absence of a success banner."""
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertNotIn("Android build complete!", result.stdout)

    def test_dependency_failure_stops_before_build(self):
        """Do not invoke the Android build after dependency installation fails."""
        result = self.run_build(fail_step="dependencies")
        self.assert_failed_without_success(result)
        self.assertNotIn("pnpm tauri", (self.root / "calls.log").read_text())

    def test_failed_build_never_collects_stale_artifacts(self):
        """Preserve the old collected release when the native build fails."""
        previous = self.seed_stale_artifacts()
        result = self.run_build(fail_step="build")
        self.assert_failed_without_success(result)
        self.assertEqual(previous.read_text(), "previous collected release")
        self.assertNotIn("Collecting artifacts", result.stdout)

    def test_success_exit_with_no_outputs_cannot_reuse_stale_release(self):
        """Reject a successful build command that produces no new release files."""
        previous = self.seed_stale_artifacts()
        result = self.run_build(fail_step="no-output")
        self.assert_failed_without_success(result)
        self.assertIn("did not produce", result.stdout)
        self.assertEqual(previous.read_text(), "previous collected release")

    def test_success_exit_with_missing_aab_is_rejected(self):
        """Require the app bundle even when the release APK exists."""
        result = self.run_build(fail_step="missing-aab")
        self.assert_failed_without_success(result)
        self.assertIn("app-universal-release.aab", result.stdout)

    def test_debug_build_requires_a_fresh_apk(self):
        """Reject a debug build that leaves only an earlier APK behind."""
        self.seed_stale_artifacts()
        result = self.run_build("--dev", fail_step="no-output")
        self.assert_failed_without_success(result)
        self.assertIn("did not produce a debug APK", result.stdout)

    def test_missing_build_tools_are_installed_even_if_sdk_exists(self):
        """Install the matching build tools when the platform is already present."""
        shutil.rmtree(self.sdk / "build-tools")
        result = self.run_build()
        self.assertEqual(result.returncode, 0, result.stdout)
        calls = (self.root / "calls.log").read_text()
        self.assertIn("platforms;android-36 build-tools;36.0.0", calls)
        self.assertEqual(
            (self.root / "dist/android/Inverter.Desktop_2.5.42.aab").read_text(),
            "new aab",
        )

    def test_sdkmanager_failure_is_not_ignored(self):
        """Stop before dependency installation when SDK preparation fails."""
        shutil.rmtree(self.sdk / "platforms")
        result = self.run_build(fail_step="sdk")
        self.assert_failed_without_success(result)
        self.assertNotIn("pnpm install", (self.root / "calls.log").read_text())

    def test_signing_failure_cannot_report_success(self):
        """Keep the old collected release and fail when APK signing fails."""
        previous = self.seed_stale_artifacts()
        result = self.run_build("--sign", fail_step="signing")
        self.assert_failed_without_success(result)
        self.assertNotIn("APK signed:", result.stdout)
        self.assertEqual(previous.read_text(), "previous collected release")

    def test_signed_release_uses_16_kb_alignment_and_verification(self):
        """Align and verify the newly signed APK before collecting it."""
        result = self.run_build("--sign")
        self.assertEqual(result.returncode, 0, result.stdout)
        calls = (self.root / "calls.log").read_text()
        self.assertIn("zipalign -v -P 16 4", calls)
        self.assertIn("zipalign -c -P 16 4", calls)
        self.assertIn("apksigner verify --verbose", calls)
        self.assertEqual(
            (self.root / "dist/android/Inverter.Desktop_2.5.42_signed.apk").read_text(),
            "new apk",
        )

    def test_clean_keeps_frozen_lockfile_and_debug_build_succeeds(self):
        """Retain the frozen lockfile while cleaning and rebuilding a debug APK."""
        result = self.run_build("--clean", "--dev")
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertEqual((self.root / "pnpm-lock.yaml").read_text(), "retained lockfile\n")
        self.assertEqual(
            (self.root / "dist/android/app-universal-debug.apk").read_text(),
            "new debug",
        )


if __name__ == "__main__":
    unittest.main()

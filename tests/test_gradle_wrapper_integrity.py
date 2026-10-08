"""Exercise bootstrap integrity failures without executing untrusted Java."""

import importlib.util
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "verify_gradle_wrapper", ROOT / "scripts/android/verify_gradle_wrapper.py"
)
VERIFIER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFIER)


class GradleWrapperIntegrityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for relative in VERIFIER.CHECKSUMS:
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, path)

    def test_reviewed_bootstrap_passes(self):
        VERIFIER.verify(self.root)

    def _file_state(self):
        state = {}
        for path in self.root.rglob("*"):
            mode = path.lstat().st_mode
            if path.is_symlink():
                content = str(path.readlink())
            elif path.is_file():
                content = path.read_bytes()
            else:
                content = None
            state[str(path.relative_to(self.root))] = (mode, content)
        return state

    def _assert_rejected_without_changes(self, message):
        before = self._file_state()
        with self.assertRaisesRegex(ValueError, message):
            VERIFIER.verify(self.root)
        self.assertEqual(self._file_state(), before)

    def test_modified_jar_is_rejected(self):
        jar = self.root / "src-tauri/gen/android/gradle/wrapper/gradle-wrapper.jar"
        jar.write_bytes(jar.read_bytes() + b"modified")
        self._assert_rejected_without_changes("checksum mismatch")

    def test_removed_distribution_pin_is_rejected(self):
        props = (
            self.root / "src-tauri/gen/android/gradle/wrapper/gradle-wrapper.properties"
        )
        text = props.read_text()
        props.write_text(
            "\n".join(
                line
                for line in text.splitlines()
                if not line.startswith("distributionSha256Sum=")
            )
        )
        self._assert_rejected_without_changes("checksum mismatch")

    def test_linked_jar_is_rejected(self):
        jar = self.root / "src-tauri/gen/android/gradle/wrapper/gradle-wrapper.jar"
        target = self.root / "copied-wrapper.jar"
        jar.rename(target)
        jar.symlink_to(target)
        self._assert_rejected_without_changes("regular file")

    def test_missing_bootstrap_is_rejected(self):
        props = (
            self.root / "src-tauri/gen/android/gradle/wrapper/gradle-wrapper.properties"
        )
        props.unlink()
        self._assert_rejected_without_changes("regular file")


if __name__ == "__main__":
    unittest.main()

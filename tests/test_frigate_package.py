"""Publisher staging checks without building or executing a worker."""

import importlib.util
import json
import os
import struct
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/plugins/prepare-frigate-package.py"
SPEC = importlib.util.spec_from_file_location("prepare_frigate_package", SCRIPT)
packaging = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(packaging)


def executable_fixture(target):
    """Construct minimal executable headers; fixtures are never executed."""
    arm = target.startswith("aarch64-")
    if "apple" in target:
        header = struct.pack("<IIIIIIII", 0xFEEDFACF, 0x100000C if arm else 0x1000007,
                             0, 2, 1, 24, 0, 0)
        return header + struct.pack("<IIIIII", 0x32, 24, 1, 0, 0, 0)
    if "windows" in target:
        data = bytearray(64 + 24 + 112 + 40)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 64)
        data[64:68] = b"PE\x00\x00"
        struct.pack_into("<HH", data, 68, 0xAA64 if arm else 0x8664, 1)
        struct.pack_into("<HHH", data, 84, 112, 2, 0x20B)
        struct.pack_into("<I", data, 104, 4096)
        struct.pack_into("<H", data, 156, 3)
        return bytes(data)
    data = bytearray(120)
    data[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HHIQQ", data, 16, 3, 183 if arm else 62, 1, 4096, 64)
    struct.pack_into("<HHH", data, 52, 64, 56, 1)
    struct.pack_into("<I", data, 64, 1)
    return bytes(data)


class FrigatePackageTests(unittest.TestCase):
    """Only a deliberately selected desktop executable becomes package payload."""

    def setUp(self):
        # TestCase owns cleanup even if an assertion fails.
        # pylint: disable-next=consider-using-with
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.worker = self.root / "worker"
        self.worker.write_bytes(executable_fixture("aarch64-apple-darwin"))
        self.output = self.root / "package"

    def test_stages_only_worker_and_unsigned_matching_metadata(self):
        """Nearby files and private signing material never enter the payload."""
        (self.root / "unrelated.key").write_bytes(os.urandom(32))
        packaging.prepare(self.worker, "aarch64-apple-darwin", self.output)
        files = sorted(
            str(path.relative_to(self.output))
            for path in self.output.rglob("*") if path.is_file()
        )
        self.assertEqual(files, ["manifest.json", "payload/bin/inverter-frigate-worker"])
        metadata = json.loads((self.output / "manifest.json").read_text())
        self.assertEqual(metadata["target"], "aarch64-apple-darwin")
        self.assertEqual(metadata["plugin_id"], "inverter-desktop.frigate")
        self.assertEqual(metadata["version"], "0.1.0")
        self.assertEqual(metadata["host_api"], "^1.2")
        self.assertIsNone(metadata["signature"])
        self.assertEqual(metadata["inventory"], [])
        self.assertEqual(
            (self.output / "payload/bin/inverter-frigate-worker").read_bytes(),
            self.worker.read_bytes(),
        )
        fields = metadata["config_schema"]["properties"]
        self.assertTrue(fields["mqtt_username"]["writeOnly"])
        self.assertTrue(fields["mqtt_password"]["writeOnly"])
        self.assertEqual(fields["mqtt_topic"]["default"], "frigate/events")

    def test_windows_entrypoint_uses_executable_suffix(self):
        """The same metadata template supports the Windows binary name."""
        self.worker.write_bytes(executable_fixture("x86_64-pc-windows-msvc"))
        packaging.prepare(self.worker, "x86_64-pc-windows-msvc", self.output)
        metadata = json.loads((self.output / "manifest.json").read_text())
        self.assertEqual(metadata["entrypoint"], "bin/inverter-frigate-worker.exe")
        self.assertTrue((self.output / "payload/bin/inverter-frigate-worker.exe").is_file())

    def test_refuses_mobile_targets_and_existing_output(self):
        """Mobile is outside the package ecosystem and existing work is preserved."""
        for target in ("aarch64-linux-android", "aarch64-apple-ios", "unrecognized"):
            with self.subTest(target=target), self.assertRaisesRegex(ValueError, "desktop"):
                packaging.prepare(self.worker, target, self.output)
        self.output.mkdir()
        keep = self.output / "keep"
        keep.write_text("unchanged")
        with self.assertRaises(FileExistsError):
            packaging.prepare(self.worker, "aarch64-apple-darwin", self.output)
        self.assertEqual(keep.read_text(), "unchanged")

    def test_refuses_empty_directory_and_oversized_worker(self):
        """Input is bounded before allocating or creating staging files."""
        with self.assertRaisesRegex(ValueError, "regular"):
            packaging.prepare(self.root, "aarch64-apple-darwin", self.output)
        self.worker.write_bytes(b"")
        with self.assertRaisesRegex(ValueError, "size"):
            packaging.prepare(self.worker, "aarch64-apple-darwin", self.output)
        with self.worker.open("wb") as stream:
            stream.truncate(packaging.MAX_WORKER_BYTES + 1)
        with self.assertRaisesRegex(ValueError, "size"):
            packaging.prepare(self.worker, "aarch64-apple-darwin", self.output)
        self.assertFalse(self.output.exists())

    def test_every_supported_target_requires_matching_executable_headers(self):
        """OS and architecture mismatches cannot be hidden in manifest metadata."""
        for target in packaging.TARGETS:
            data = executable_fixture(target)
            packaging.verify_worker_target(data, target)
            for other in packaging.TARGETS:
                if target == other:
                    continue
                with (self.subTest(source=target, target=other),
                      self.assertRaisesRegex(ValueError, "match")):
                    packaging.verify_worker_target(data, other)

    def test_rejects_mobile_macho_dll_empty_and_truncated_headers(self):
        """Headers must identify a desktop executable, not merely a CPU family."""
        ios = bytearray(executable_fixture("aarch64-apple-darwin"))
        struct.pack_into("<I", ios, 40, 2)
        dll = bytearray(executable_fixture("x86_64-pc-windows-msvc"))
        struct.pack_into("<H", dll, 86, 0x2002)
        for target, data in (
            ("aarch64-apple-darwin", ios),
            ("x86_64-pc-windows-msvc", dll),
            ("aarch64-apple-darwin", b"not executable"),
            ("x86_64-unknown-linux-gnu", b"\x7fELF\x02\x01\x01"),
            ("aarch64-apple-darwin", b"\xcf\xfa\xed\xfe"),
        ):
            with self.subTest(target=target, data=data), self.assertRaises(ValueError):
                packaging.verify_worker_target(data, target)

    def test_architecture_failure_removes_partial_staging(self):
        """Mislabelled payloads leave no output directory to accidentally sign."""
        with self.assertRaisesRegex(ValueError, "match"):
            packaging.prepare(self.worker, "x86_64-unknown-linux-gnu", self.output)
        self.assertFalse(self.output.exists())

    @unittest.skipIf(os.name == "nt", "Creating symlinks requires Windows developer privileges")
    def test_refuses_links_in_file_and_parent_components(self):
        """Staging cannot silently follow a selected symlink or linked directory."""
        link = self.root / "link"
        link.symlink_to(self.worker)
        with self.assertRaisesRegex(ValueError, "symlinks"):
            packaging.prepare(link, "aarch64-apple-darwin", self.output)
        parent = self.root / "parent"
        parent.symlink_to(self.root, target_is_directory=True)
        source = parent / self.worker.name
        with self.assertRaisesRegex(ValueError, "symlinks"):
            packaging.prepare(source, "aarch64-apple-darwin", self.output)

    def test_cargo_hardlinked_input_is_copied_to_an_independent_file(self):
        """Cargo may hard-link its output; staging never preserves that linkage."""
        link = self.root / "hardlink"
        os.link(self.worker, link)
        packaging.prepare(link, "aarch64-apple-darwin", self.output)
        staged = self.output / "payload/bin/inverter-frigate-worker"
        self.assertEqual(staged.stat().st_nlink, 1)
        self.assertEqual(staged.read_bytes(), self.worker.read_bytes())
        self.assertFalse(os.path.samefile(staged, self.worker))

    def test_refuses_parent_traversal_before_normalizing_relative_paths(self):
        """Checking raw components prevents abspath from hiding traversal."""
        with self.assertRaisesRegex(ValueError, "traversal"):
            packaging.checked_path("unused/../worker")


if __name__ == "__main__":
    unittest.main()

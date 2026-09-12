"""Verify the ELF gate rejects incompatible or missing native payloads."""

import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location(
    "alignment", Path(__file__).resolve().parents[1] / "scripts/android/check_native_alignment.py"
)
ALIGNMENT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ALIGNMENT)


class AlignmentTests(unittest.TestCase):
    """Use actual ELF program header bytes rather than source text assertions."""

    def test_elf_alignment_and_missing_payload(self):
        """Accept 16 KB, reject 4 KB and incongruent segments, and fail empty input."""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.aab"
            for alignment, address, accepted in (
                (16384, 0, True),
                (4096, 0, False),
                (16384, 4096, False),
            ):
                with self.subTest(alignment=alignment, address=address):
                    data = bytearray(176)
                    data[:6] = b"\x7fELF\x02\x01"
                    struct.pack_into("<Q", data, 32, 64)
                    struct.pack_into("<HH", data, 54, 56, 2)
                    struct.pack_into("<I", data, 64, 1)
                    struct.pack_into("<Q", data, 80, address)
                    struct.pack_into("<Q", data, 112, alignment)
                    struct.pack_into("<I", data, 120, 0x6474E552)
                    struct.pack_into("<Q", data, 160, 16384)
                    with zipfile.ZipFile(path, "w") as bundle:
                        bundle.writestr("base/lib/arm64-v8a/libapp.so", data)
                    if accepted:
                        self.assertEqual(len(ALIGNMENT.check_bundle(path)), 1)
                    else:
                        with self.assertRaises(ValueError):
                            ALIGNMENT.check_bundle(path)
            with zipfile.ZipFile(path, "w"):
                pass
            with self.assertRaises(ValueError):
                ALIGNMENT.check_bundle(path)

    def test_relro_end_and_missing_protection(self):
        """Reject 4/8 KB RELRO ends even when all LOAD headers are 16 KB aligned."""
        for memory_size, present in ((16384, True), (4096, True), (8192, True), (16384, False)):
            with self.subTest(memory_size=memory_size, present=present):
                data = bytearray(176)
                data[:6] = b"\x7fELF\x02\x01"
                struct.pack_into("<Q", data, 32, 64)
                struct.pack_into("<HH", data, 54, 56, 2)
                struct.pack_into("<I", data, 64, 1)
                struct.pack_into("<Q", data, 112, 16384)
                struct.pack_into("<I", data, 120, 0x6474E552 if present else 0)
                struct.pack_into("<Q", data, 160, memory_size)
                if memory_size == 16384 and present:
                    ALIGNMENT.check_elf(data, "fixture.so")
                else:
                    with self.assertRaises(ValueError):
                        ALIGNMENT.check_elf(data, "fixture.so")


if __name__ == "__main__":
    unittest.main()

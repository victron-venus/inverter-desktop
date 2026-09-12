#!/usr/bin/env python3
"""Reject 64-bit Android ELF load segments incompatible with 16 KB pages."""

import argparse
import struct
import zipfile


def check_bundle(path):
    """Inspect actual packaged ELF headers, without executing native code."""
    checked = []
    with zipfile.ZipFile(path) as bundle:
        for name in bundle.namelist():
            if not name.endswith(".so") or not any(
                f"/lib/{abi}/" in name for abi in ("arm64-v8a", "x86_64")
            ):
                continue
            data = bundle.read(name)
            if data[:6] != b"\x7fELF\x02\x01":
                raise ValueError(f"Expected a 64-bit little-endian ELF: {name}")
            offset = struct.unpack_from("<Q", data, 32)[0]
            size, count = struct.unpack_from("<HH", data, 54)
            if size < 56:
                raise ValueError(f"Invalid ELF program header size: {name}")
            loads = 0
            for index in range(count):
                header = offset + size * index
                if struct.unpack_from("<I", data, header)[0] != 1:
                    continue
                file_offset, address = struct.unpack_from("<QQ", data, header + 8)
                alignment = struct.unpack_from("<Q", data, header + 48)[0]
                if (
                    alignment < 16384
                    or alignment & (alignment - 1)
                    or (address - file_offset) % 16384
                ):
                    raise ValueError(f"ELF load segment is not 16 KB compatible: {name}")
                loads += 1
            if not loads:
                raise ValueError(f"Missing ELF load segments: {name}")
            checked.append(name)
    if not checked:
        raise ValueError("No 64-bit Android native libraries found")
    return checked


def main():
    """Fail the build if any shipped 64-bit library has incompatible alignment."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle")
    args = parser.parse_args()
    try:
        for name in check_bundle(args.bundle):
            print(f"16 KB ELF alignment verified: {name}")
    except (ValueError, OSError, struct.error, zipfile.BadZipFile) as error:
        parser.exit(1, f"Native alignment check failed: {error}\n")


if __name__ == "__main__":
    main()

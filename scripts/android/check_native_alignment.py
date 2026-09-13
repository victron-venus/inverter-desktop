#!/usr/bin/env python3
"""Reject 64-bit Android ELF load segments incompatible with 16 KB pages."""

import argparse
import struct
import zipfile


def check_load_segment(data, header, name):
    """Validate the load alignment and file/virtual address congruence."""
    file_offset, address = struct.unpack_from("<QQ", data, header + 8)
    alignment = struct.unpack_from("<Q", data, header + 48)[0]
    if alignment < 16384 or alignment & (alignment - 1) or (address - file_offset) % 16384:
        raise ValueError(f"ELF load segment is not 16 KB compatible: {name}")


def check_elf(data, name):
    """Check load segments and RELRO end boundaries in a packaged 64-bit ELF."""
    if data[:6] != b"\x7fELF\x02\x01":
        raise ValueError(f"Expected a 64-bit little-endian ELF: {name}")
    offset = struct.unpack_from("<Q", data, 32)[0]
    size, count = struct.unpack_from("<HH", data, 54)
    if size < 56:
        raise ValueError(f"Invalid ELF program header size: {name}")
    loads = relro = 0
    for index in range(count):
        header = offset + size * index
        kind = struct.unpack_from("<I", data, header)[0]
        if kind == 1:
            check_load_segment(data, header, name)
            loads += 1
        elif kind == 0x6474E552:
            address = struct.unpack_from("<Q", data, header + 16)[0]
            memory_size = struct.unpack_from("<Q", data, header + 40)[0]
            if (address + memory_size) % 16384:
                raise ValueError(f"ELF RELRO end is not 16 KB aligned: {name}")
            relro += 1
    if not loads or not relro:
        raise ValueError(f"Missing ELF load segments or RELRO protection: {name}")


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
            check_elf(data, name)
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

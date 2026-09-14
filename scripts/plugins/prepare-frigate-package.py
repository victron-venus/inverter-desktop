#!/usr/bin/env python3
"""Stage one prebuilt desktop Frigate worker for the native plugin-package signer.

This helper copies a binary and writes unsigned metadata. It does not build or
execute the worker, read signing keys, install packages, or establish trust.
The binary must be a matching ELF64, PE32+, or thin 64-bit macOS executable;
headers verify format and architecture, not complete runtime ABI compatibility.
Use the existing native plugin-package example to sign the resulting manifest
and payload with an externally supplied publisher key.
"""

# CLI filenames use hyphens to match their shell entry points.
# pylint: disable=invalid-name

import argparse
import json
import os
import shutil
import stat
import struct
from pathlib import Path

TEMPLATE = Path(__file__).with_name("frigate-manifest.json")
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
)
MAX_WORKER_BYTES = 60 * 1024 * 1024


def verify_worker_target(data, target):
    """Check executable format/CPU before attaching desktop target metadata.

    Headers establish architecture and, for Mach-O, the deployment platform.
    They cannot prove compiler provenance or every dynamic-library ABI choice.
    Universal Mach-O files are deliberately unsupported: package one target.
    """
    if target not in TARGETS:
        raise ValueError("Only supported desktop targets can be packaged")
    arm = target.startswith("aarch64-")
    try:
        if "apple" in target:
            matches = macho_target(data, arm)
        elif "windows" in target:
            matches = pe_target(data, arm)
        else:
            matches = elf_target(data, arm)
    except struct.error as error:
        raise ValueError("Worker has a truncated executable header") from error
    if not matches:
        raise ValueError("Worker executable format or architecture does not match --target")


def macho_target(data, arm):
    """Require a thin 64-bit executable explicitly targeting macOS, never iOS."""
    if data[:4] != b"\xcf\xfa\xed\xfe":
        return False
    cpu, _, kind, count, command_bytes = struct.unpack_from("<IIIII", data, 4)
    end = 32 + command_bytes
    if (cpu != (0x100000C if arm else 0x1000007) or kind != 2
            or count > 4096 or end > len(data)):
        return False
    cursor = 32
    platforms = []
    for _ in range(count):
        command, size = struct.unpack_from("<II", data, cursor)
        if size < 8 or size % 8 or cursor + size > end:
            return False
        if command in (0x32, 0x24):  # LC_BUILD_VERSION or LC_VERSION_MIN_MACOSX
            if size < (24 if command == 0x32 else 16):
                return False
            platforms.append(struct.unpack_from("<I", data, cursor + 8)[0]
                             if command == 0x32 else 1)
        elif command in (0x25, 0x2F, 0x30):  # iOS, tvOS, watchOS minimum versions
            return False
        cursor += size
    return cursor == end and platforms == [1]


def elf_target(data, arm):
    """Require a little-endian ELF64 executable with the selected Linux CPU."""
    if data[:7] != b"\x7fELF\x02\x01\x01" or len(data) < 64:
        return False
    kind, machine, version = struct.unpack_from("<HHI", data, 16)
    entry, offset = struct.unpack_from("<QQ", data, 24)
    header_size, record_size, count = struct.unpack_from("<HHH", data, 52)
    if (kind not in (2, 3) or machine != (183 if arm else 62) or version != 1
            or data[7] not in (0, 3) or not entry):
        return False
    if (header_size != 64 or record_size != 56 or not 0 < count <= 4096
            or offset < 64 or offset + record_size * count > len(data)):
        return False
    return any(struct.unpack_from("<I", data, offset + index * record_size)[0] == 1
               for index in range(count))


def pe_target(data, arm):
    """Require a Windows PE32+ executable, rejecting DLLs and other CPU types."""
    if data[:2] != b"MZ" or len(data) < 64:
        return False
    offset = struct.unpack_from("<I", data, 60)[0]
    if offset < 64 or data[offset:offset + 4] != b"PE\x00\x00":
        return False
    machine, sections = struct.unpack_from("<HH", data, offset + 4)
    optional_size, flags = struct.unpack_from("<HH", data, offset + 20)
    optional = offset + 24
    if machine != (0xAA64 if arm else 0x8664) or not flags & 2 or flags & 0x2000:
        return False
    if (optional_size < 112 or not sections
            or optional + optional_size + sections * 40 > len(data)):
        return False
    magic = struct.unpack_from("<H", data, optional)[0]
    entry = struct.unpack_from("<I", data, optional + 16)[0]
    subsystem = struct.unpack_from("<H", data, optional + 68)[0]
    return magic == 0x20B and entry != 0 and subsystem in (2, 3)


def checked_path(path):
    """Reject symlinks, reparse points and traversal before resolving any input."""
    path = Path(path)
    if ".." in path.parts:
        raise ValueError("Paths must not contain parent traversal")
    if not path.is_absolute():
        path = Path.cwd() / path
    for component in (*reversed(path.parents), path):
        metadata = component.lstat()
        if stat.S_ISLNK(metadata.st_mode) or getattr(metadata, "st_file_attributes", 0) & 0x400:
            raise ValueError("Paths must not contain symlinks or reparse points")
    return path


def prepare(worker, target, output):
    """Create a new staging directory containing only the supplied regular file."""
    if target not in TARGETS:
        raise ValueError("Only supported desktop targets can be packaged")
    worker = checked_path(worker)
    source = worker.stat()
    if not stat.S_ISREG(source.st_mode):
        raise ValueError("Worker must be a regular file")
    if not 0 < source.st_size <= MAX_WORKER_BYTES:
        raise ValueError("Worker size must be between 1 byte and 60 MiB")
    output = Path(output)
    parent = checked_path(output.parent)
    if output.name in ("", ".", ".."):
        raise ValueError("Choose a new staging directory")
    output = parent / output.name
    output.mkdir(mode=0o700)
    try:
        payload = output / "payload" / "bin"
        payload.mkdir(parents=True)
        name = "inverter-frigate-worker" + (".exe" if "windows" in target else "")
        destination = payload / name
        # Open without following a replaced final symlink; metadata also rejects
        # nonregular files and replacement between inspection and opening.
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
        with os.fdopen(os.open(worker, flags), "rb") as stream:
            opened = os.fstat(stream.fileno())
            if (opened.st_dev, opened.st_ino, opened.st_size,
                opened.st_mtime_ns, opened.st_ctime_ns) != (
                source.st_dev, source.st_ino, source.st_size,
                source.st_mtime_ns, source.st_ctime_ns
            ) or not stat.S_ISREG(opened.st_mode):
                raise ValueError("Worker changed while preparing the package")
            data = stream.read(MAX_WORKER_BYTES + 1)
            after = os.fstat(stream.fileno())
        if (len(data) != source.st_size or after.st_mtime_ns != opened.st_mtime_ns
                or after.st_ctime_ns != opened.st_ctime_ns):
            raise ValueError("Worker changed while preparing the package")
        verify_worker_target(data, target)
        destination.write_bytes(data)
        destination.chmod(0o755)
        manifest = json.loads(TEMPLATE.read_text(encoding="utf-8"))
        manifest["target"] = target
        manifest["entrypoint"] = f"bin/{name}"
        (output / "manifest.json").write_text(
            json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
    except (OSError, ValueError):
        shutil.rmtree(output)
        raise
    return output


def main():
    """Require an explicit prebuilt worker and its desktop compilation target."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, required=True, help="Prebuilt regular worker file")
    parser.add_argument(
        "--target", choices=TARGETS, required=True, help="Binary compilation target"
    )
    parser.add_argument("--output", type=Path, required=True, help="New staging directory")
    args = parser.parse_args()
    try:
        prepare(args.worker, args.target, args.output)
    except (OSError, ValueError) as error:
        parser.exit(1, f"Cannot prepare Frigate package: {error}\n")
    print(
        "Staged manifest.json and payload/. "
        "Use the native plugin-package signer; no worker was run."
    )


if __name__ == "__main__":
    main()

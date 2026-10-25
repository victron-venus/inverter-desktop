"""Verify the macOS bundle seal and the exact app carried by each installer."""

import hashlib
import os
import platform
import plistlib
import stat
import subprocess
import tempfile
import zipfile
from pathlib import Path

BUNDLE_ID = "com.alvit.inverter-dashboard"


def verify_bundle(app):
    """Require a sealed app; ad-hoc signing does not imply notarization."""
    if platform.system() != "Darwin":
        raise ValueError("macOS bundle verification requires native macOS tools")
    app = Path(app)
    if app.is_symlink() or not app.is_dir():
        raise ValueError("Expected a real macOS application directory")
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    if info.get("CFBundleIdentifier") != BUNDLE_ID:
        raise ValueError("Unexpected macOS bundle identifier")
    subprocess.run(
        ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)],
        check=True,
        timeout=60,
    )


def app_files(app):
    """Describe payload bytes, links and permissions without following links."""
    files = {}
    for path in sorted(Path(app).rglob("*")):
        mode = path.lstat().st_mode
        if stat.S_ISDIR(mode):
            continue
        if stat.S_ISLNK(mode):
            data = os.readlink(path).encode()
        elif stat.S_ISREG(mode):
            data = path.read_bytes()
        else:
            raise ValueError("Unsupported macOS bundle file type")
        files[path.relative_to(app).as_posix()] = (
            stat.S_IFMT(mode),
            stat.S_IMODE(mode),
            len(data),
            hashlib.sha256(data).hexdigest(),
        )
    return files


def verify_zip(archive, app):
    """Bind ZIP contents to the sealed app without extracting archive paths."""
    verify_bundle(app)
    expected = app_files(app)
    actual = {}
    with zipfile.ZipFile(archive) as package:
        for member in package.infolist():
            if member.is_dir() or member.filename.startswith("__MACOSX/"):
                continue
            prefix = Path(app).name + "/"
            if not member.filename.startswith(prefix):
                raise ValueError("Unexpected file outside packaged macOS app")
            name = member.filename.removeprefix(prefix)
            if name not in expected or name in actual:
                raise ValueError("Unexpected or duplicate macOS ZIP member")
            if member.file_size != expected[name][2]:
                raise ValueError("Packaged macOS ZIP size differs from sealed app")
            mode = member.external_attr >> 16
            actual[name] = (
                stat.S_IFMT(mode),
                stat.S_IMODE(mode),
                member.file_size,
                hashlib.sha256(package.read(member)).hexdigest(),
            )
    if actual != expected:
        raise ValueError("Packaged macOS ZIP differs from sealed app")


def verify_dmg(image, app):
    """Mount read-only, verify the inner seal and compare it with the built app."""
    verify_bundle(app)
    with tempfile.TemporaryDirectory(prefix="inverter-dmg-check-") as temp:
        mount = Path(temp) / "volume"
        mount.mkdir()
        subprocess.run(
            [
                "/usr/bin/hdiutil",
                "attach",
                "-readonly",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
                str(mount),
                str(image),
            ],
            check=True,
            timeout=60,
            stdout=subprocess.DEVNULL,
        )
        try:
            candidates = list(mount.glob("*.app"))
            if len(candidates) != 1 or candidates[0].name != Path(app).name:
                raise ValueError("Expected one macOS application in disk image")
            verify_bundle(candidates[0])
            if app_files(candidates[0]) != app_files(app):
                raise ValueError("Packaged macOS DMG differs from sealed app")
        finally:
            subprocess.run(
                ["/usr/bin/hdiutil", "detach", str(mount)],
                check=True,
                timeout=60,
                stdout=subprocess.DEVNULL,
            )

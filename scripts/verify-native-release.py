#!/usr/bin/env python3
"""Read versions from native desktop and mobile packages before uploading them."""

# CLI filenames use hyphens to match their shell entry points.
# pylint: disable=invalid-name

import argparse
import hashlib
import io
import json
import os
import platform
from pathlib import Path
import plistlib
import re
import subprocess
import tarfile
import tempfile
import xml.etree.ElementTree as ET
import zipfile

from version_plan import projected_value, validate_plan
from version_receipt import confined_cli_path

CHECKOUT = Path(__file__).resolve().parents[1]
BUNDLETOOL_SHA256 = "a099cfa1543f55593bc2ed16a70a7c67fe54b1747bb7301f37fdfd6d91028e29"


def expected_inspector(root_variable, relative, requested):
    """Select the fixed installed inspector, never a caller-chosen executable."""
    configured = os.environ.get(root_variable)
    if not configured:
        raise ValueError(f"Inspector requires {root_variable}")
    root = Path(configured).resolve(strict=True)
    expected = confined_cli_path(root, root / relative, "file")
    selected = confined_cli_path(root, requested, "file")
    if selected != expected:
        raise ValueError(f"Inspector must be {root_variable}/{relative}")
    return expected


def verify_plist(info, plan):
    """Require the package plist's numeric versions to match the frozen plan."""
    expected = {
        "CFBundleShortVersionString": plan["base_version"],
        "CFBundleVersion": projected_value(plan, {"value": "apple-build"}),
    }
    for key, value in expected.items():
        if info.get(key) != value:
            raise ValueError(f"Packaged {key}={info.get(key)!r}, expected {value}")


def verify_embedded_identity(binary, plan):
    """Check the compiled descriptor's full version and source commit markers."""
    # The validated descriptor is compiled into the Rust IPC handler. Inspecting
    # it complements native plist/APK metadata, whose version is platform-specific.
    for key in ("version", "source_sha"):
        marker = json.dumps({key: plan[key]}, separators=(",", ":"))[1:-1].encode()
        if marker not in binary:
            raise ValueError(f"Packaged binary does not contain the planned {key}")


def verify_apple(path, plan):
    """Inspect native metadata and the Rust binary in an Apple app or archive."""
    if path.is_dir():
        info = plistlib.loads((path / "Contents/Info.plist").read_bytes())
        binary = (path / "Contents/MacOS" / info["CFBundleExecutable"]).read_bytes()
    else:
        with zipfile.ZipFile(path) as archive:
            manifests = [
                name
                for name in archive.namelist()
                if (
                    name.endswith(".app/Contents/Info.plist")
                    or name.startswith("Payload/")
                    and name.count("/") == 2
                    and name.endswith(".app/Info.plist")
                )
                and not name.startswith("__MACOSX/")
            ]
            if len(manifests) != 1:
                raise ValueError("Expected one application plist")
            name = manifests[0]
            info = plistlib.loads(archive.read(name))
            prefix = name.removesuffix("Info.plist")
            executable = ("MacOS/" if ".app/Contents/" in name else "") + info[
                "CFBundleExecutable"
            ]
            binary = archive.read(prefix + executable)
    verify_plist(info, plan)
    verify_embedded_identity(binary, plan)


def verify_apk(path, plan, aapt):
    """Check Android package metadata and every bundled application library."""
    path = path.resolve(strict=True)
    aapt = expected_inspector("ANDROID_HOME", "build-tools/34.0.0/aapt", aapt)
    output = subprocess.check_output(
        [str(aapt), "dump", "badging", str(path)], text=True
    )
    match = re.search(
        r"^package: name='([^']+)' versionCode='([^']+)' versionName='([^']+)'",
        output,
        re.MULTILINE,
    )
    if not match or match.groups() != (
        "com.alvit.inverter_dashboard",
        str(plan["build_number"]),
        plan["version"],
    ):
        raise ValueError(
            "APK package, versionCode or versionName differs from release plan"
        )
    with zipfile.ZipFile(path) as archive:
        libraries = [
            name
            for name in archive.namelist()
            if name.startswith("lib/")
            and name.endswith("/libinverter_dashboard_lib.so")
        ]
        if not libraries:
            raise ValueError("APK contains no application Rust library")
        for name in libraries:
            verify_embedded_identity(archive.read(name), plan)


def verify_aab(path, plan, bundletool):
    """Inspect the base bundle manifest and its compiled application libraries."""
    path = path.resolve(strict=True)
    bundletool = expected_inspector("RUNNER_TEMP", "bundletool.jar", bundletool)
    if hashlib.sha256(bundletool.read_bytes()).hexdigest() != BUNDLETOOL_SHA256:
        raise ValueError(
            "Bundle metadata inspector checksum differs from pinned bundletool"
        )
    output = subprocess.check_output(
        [
            "java",
            "-jar",
            str(bundletool),
            "dump",
            "manifest",
            f"--bundle={path}",
            "--module=base",
        ],
        text=True,
    )
    info = ET.fromstring(output).attrib
    namespace = "{http://schemas.android.com/apk/res/android}"
    expected = {
        "package": "com.alvit.inverter_dashboard",
        namespace + "versionName": plan["version"],
        namespace + "versionCode": str(plan["build_number"]),
    }
    if any(info.get(key) != value for key, value in expected.items()):
        raise ValueError(
            "AAB package, versionCode or versionName differs from release plan"
        )
    with zipfile.ZipFile(path) as archive:
        libraries = [
            name
            for name in archive.namelist()
            if name.startswith("base/lib/")
            and name.endswith("/libinverter_dashboard_lib.so")
        ]
        if not libraries:
            raise ValueError("AAB contains no application Rust library")
        for name in libraries:
            verify_embedded_identity(archive.read(name), plan)


def verify_linux(path, plan):
    """Check the native Linux package version and embedded application identity."""
    path = path.resolve(strict=True)
    if path.suffix == ".deb":
        version = subprocess.check_output(
            ["dpkg-deb", "-f", str(path), "Version"], text=True
        ).strip()
        architecture = subprocess.check_output(
            ["dpkg-deb", "-f", str(path), "Architecture"], text=True
        ).strip()
        expected_architecture = {
            "x86_64": "amd64",
            "aarch64": "arm64",
            "arm64": "arm64",
        }.get(platform.machine())
        if architecture != expected_architecture:
            raise ValueError(
                f"Debian architecture {architecture}; expected {expected_architecture}"
            )
        payload = subprocess.check_output(["dpkg-deb", "--fsys-tarfile", str(path)])
        with tarfile.open(fileobj=io.BytesIO(payload)) as archive:
            members = [
                member
                for member in archive.getmembers()
                if member.name.lstrip("./") == "usr/bin/inverter-dashboard"
                and member.isfile()
            ]
            if len(members) != 1:
                raise ValueError("Debian package must contain one application binary")
            verify_embedded_identity(archive.extractfile(members[0]).read(), plan)
    elif path.suffix == ".rpm":
        version = subprocess.check_output(
            ["rpm", "-qp", "--queryformat", "%{VERSION}", str(path)], text=True
        ).strip()
        names = subprocess.check_output(
            ["bsdtar", "-tf", str(path)], text=True
        ).splitlines()
        members = [
            name for name in names if name.lstrip("./") == "usr/bin/inverter-dashboard"
        ]
        if len(members) != 1:
            raise ValueError("RPM package must contain one application binary")
        binary = subprocess.check_output(["bsdtar", "-xOf", str(path), members[0]])
        verify_embedded_identity(binary, plan)
    elif path.suffix == ".AppImage":
        # Extraction mode runs only the AppImage runtime, not the application.
        with tempfile.TemporaryDirectory() as directory:
            subprocess.run(
                [str(path.resolve()), "--appimage-extract"],
                cwd=directory,
                check=True,
                stdout=subprocess.DEVNULL,
            )
            binary = Path(directory) / "squashfs-root/usr/bin/inverter-dashboard"
            verify_embedded_identity(binary.read_bytes(), plan)
        return
    else:
        raise ValueError(f"Unsupported Linux package: {path}")
    if version != plan["base_version"]:
        raise ValueError(
            f"Linux package version {version}; expected {plan['base_version']}"
        )


def verify_windows(path, plan):
    """Read the actual installer version from Windows package metadata."""
    environment = {**os.environ, "INVERTER_PACKAGE_PATH": str(path.resolve())}
    if path.suffix == ".msi":
        command = """
$ErrorActionPreference = 'Stop'
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase($env:INVERTER_PACKAGE_PATH, 0)
$query = 'SELECT `Value` FROM `Property` WHERE `Property` = ''ProductVersion'''
$view = $database.OpenView($query)
$view.Execute()
$record = $view.Fetch()
if ($null -eq $record) { throw 'MSI ProductVersion is missing' }
$record.StringData(1)
"""
    elif path.suffix == ".exe":
        command = "(Get-Item -LiteralPath $env:INVERTER_PACKAGE_PATH).VersionInfo.ProductVersion"
    else:
        raise ValueError(f"Unsupported Windows package: {path}")
    version = subprocess.check_output(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", command],
        env=environment,
        text=True,
    ).strip()
    if version not in {plan["base_version"], plan["base_version"] + ".0"}:
        raise ValueError(
            f"Windows package version {version}; expected {plan['base_version']}"
        )


def main():
    """Select a package inspector and load its frozen expected release identity."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=("apple", "android", "aab", "linux", "windows"))
    parser.add_argument("path", type=Path)
    parser.add_argument("--plan", type=Path, default=Path(".release-plan.json"))
    parser.add_argument("--aapt", type=Path)
    parser.add_argument("--bundletool", type=Path)
    args = parser.parse_args()
    plan_path = confined_cli_path(CHECKOUT, args.plan, "metadata")
    artifact_kind = (
        "directory" if args.kind == "apple" and args.path.suffix == ".app" else "file"
    )
    artifact = confined_cli_path(CHECKOUT, args.path, artifact_kind)
    plan = validate_plan(json.loads(plan_path.read_bytes()))
    if args.kind == "apple":
        verify_apple(artifact, plan)
    elif args.kind == "android":
        if not args.aapt:
            parser.error("Android verification requires --aapt")
        verify_apk(artifact, plan, args.aapt)
    elif args.kind == "aab":
        if not args.bundletool:
            parser.error("AAB verification requires --bundletool")
        verify_aab(artifact, plan, args.bundletool)
    elif args.kind == "linux":
        verify_linux(artifact, plan)
    else:
        verify_windows(artifact, plan)
    print(
        f"Verified packaged {plan['version']} build {plan['build_number']}: {artifact}"
    )


if __name__ == "__main__":
    main()

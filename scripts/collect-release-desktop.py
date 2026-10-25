#!/usr/bin/env python3
# CLI filenames use hyphens to match their shell entry points.
# pylint: disable=invalid-name
"""Collect locally built Tauri installers without creating a GitHub release."""

import os
import shutil
import subprocess
import sys
from pathlib import Path

from macos_release import verify_bundle, verify_dmg, verify_zip

root = Path.cwd()
out = root / "release-output" / "desktop"
out.mkdir(parents=True, exist_ok=True)
target = os.environ.get("TAURI_TARGET", "")
platform = os.environ.get(
    "RUNNER_OS", os.uname().sysname if hasattr(os, "uname") else "Windows"
)
bases = [root / "src-tauri" / "target", root / "target"]
found = 0
for base in bases:
    bundle = (
        base / target / "release" / "bundle" if target else base / "release" / "bundle"
    )
    if not bundle.exists():
        continue
    for path in sorted(bundle.rglob("*")):
        if path.is_dir() and path.suffix == ".app":
            verify_bundle(path)
            if (root / ".release-plan.json").exists():
                subprocess.run(
                    [
                        sys.executable,
                        "scripts/verify-native-release.py",
                        "apple",
                        str(path),
                    ],
                    check=True,
                )
            dest = out / (
                path.stem.replace(" ", ".") + f"_{target or platform}.app.zip"
            )
            subprocess.run(
                [
                    "ditto",
                    "-c",
                    "-k",
                    "--sequesterRsrc",
                    "--keepParent",
                    str(path),
                    str(dest),
                ],
                check=True,
            )
            verify_zip(dest, path)
            found += 1
        elif (
            path.is_file()
            and not any(part.endswith(".app") for part in path.parts)
            and any(
                path.name.endswith(ext)
                for ext in (
                    ".dmg",
                    ".msi",
                    ".exe",
                    ".deb",
                    ".rpm",
                    ".AppImage",
                    ".sig",
                    ".app.tar.gz",
                )
            )
        ):
            if path.suffix == ".dmg":
                applications = list(bundle.glob("macos/*.app"))
                if len(applications) != 1:
                    raise ValueError("Expected one built macOS application")
                verify_dmg(path, applications[0])
            if (root / ".release-plan.json").exists():
                if path.suffix in {".deb", ".rpm", ".AppImage"}:
                    kind = "linux"
                elif path.suffix in {".exe", ".msi"}:
                    kind = "windows"
                else:
                    kind = None
                if kind:
                    subprocess.run(
                        [
                            sys.executable,
                            "scripts/verify-native-release.py",
                            kind,
                            str(path),
                        ],
                        check=True,
                    )
            dest = out / (f"{target or platform}_" + path.name.replace(" ", "."))
            shutil.copy2(path, dest)
            found += 1
if not found:
    raise SystemExit("No native installer was built for this platform")
print(f"Collected {found} installer files under {out}")

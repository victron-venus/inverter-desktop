"""Verify the reviewed Android Gradle bootstrap before it can execute."""

import argparse
import hashlib
from pathlib import Path

# Official Gradle 8.14.3 wrapper digest; properties also pin the distribution ZIP.
# Update these reviewed inputs together when upgrading Gradle.
CHECKSUMS = {
    "src-tauri/gen/android/gradle/wrapper/gradle-wrapper.jar": "7d3a4ac4de1c32b59bc6a4eb8ecb8e612ccd0cf1ae1e99f66902da64df296172",
    "src-tauri/gen/android/gradle/wrapper/gradle-wrapper.properties": "1e73d8d1e5d0f54ce581e724659d2a2c914a3d772b38d11aa14f61daea35d133",
}


def verify(root: Path) -> None:
    """Reject missing, linked or modified bootstrap files before running Java."""
    for relative, expected in CHECKSUMS.items():
        path = root / relative
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"Gradle bootstrap must be a regular file: {relative}")
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != expected:
            raise ValueError(f"Gradle bootstrap checksum mismatch: {relative}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[2]
    )
    args = parser.parse_args()
    verify(args.root)
    print("Verified Gradle wrapper and pinned distribution configuration.")


if __name__ == "__main__":
    main()

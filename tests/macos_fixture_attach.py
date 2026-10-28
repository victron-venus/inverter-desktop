"""Narrow, test-owned DMG attach recovery; never used by release scripts."""

import hashlib
import os
import plistlib
import re
import stat
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ATTACH_PREFIX = [
    "/usr/bin/hdiutil", "attach", "-readonly", "-nobrowse", "-noautoopen", "-mountpoint"
]
EAGAIN = b"hdiutil: attach failed - Resource temporarily unavailable"
MAX_REGISTRY = 1024 * 1024


class UnsafeRetry(Exception):
    """The next attach cannot be justified by an unambiguous empty state."""


class UniqueDictionary(dict):
    """Reject ambiguous duplicate keys in the registry plist."""

    def __setitem__(self, key, value):
        if key in self:
            raise UnsafeRetry("registry contains duplicate keys")
        super().__setitem__(key, value)


def canonical(value):
    if not isinstance(value, str) or not value or not os.path.isabs(value):
        raise UnsafeRetry("invalid registry or fixture path")
    return Path(os.path.realpath(value))


def file_identity(path):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode):
        raise UnsafeRetry("fixture image is not a regular file")
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def registration_summary(entities):
    """Describe an already validated owned record without paths or new probes."""
    devices = {entity["dev-entry"] for entity in entities}
    roots = {re.match(r"/dev/disk[0-9]+", device)[0] for device in devices}
    return (
        f"entities={len(entities)}, device_roots={len(roots)}, "
        f"whole_devices={len(roots & devices)}, "
        f"mount_entries={sum('mount-point' in entity for entity in entities)}"
    )


def mount_identity(mount):
    parent = mount.parent
    if (
        mount.name != "volume"
        or not re.fullmatch(r"inverter-dmg-check-[A-Za-z0-9_-]+", parent.name)
        or parent.parent != Path(tempfile.gettempdir()).resolve()
    ):
        raise UnsafeRetry("mount is outside the fixture namespace")
    identities = []
    for path in (parent, mount):
        info = path.lstat()
        if not stat.S_ISDIR(info.st_mode) or path.is_mount():
            raise UnsafeRetry("fixture mount is occupied or replaced")
        if hasattr(os, "getuid") and info.st_uid != os.getuid():
            raise UnsafeRetry("fixture mount owner changed")
        identities.append((info.st_dev, info.st_ino))
    if next(mount.iterdir(), None) is not None:
        raise UnsafeRetry("fixture mount is not empty")
    return tuple(identities)


class FixtureSubprocess:
    """Private subprocess proxy for one exact image created by one native test."""

    def __init__(self, root, image, *, runner=None, clock=None, sleep=None, log=None):
        self.image = Path(image)
        if self.image != Path(root).resolve() / "fixture.dmg":
            raise UnsafeRetry("image was not created in this fixture root")
        self.identity = file_identity(self.image)
        self.cleanup_digest = self.image_digest()
        if file_identity(self.image) != self.identity:
            raise UnsafeRetry("fixture image changed during initial hash")
        self.attempted_mounts = set()
        self.cleanup_detach_attempted = False
        self.cleanup_failure = None
        self.runner = runner or subprocess.run
        self.clock = clock or time.monotonic
        self.sleep = sleep or time.sleep
        self.log = log if log is not None else sys.stderr

    def __getattr__(self, name):
        return getattr(subprocess, name)

    def coordinates(self, arguments, options):
        if (not isinstance(arguments, (list, tuple)) or len(arguments) != 8
                or list(arguments[:6]) != ATTACH_PREFIX
                or options != {"check": True, "timeout": 60, "stdout": subprocess.DEVNULL}):
            return None
        try:
            if canonical(arguments[7]) != self.image or file_identity(self.image) != self.identity:
                return None
            mount = canonical(arguments[6])
            # Reject aliases at the mount itself; the /var -> /private alias is fine.
            if Path(arguments[6]).is_symlink() or Path(arguments[6]).parent.is_symlink():
                return None
            return mount, mount_identity(mount)
        except (OSError, UnsafeRetry, ValueError):
            return None

    def remaining(self, deadline):
        remaining = deadline - self.clock()
        if remaining <= 0:
            raise UnsafeRetry("shared 60-second attach deadline exhausted")
        return remaining

    def registry_images(self, deadline):
        with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
            self.runner(["/usr/bin/hdiutil", "info", "-plist"], check=True,
                        timeout=min(5, self.remaining(deadline)), stdout=output, stderr=errors)
            output.seek(0)
            data = output.read(MAX_REGISTRY + 1)
        if len(data) > MAX_REGISTRY:
            raise UnsafeRetry("registry exceeds observation bound")
        registry = plistlib.loads(data, dict_type=UniqueDictionary)
        if not isinstance(registry, dict) or not isinstance(registry.get("images"), list):
            raise UnsafeRetry("registry images are missing or malformed")
        if len(registry["images"]) > 256:
            raise UnsafeRetry("registry image count exceeds observation bound")
        seen_images, seen_devices = set(), set()
        images = []
        for image in registry["images"]:
            if not isinstance(image, dict):
                raise UnsafeRetry("registry image is malformed")
            path = canonical(image.get("image-path"))
            if path in seen_images:
                raise UnsafeRetry("registry has ambiguous image aliases")
            seen_images.add(path)
            entities = image.get("system-entities")
            if not isinstance(entities, list) or len(entities) > 128:
                raise UnsafeRetry("registry entities are missing or malformed")
            for entity in entities:
                if not isinstance(entity, dict):
                    raise UnsafeRetry("registry entity is malformed")
                device = entity.get("dev-entry")
                if (
                    not isinstance(device, str)
                    or not re.fullmatch(r"/dev/disk[0-9]+(?:s[0-9]+)*", device)
                ):
                    raise UnsafeRetry("registry device identity is missing or malformed")
                if device in seen_devices:
                    raise UnsafeRetry("registry device identity is ambiguous")
                seen_devices.add(device)
                if "mount-point" in entity:
                    canonical(entity["mount-point"])
            images.append((path, entities))
        return images

    def registry_clear(self, mount, deadline):
        for path, entities in self.registry_images(deadline):
            if path == self.image or (path.exists() and os.path.samefile(path, self.image)):
                raise UnsafeRetry(
                    "owned image remains registered (" + registration_summary(entities) + ")"
                )
            if any("mount-point" in entity and canonical(entity["mount-point"]) == mount
                   for entity in entities):
                raise UnsafeRetry("fixture mount remains registered")

    def image_digest(self):
        digest = hashlib.sha256()
        with self.image.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
        return digest.digest()

    def cleanup_image_unchanged(self):
        # hdiutil adds com.apple.diskimages.recentcksum, changing ctime only.
        # Cleanup binds the same regular inode, size, mtime and complete bytes.
        # The stricter retry identity above deliberately remains unchanged.
        before = file_identity(self.image)
        if before[:4] != self.identity[:4]:
            return False
        digest = self.image_digest()
        after = file_identity(self.image)
        return after[:4] == before[:4] and digest == self.cleanup_digest

    def cleanup_owned_image(self):
        """Detach one exactly owned fixture image, including a partial attachment."""
        if self.cleanup_failure is not None:
            raise self.cleanup_failure
        deadline = self.clock() + 60
        images = self.registry_images(deadline)
        owned = [(path, entities) for path, entities in images if path == self.image]
        if not owned:
            if any(path.exists() and os.path.samefile(path, self.image)
                   for path, _ in images):
                raise UnsafeRetry("fixture image has an unexpected registry alias")
            if any("mount-point" in entity
                   and canonical(entity["mount-point"]) in self.attempted_mounts
                   for _, entities in images for entity in entities):
                raise UnsafeRetry("attempted fixture mount remains registered")
            if any(mount.is_mount() for mount in self.attempted_mounts):
                raise UnsafeRetry("attempted fixture volume is still mounted")
            return
        if not self.cleanup_image_unchanged():
            raise UnsafeRetry("fixture image changed before cleanup")
        entities = owned[0][1]
        roots = {re.match(r"/dev/disk[0-9]+", entity["dev-entry"])[0]
                 for entity in entities}
        if not roots or not roots.issubset({entity["dev-entry"] for entity in entities}):
            raise UnsafeRetry("fixture registry lacks the whole-device identity")
        for path, other_entities in images:
            if path == self.image:
                continue
            if (path.exists() and os.path.samefile(path, self.image)) or any(
                re.match(r"/dev/disk[0-9]+", entity["dev-entry"])[0] in roots
                for entity in other_entities
            ):
                raise UnsafeRetry("fixture device is shared with another registry image")
        mounts = {canonical(entity["mount-point"]) for entity in entities
                  if "mount-point" in entity}
        if not mounts.issubset(self.attempted_mounts):
            raise UnsafeRetry("fixture device has an unexpected mount point")
        if any("mount-point" in entity and canonical(entity["mount-point"]) in mounts
               for path, other_entities in images if path != self.image
               for entity in other_entities):
            raise UnsafeRetry("fixture mount is shared with another registry image")
        if len(mounts) == 1 and next(iter(mounts)).is_mount():
            # APFS exposes both a backing disk and a synthesized container.
            # Use the single proven mount, never guess which root disk to detach.
            target = str(next(iter(mounts)))
        elif not mounts or (len(mounts) == 1 and not next(iter(mounts)).is_mount()):
            if len(roots) != 1:
                # This refusal proves no single actual owned mount. Keep its
                # observed topology in the error without another native query.
                raise UnsafeRetry(
                    "partial fixture cleanup requires one owned device ("
                    + registration_summary(entities)
                    + f", registered_mounts={len(mounts)}, actual_owned_mount=no)"
                )
            target = next(iter(roots))
        else:
            raise UnsafeRetry("fixture cleanup requires one unambiguous owned mount")
        if not self.cleanup_image_unchanged():
            raise UnsafeRetry("fixture image changed during cleanup observation")
        # Record intent before the call: even a timeout must never cause another detach.
        if self.cleanup_detach_attempted:
            raise UnsafeRetry("fixture cleanup detach was already attempted")
        self.cleanup_detach_attempted = True
        try:
            self.runner(["/usr/bin/hdiutil", "detach", target], check=True,
                        timeout=self.remaining(deadline), stdout=subprocess.DEVNULL)
        except BaseException as error:
            self.cleanup_failure = error
            raise
        for mount in self.attempted_mounts or {None}:
            self.registry_clear(mount, deadline)
        if any(mount.is_mount() for mount in self.attempted_mounts):
            raise UnsafeRetry("fixture remains mounted after cleanup")
        if not self.cleanup_image_unchanged():
            raise UnsafeRetry("fixture image changed during cleanup")

    def retry_safe(self, mount, identity, deadline):
        if file_identity(self.image) != self.identity or mount_identity(mount) != identity:
            raise UnsafeRetry("owned fixture identity changed")
        self.registry_clear(mount, deadline)
        if file_identity(self.image) != self.identity or mount_identity(mount) != identity:
            raise UnsafeRetry("owned fixture changed during registry observation")
        self.remaining(deadline)

    def report(self, error, attempt):
        value = error.stderr or b""
        tail = value[-4096:]
        if isinstance(tail, bytes):
            tail = tail.decode("utf-8", errors="replace")
        truncated = "[truncated to last 4096 bytes/characters] " if len(value) > 4096 else ""
        print(
            f"Native fixture attach attempt {attempt} failed; stderr={truncated}{tail!r}",
            file=self.log, flush=True,
        )

    def run(self, arguments, **options):
        coordinates = self.coordinates(arguments, options)
        if coordinates is None:
            return self.runner(arguments, **options)
        mount, identity = coordinates
        self.attempted_mounts.add(mount)
        deadline = self.clock() + 60
        previous = None
        for attempt in range(1, 4):
            try:
                remaining = self.remaining(deadline)
            except UnsafeRetry as reason:
                if previous is None:
                    raise subprocess.TimeoutExpired(arguments, 60) from reason
                previous.add_note(str(reason))
                raise previous
            try:
                result = self.runner(
                    arguments, **{**options, "timeout": remaining, "stderr": subprocess.PIPE}
                )
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
                self.report(error, attempt)
                previous = error
                stderr = error.stderr
                if isinstance(stderr, str):
                    stderr = stderr.encode("utf-8", errors="replace")
                if (not isinstance(error, subprocess.CalledProcessError) or error.returncode != 1
                        or stderr not in (EAGAIN, EAGAIN + b"\n", EAGAIN + b"\r\n")):
                    raise
                if attempt == 3:
                    error.add_note(
                        "Fixture attach stopped after 3 attempts within one 60-second budget"
                    )
                    raise
                try:
                    self.retry_safe(mount, identity, deadline)
                    delay = 0.2 * attempt
                    if self.remaining(deadline) <= delay:
                        raise UnsafeRetry("no deadline budget for bounded attach backoff")
                    self.sleep(delay)
                    self.retry_safe(mount, identity, deadline)
                except Exception as reason:
                    # Never include a registry payload or unrelated image paths in logs.
                    detail = (
                        str(reason) if isinstance(reason, UnsafeRetry) else type(reason).__name__
                    )
                    error.add_note("Fixture attach retry refused: " + detail)
                    raise error from None
                continue
            if self.clock() > deadline:
                error = subprocess.TimeoutExpired(
                    arguments, 60, output=result.stdout, stderr=result.stderr
                )
                error.add_note("Fixture attach completed beyond the shared deadline")
                self.report(error, attempt)
                raise error
            if result.stderr:
                output = result.stderr[-4096:]
                print(f"Native fixture attach stderr: {output!r}", file=self.log, flush=True)
            if attempt > 1:
                print(
                    f"Native fixture attach recovered on attempt {attempt}",
                    file=self.log, flush=True,
                )
            return result
        raise AssertionError("unreachable attach attempt state")

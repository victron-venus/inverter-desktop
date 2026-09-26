//! macOS keeps the existing installation key across ad-hoc-signed app updates.
//! All filesystem operations stay relative to opened, inspected directories;
//! the cache is immutable and published with an exclusive atomic rename.

use base64::{engine::general_purpose, Engine as _};
use rand::RngExt;
use std::ffi::{CStr, CString};
use std::fs::{File, Metadata};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

const KEY_FILE: &CStr = c"config.key";
const MAX_KEY_BYTES: u64 = 128;
const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;

fn owner() -> u32 {
    // SAFETY: geteuid has no preconditions and does not expose credentials.
    unsafe { libc::geteuid() }
}

fn open_at(directory: &File, name: &CStr, flags: i32, mode: u32) -> std::io::Result<File> {
    // SAFETY: the directory descriptor and NUL-terminated name live throughout
    // the call. A successful fresh descriptor is owned exactly once by File.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, mode) };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

fn check_directory(file: &File, final_directory: bool) -> Result<(), String> {
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect key directory")?;
    let trusted_owner = metadata.uid() == owner() || (!final_directory && metadata.uid() == 0);
    // Root-owned sticky temporary ancestors are needed by macOS's standard
    // private app/test directories; the selected app directory must be owned.
    let sticky_ancestor = !final_directory && metadata.uid() == 0 && metadata.mode() & 0o1000 != 0;
    if !metadata.is_dir() || !trusted_owner || (metadata.mode() & 0o022 != 0 && !sticky_ancestor) {
        return Err("Encryption key directory has unsafe ownership or permissions".into());
    }
    Ok(())
}

fn child_directory(parent: &File, name: &CStr, create: bool) -> Result<Option<File>, String> {
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    match open_at(parent, name, flags, 0) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            // SAFETY: parent is an open directory and name is NUL-terminated.
            if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err("Cannot create encryption key directory".into());
            }
            open_at(parent, name, flags, 0)
                .map(Some)
                .map_err(|_| "Cannot open encryption key directory".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Encryption key directory is inaccessible or a symlink".into()),
    }
}

fn app_directory(path: &Path) -> Result<File, String> {
    if !path.is_absolute() {
        return Err("Encryption key directory must be absolute".into());
    }
    let mut directory = File::open("/").map_err(|_| "Cannot open filesystem root")?;
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                check_directory(&directory, false)?;
                let name = CString::new(name.as_bytes()).map_err(|_| "Invalid key directory")?;
                directory = child_directory(&directory, &name, true)?
                    .ok_or("Cannot create encryption key directory")?;
            }
            _ => return Err("Encryption key directory contains traversal".into()),
        }
    }
    check_directory(&directory, true)?;
    Ok(directory)
}

fn validate_file(metadata: &Metadata, private: bool, limit: u64, uid: u32) -> Result<(), String> {
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != uid
        || metadata.len() > limit
        || (private && metadata.mode() & 0o7777 != 0o600)
    {
        return Err(
            "Encryption key or settings file has unsafe type, ownership, permissions or size"
                .into(),
        );
    }
    Ok(())
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    (
        before.dev(),
        before.ino(),
        before.len(),
        before.mtime(),
        before.mtime_nsec(),
        before.ctime(),
        before.ctime_nsec(),
    ) == (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec(),
    )
}

fn read_at(
    directory: &File,
    name: &CStr,
    private: bool,
    limit: u64,
) -> Result<Option<Vec<u8>>, String> {
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    let file = match open_at(directory, name, flags, 0) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err("Encryption key or settings file is inaccessible or a symlink".into())
        }
    };
    let before = file
        .metadata()
        .map_err(|_| "Cannot inspect encryption key or settings")?;
    validate_file(&before, private, limit, owner())?;
    let mut bytes = Vec::new();
    (&file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read encryption key or settings")?;
    let after = file
        .metadata()
        .map_err(|_| "Cannot inspect encryption key or settings")?;
    validate_file(&after, private, limit, owner())?;
    if bytes.len() as u64 != before.len() || !unchanged(&before, &after) {
        return Err("Encryption key or settings changed while reading".into());
    }
    Ok(Some(bytes))
}

fn read_key(directory: &File) -> Result<Option<Vec<u8>>, String> {
    read_at(directory, KEY_FILE, true, MAX_KEY_BYTES)?
        .map(|bytes| {
            let value = std::str::from_utf8(&bytes).map_err(|_| "Invalid config.key encoding")?;
            super::decode_key(value)
                .map_err(|_| "Invalid config.key; restore the original installation key".into())
        })
        .transpose()
}

fn has_plugin_settings(directory: &File) -> Result<bool, String> {
    let Some(plugins) = child_directory(directory, c"plugins", false)? else {
        return Ok(false);
    };
    check_directory(&plugins, true)?;
    let Some(settings) = child_directory(&plugins, c"settings", false)? else {
        return Ok(false);
    };
    check_directory(&settings, true)?;
    // Readdir uses an owned duplicate of the inspected directory, never a
    // pathname that could be redirected between inspection and iteration.
    let owned = settings
        .try_clone()
        .map_err(|_| "Cannot inspect plugin settings")?;
    use std::os::fd::IntoRawFd;
    let fd = owned.into_raw_fd();
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        // SAFETY: fdopendir takes ownership only on success.
        unsafe { libc::close(fd) };
        return Err("Cannot inspect plugin settings".into());
    }
    let result = directory_nonempty(stream);
    // SAFETY: stream was successfully opened and is closed exactly once.
    unsafe { libc::closedir(stream) };
    result
}

fn directory_nonempty(stream: *mut libc::DIR) -> Result<bool, String> {
    loop {
        // SAFETY: caller owns the live DIR stream. Reset errno distinguishes
        // end-of-directory from an I/O error without trusting an empty result.
        let error = unsafe {
            #[cfg(target_os = "macos")]
            {
                libc::__error()
            }
            #[cfg(target_os = "linux")]
            {
                libc::__errno_location()
            }
        };
        unsafe { *error = 0 };
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            return if unsafe { *error } == 0 {
                Ok(false)
            } else {
                Err("Cannot inspect plugin settings".into())
            };
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name != c"." && name != c".." {
            return Ok(true);
        }
    }
}

fn has_encrypted_data(directory: &File) -> Result<bool, String> {
    if let Some(bytes) = read_at(directory, c"config.json", false, MAX_CONFIG_BYTES)? {
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
            "Cannot inspect existing configuration; encryption key was not replaced"
        })?;
        let object = value
            .as_object()
            .ok_or("Existing configuration is not an object")?;
        if let Some(config) = object.get("config") {
            if config.is_string() {
                return Ok(true);
            }
            // The store also accepts a legacy plaintext FullConfig object.
            // Validate that exact format before permitting first-time encryption.
            serde_json::from_value::<super::FullConfig>(config.clone())
                .map_err(|_| "Existing configuration has an invalid legacy record")?;
        }
    }
    has_plugin_settings(directory)
}

struct PendingKey<'a> {
    directory: &'a File,
    name: CString,
}

impl Drop for PendingKey<'_> {
    fn drop(&mut self) {
        // SAFETY: only the unpredictable temporary basename owned by this
        // transaction is removed; the final config.key is never removed.
        unsafe { libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0) };
    }
}

fn rename_exclusive(directory: &File, source: &CStr) -> std::io::Result<()> {
    // SAFETY: both names are relative to the same live directory descriptor.
    // Both platform primitives atomically fail if the destination exists.
    let result = unsafe {
        #[cfg(target_os = "macos")]
        {
            libc::renameatx_np(
                directory.as_raw_fd(),
                source.as_ptr(),
                directory.as_raw_fd(),
                KEY_FILE.as_ptr(),
                libc::RENAME_EXCL,
            )
        }
        #[cfg(target_os = "linux")]
        {
            libc::renameat2(
                directory.as_raw_fd(),
                source.as_ptr(),
                directory.as_raw_fd(),
                KEY_FILE.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        }
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn persist_key(directory: &File, key: &[u8]) -> Result<Vec<u8>, String> {
    if key.len() != 32 {
        return Err("Invalid encryption key length".into());
    }
    let name = CString::new(format!(".config-key-{}", uuid::Uuid::new_v4())).unwrap();
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mut file = open_at(directory, &name, flags, 0o600)
        .map_err(|_| "Cannot create private encryption key cache")?;
    // Only arm cleanup after exclusive creation proves this is our own file.
    let pending = PendingKey { directory, name };
    // The creation mode is restrictive even before this call; fchmod handles a
    // more restrictive inherited umask without opening a plaintext exposure.
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|_| "Cannot secure encryption key cache")?;
    validate_file(
        &file
            .metadata()
            .map_err(|_| "Cannot inspect encryption key cache")?,
        true,
        MAX_KEY_BYTES,
        owner(),
    )?;
    file.write_all(general_purpose::STANDARD.encode(key).as_bytes())
        .map_err(|_| "Cannot write encryption key cache")?;
    file.sync_all()
        .map_err(|_| "Cannot persist encryption key cache")?;
    match rename_exclusive(directory, &pending.name) {
        Ok(()) => directory
            .sync_all()
            .map_err(|_| "Cannot persist encryption key directory")?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => {
            return Err("Cannot publish encryption key cache without replacing a file".into())
        }
    }
    // A concurrently created valid cache wins. Never return a new key whose
    // durable file was not installed, and never overwrite or unlink that winner.
    read_key(directory)?.ok_or("Encryption key cache disappeared during creation".into())
}

pub(super) fn load_or_create(
    path: &Path,
    read_keychain: impl FnOnce() -> Result<Option<Vec<u8>>, String>,
) -> Result<Vec<u8>, String> {
    let directory = app_directory(path)?;
    if let Some(key) = read_key(&directory)? {
        return Ok(key);
    }
    let keychain = read_keychain();
    // Another launch may have published the cache while this one waited for
    // Keychain authorization. Prefer that durable key even if access was denied.
    if let Some(key) = read_key(&directory)? {
        return Ok(key);
    }
    if let Some(key) = keychain? {
        return persist_key(&directory, &key);
    }
    if has_encrypted_data(&directory)? {
        // A concurrent winner can also save ciphertext during this inspection.
        return read_key(&directory)?.ok_or("Existing encrypted settings have no available key; restore config.key or the original macOS Keychain entry".into());
    }
    let mut key = [0u8; 32];
    rand::rng().fill(&mut key);
    persist_key(&directory, &key)
}

#[cfg(test)]
mod tests;

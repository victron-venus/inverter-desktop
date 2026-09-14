//! Read and write user-selected backup documents, including Android SAF URIs.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use tauri_plugin_dialog::FilePath;

#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
}

impl Access {
    fn open_local(self, path: &std::path::Path) -> io::Result<File> {
        match self {
            Self::Read => File::open(path),
            Self::Write => OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path),
        }
    }
}

fn open_document(
    document: FilePath,
    access: Access,
    resolve_content_uri: impl FnOnce(FilePath, Access) -> io::Result<File>,
) -> io::Result<File> {
    match document {
        FilePath::Path(path) => access.open_local(&path),
        FilePath::Url(url) if url.scheme() == "file" => {
            let path = url.to_file_path().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "Invalid backup file URL")
            })?;
            access.open_local(&path)
        }
        FilePath::Url(url) if url.scheme() == "content" => {
            // Preserve the complete URI: document IDs are opaque and are not
            // filesystem paths, including percent escapes and provider names.
            resolve_content_uri(FilePath::Url(url), access)
        }
        FilePath::Url(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unsupported backup document URL",
        )),
    }
}

fn open(app: &tauri::AppHandle, document: FilePath, access: Access) -> io::Result<File> {
    open_document(document, access, |uri, access| {
        #[cfg(target_os = "android")]
        {
            use tauri_plugin_fs::{FsExt, OpenOptions};
            let mut options = OpenOptions::new();
            match access {
                Access::Read => options.read(true),
                // "wt" tells ContentResolver to truncate an existing document.
                Access::Write => options.write(true).create(true).truncate(true),
            };
            app.fs().open(uri, options)
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (app, uri, access);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Content-provider documents require Android",
            ))
        }
    })
}

pub(super) fn write(
    app: &tauri::AppHandle,
    document: FilePath,
    content: &str,
) -> Result<(), String> {
    let write_document = || -> io::Result<()> {
        let mut file = open(app, document, Access::Write)?;
        file.write_all(content.as_bytes())?;
        file.flush()
    };
    write_document().map_err(|error| format!("Failed to write backup file: {error}"))
}

pub(super) fn read(app: &tauri::AppHandle, document: FilePath) -> Result<String, String> {
    let read_document = || -> io::Result<String> {
        let mut content = String::new();
        open(app, document, Access::Read)?.read_to_string(&mut content)?;
        Ok(content)
    };
    read_document().map_err(|error| format!("Failed to read backup file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TemporaryDocument(std::path::PathBuf);

    impl TemporaryDocument {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("backup-test-{}.json", uuid::Uuid::new_v4())))
        }
    }

    impl Drop for TemporaryDocument {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn saf_uri_round_trip_keeps_document_id_and_truncates_previous_backup() {
        let backing = TemporaryDocument::new();
        std::fs::write(&backing.0, "obsolete contents longer than the replacement").unwrap();
        let uri = "content://com.android.externalstorage.documents/document/primary%3ADownload%2Fbackup.json";
        let document = || FilePath::Url(uri.parse().unwrap());
        // A provider returns a descriptor; the caller must not reinterpret its
        // opaque document ID as a local path. Model that boundary with a file.
        let provider = |selected: FilePath, access: Access| {
            assert_eq!(selected.to_string(), uri);
            access.open_local(&backing.0)
        };
        open_document(document(), Access::Write, provider)
            .unwrap()
            .write_all("{\"label\":\"Сонце ☀\"}".as_bytes())
            .unwrap();
        let mut restored = String::new();
        open_document(document(), Access::Read, provider)
            .unwrap()
            .read_to_string(&mut restored)
            .unwrap();
        assert_eq!(restored, "{\"label\":\"Сонце ☀\"}");
    }

    #[test]
    fn native_paths_and_file_urls_use_local_io() {
        let backing = TemporaryDocument::new();
        let no_provider = |_: FilePath, _: Access| panic!("Local files must not use a provider");
        open_document(
            FilePath::Path(backing.0.clone()),
            Access::Write,
            no_provider,
        )
        .unwrap()
        .write_all(b"{\"config\":true}")
        .unwrap();
        let file_url = tauri::Url::from_file_path(&backing.0).unwrap();
        let mut content = String::new();
        open_document(FilePath::Url(file_url), Access::Read, no_provider)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "{\"config\":true}");
    }

    #[test]
    fn provider_failure_and_unsupported_uri_are_errors() {
        let document = FilePath::Url("content://provider/document/id".parse().unwrap());
        let error = open_document(document, Access::Read, |_, _| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Permission revoked",
            ))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        let document = FilePath::Url("https://example.invalid/backup.json".parse().unwrap());
        let error = open_document(document, Access::Read, |_, _| {
            panic!("Unsupported URLs must not reach a content provider")
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}

//! Explicit publisher-side package creation. This example never installs or runs a worker.

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Component, Path, PathBuf};

    use ed25519_dalek::SigningKey;
    use inverter_dashboard_lib::plugins::package::read_regular_file;
    use inverter_dashboard_lib::plugins::packaging::{build_package, write_package_atomic};
    use inverter_dashboard_lib::plugins::protocol::{PluginManifest, MAX_MANIFEST_BYTES};

    const HELP: &str = "Create a signed desktop .idplugin package without installing or executing it.

Usage:
  cargo run --example plugin-package -- --manifest FILE --root DIRECTORY --key-id ID --key-file FILE --output FILE.idplugin

All options are required. Existing output files are never overwritten.
--manifest  Unsigned PluginManifest JSON metadata; inventory and signature are rebuilt.
--root      Payload directory containing the entrypoint and all files to include.
--key-id    Publisher key identifier already agreed with the receiving trust store.
--key-file  Existing regular file containing exactly 32 raw Ed25519 seed bytes.
--output    New .idplugin file in an existing directory.

Keep the manifest and private key outside the payload directory. Source paths and
their parent components must not be symlinks/reparse points. Unix hard links are
rejected. Source filenames must be portable
ASCII paths; manifest.json is reserved. Archive timestamps and file modes are fixed.
Key bytes are read only from --key-file, never from arguments or environment.
This command does not generate keys, establish publisher trust, install packages,
or start workers.";

    pub fn run() -> Result<(), String> {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
            println!("{HELP}");
            return Ok(());
        }
        let values = parse_options(args)?;
        let manifest_path = PathBuf::from(&values["--manifest"]);
        let root = PathBuf::from(&values["--root"]);
        let key_path = PathBuf::from(&values["--key-file"]);
        let output = PathBuf::from(&values["--output"]);
        if output
            .extension()
            .is_none_or(|extension| extension != "idplugin")
        {
            return Err("--output must end in .idplugin".into());
        }
        let key_id = values["--key-id"]
            .to_str()
            .ok_or("--key-id must be a UTF-8 identifier")?;
        reject_symlink_components(&root)?;
        reject_symlink_components(&manifest_path)?;
        reject_symlink_components(&key_path)?;
        let canonical_root = root
            .canonicalize()
            .map_err(|_| "cannot open payload directory")?;
        for path in [&manifest_path, &key_path] {
            if path
                .canonicalize()
                .map_err(|_| "cannot inspect input file")?
                .starts_with(&canonical_root)
            {
                return Err("manifest and key files must be outside the payload directory".into());
            }
        }
        let manifest_bytes = read_regular_file(&manifest_path, MAX_MANIFEST_BYTES)?;
        // Do not echo parse errors: malformed input may contain values that the
        // operator did not intend to print in terminal logs.
        let manifest: PluginManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|_| "manifest file must contain PluginManifest JSON metadata")?;
        let mut key_bytes = read_regular_file(&key_path, 32)?;
        if key_bytes.len() != 32 {
            key_bytes.fill(0);
            return Err("key file must contain exactly 32 raw Ed25519 seed bytes".into());
        }
        let mut seed = [0_u8; 32];
        seed.copy_from_slice(&key_bytes);
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        key_bytes.fill(0);
        let archive = build_package(manifest, &root, key_id, &key)?;
        write_package_atomic(&output, &archive)?;
        println!(
            "Signed package created ({} bytes). No worker was installed or run.",
            archive.len()
        );
        Ok(())
    }

    fn parse_options(args: Vec<OsString>) -> Result<BTreeMap<String, OsString>, String> {
        const NAMES: [&str; 5] = ["--manifest", "--root", "--key-id", "--key-file", "--output"];
        if args.len() != NAMES.len() * 2 {
            return Err("provide all five option/value pairs; use --help for the format".into());
        }
        let mut values = BTreeMap::new();
        for pair in args.as_chunks::<2>().0 {
            let name = pair[0].to_str().ok_or("invalid option name")?;
            if !NAMES.contains(&name) || values.insert(name.into(), pair[1].clone()).is_some() {
                return Err("unknown or duplicate option; use --help for the format".into());
            }
        }
        Ok(values)
    }

    fn reject_symlink_components(path: &Path) -> Result<(), String> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| "cannot locate working directory")?
                .join(path)
        };
        let mut checked = PathBuf::new();
        for component in absolute.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => checked.push(component.as_os_str()),
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err("input paths must not contain parent traversal".into())
                }
                Component::Normal(name) => {
                    checked.push(name);
                    let metadata =
                        fs::symlink_metadata(&checked).map_err(|_| "cannot inspect input path")?;
                    #[cfg(windows)]
                    let is_link = {
                        use std::os::windows::fs::MetadataExt;
                        metadata.file_attributes() & 0x400 != 0
                    };
                    #[cfg(not(windows))]
                    let is_link = metadata.file_type().is_symlink();
                    if is_link {
                        return Err(
                            "input paths and their parents must not be symlinks or reparse points"
                                .into(),
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn main() {
    if let Err(error) = desktop::run() {
        eprintln!("Package creation failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn main() {
    eprintln!("Plugin packages are supported only on desktop targets.");
    std::process::exit(1);
}

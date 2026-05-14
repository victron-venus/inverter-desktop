use std::fs;
use std::path::Path;
use std::env;

fn main() {
    // Read VERSION file
    let version = fs::read_to_string("VERSION")
        .expect("Failed to read VERSION file")
        .trim()
        .to_string();

    // Set version for Cargo
    println!("cargo:rustc-env=APP_VERSION={}", version);

    // Also set the version from Cargo.toml for consistency
    println!("cargo:rustc-env=CARGO_PKG_VERSION={}", version);

    tauri_build::build()
}

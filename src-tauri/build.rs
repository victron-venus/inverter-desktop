fn main() {
    // Read VERSION file
    let version = std::fs::read_to_string("VERSION")
        .expect("Failed to read VERSION file")
        .trim()
        .to_string();

    // Set version for Cargo
    println!("cargo:rustc-env=CARGO_PKG_VERSION={}", version);

    tauri_build::build()
}

use std::env;

fn main() {
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set");
    env::set_var("TAURI_APP_VERSION", &version);
    println!("cargo:rustc-env=APP_VERSION={}", version);

    // biometric.m is Apple-only (uses LocalAuthentication), skip for Android/Linux/Windows
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("apple") {
        cc::Build::new()
            .file("src/biometric.m")
            .compile("biometric");
        println!("cargo:rustc-link-lib=framework=LocalAuthentication");
    }

    tauri_build::build()
}

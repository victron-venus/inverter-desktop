use std::env;

fn main() {
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set");
    env::set_var("TAURI_APP_VERSION", &version);
    println!("cargo:rustc-env=APP_VERSION={}", version);

    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/biometric.m")
            .compile("biometric");
        println!("cargo:rustc-link-lib=framework=LocalAuthentication");
    }

    tauri_build::build()
}

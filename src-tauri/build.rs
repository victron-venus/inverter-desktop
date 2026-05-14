use std::env;

fn main() {
    let version = env::var("CARGO_PKG_VERSION")
        .expect("CARGO_PKG_VERSION not set");
    env::set_var("TAURI_APP_VERSION", &version);
    println!("cargo:rustc-env=APP_VERSION={}", version);
    tauri_build::build()
}

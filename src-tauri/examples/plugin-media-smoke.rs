//! Explicit native media smoke entry; never used by the production executable.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn main() {
    if let Err(error) = inverter_dashboard_lib::run_native_media_smoke() {
        eprintln!("Native media smoke failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn main() {
    compile_error!("The native media smoke executable is desktop-only");
}

fn main() {
    println!("cargo:rerun-if-env-changed=INVERTER_BUILD_PROFILE");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    assert!(
        matches!(target.as_str(), "macos" | "linux" | "windows")
            && std::env::var("INVERTER_BUILD_PROFILE").as_deref() != Ok("mobile"),
        "Ring worker supports desktop builds only"
    );
}

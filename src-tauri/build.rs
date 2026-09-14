use std::env;
use std::path::PathBuf;
use std::process::Command;

mod mobile_build;
mod release_identity;

fn main() {
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set");
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .to_owned();
    let frontend_receipt = root.join("dist/build-profile.json");
    println!("cargo:rerun-if-changed={}", frontend_receipt.display());
    let target = env::var("TARGET").unwrap_or_default();
    let profile = env::var("PROFILE").unwrap_or_default();
    let receipt = std::fs::read_to_string(&frontend_receipt).ok();
    mobile_build::validate_frontend(&target, &profile, receipt.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    let plan_path = root.join(".release-plan.json");
    println!("cargo:rerun-if-changed={}", plan_path.display());
    println!("cargo:rerun-if-env-changed=RELEASE_CHANNEL");
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let sha = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    // A new checkout must invalidate any cached build-time identity, including worktrees.
    for name in ["HEAD", "refs/heads"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    let plan = match std::fs::read_to_string(&plan_path) {
        Ok(plan) => Some(plan),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("Cannot read release plan: {error}"),
    };
    if env::var("RELEASE_CHANNEL").is_ok() && plan.is_none() {
        panic!("Release builds require .release-plan.json");
    }
    let identity = release_identity::release_identity(&version, &sha, plan.as_deref())
        .unwrap_or_else(|error| panic!("Invalid release identity: {error}"));
    if let Ok(channel) = env::var("RELEASE_CHANNEL") {
        assert_eq!(
            identity["channel"].as_str(),
            Some(channel.as_str()),
            "Release channel mismatch"
        );
    }
    std::fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("release-info.json"),
        serde_json::to_vec(&identity).unwrap(),
    )
    .expect("Cannot embed release identity");
    env::set_var(
        "TAURI_APP_VERSION",
        identity["base_version"].as_str().unwrap(),
    );
    println!(
        "cargo:rustc-env=APP_VERSION={}",
        identity["version"].as_str().unwrap()
    );

    // biometric.m is Apple-only (uses LocalAuthentication), skip for Android/Linux/Windows
    if target.contains("apple") {
        cc::Build::new()
            .file("src/biometric.m")
            .compile("biometric");
        println!("cargo:rustc-link-lib=framework=LocalAuthentication");
    }

    if target.contains("apple-ios") {
        cc::Build::new()
            .file("src/mobile_credentials.m")
            .compile("mobile_credentials");
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rerun-if-changed=src/mobile_credentials.m");
    }

    tauri_build::build()
}

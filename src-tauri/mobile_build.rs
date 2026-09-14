//! Reject stale desktop frontend assets when Cargo builds a mobile release directly.

pub fn validate_frontend(target: &str, profile: &str, receipt: Option<&str>) -> Result<(), String> {
    let mobile = target.contains("-apple-ios") || target.contains("-linux-android");
    if !mobile || profile != "release" {
        return Ok(());
    }
    let receipt = receipt
        .ok_or("Mobile release requires dist/build-profile.json; run pnpm build:mobile first")?;
    let value: serde_json::Value = serde_json::from_str(receipt)
        .map_err(|error| format!("Invalid frontend build receipt: {error}"))?;
    if value["schema_version"] != 1 || value["profile"] != "mobile" {
        return Err("Mobile release requires a mobile frontend build receipt".into());
    }
    let modules = value["modules"]
        .as_array()
        .ok_or("Frontend build receipt has no module graph")?;
    if modules.iter().any(|module| !module.is_string())
        || !modules.iter().any(|module| module == "src/main.ts")
        || !modules
            .iter()
            .any(|module| module == "src/features/mobile.ts")
    {
        return Err("Frontend build receipt is missing the mobile application graph".into());
    }
    for module in modules.iter().filter_map(serde_json::Value::as_str) {
        if module.starts_with("src/features/desktop.")
            || module.starts_with("src/features/desktop/")
            || module.starts_with("src/features/messages.desktop.")
            || module.starts_with("src/plugins/")
            || matches!(
                module,
                "src/CameraVideo.vue"
                    | "src/composables/useHA.ts"
                    | "src/composables/useDashboardControlsConfig.ts"
                    | "src/components/HaEntitiesEditor.vue"
                    | "src/components/EntityAutocompleteInput.vue"
            )
        {
            return Err(format!(
                "Desktop module in mobile frontend receipt: {module}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(profile: &str, modules: serde_json::Value) -> String {
        serde_json::json!({"schema_version": 1, "profile": profile, "modules": modules}).to_string()
    }

    #[test]
    fn mobile_release_requires_mobile_assets_for_every_target_family() {
        let valid = receipt(
            "mobile",
            serde_json::json!(["src/main.ts", "src/features/mobile.ts"]),
        );
        let desktop = receipt(
            "desktop",
            serde_json::json!(["src/main.ts", "src/features/desktop.ts"]),
        );
        for target in [
            "aarch64-apple-ios",
            "aarch64-apple-ios-sim",
            "aarch64-linux-android",
            "armv7-linux-androideabi",
        ] {
            assert!(validate_frontend(target, "release", None).is_err());
            assert!(validate_frontend(target, "release", Some(&desktop)).is_err());
            assert!(validate_frontend(target, "release", Some(&valid)).is_ok());
        }
    }

    #[test]
    fn invalid_or_incomplete_receipts_are_rejected() {
        for invalid in [
            "garbage".into(),
            "{}".into(),
            receipt("mobile", serde_json::json!([])),
            receipt("mobile", serde_json::json!(["src/main.ts"])),
            receipt("mobile", serde_json::json!(["src/main.ts", 123])),
        ] {
            assert!(validate_frontend("aarch64-apple-ios", "release", Some(&invalid)).is_err());
        }
    }

    #[test]
    fn mobile_label_cannot_hide_a_mixed_module_graph() {
        for desktop_module in [
            "src/features/desktop.ts",
            "src/features/desktop/CameraAction.vue",
            "src/features/messages.desktop.ts",
            "src/plugins/install.ts",
            "src/CameraVideo.vue",
            "src/composables/useHA.ts",
            "src/composables/useDashboardControlsConfig.ts",
            "src/components/HaEntitiesEditor.vue",
            "src/components/EntityAutocompleteInput.vue",
        ] {
            let mixed = receipt(
                "mobile",
                serde_json::json!(["src/main.ts", "src/features/mobile.ts", desktop_module]),
            );
            assert!(
                validate_frontend("aarch64-linux-android", "release", Some(&mixed)).is_err(),
                "accepted {desktop_module}"
            );
        }
    }

    #[test]
    fn debug_checks_and_desktop_builds_do_not_need_packaged_assets() {
        assert!(validate_frontend("aarch64-apple-ios", "debug", None).is_ok());
        assert!(validate_frontend("aarch64-linux-android", "debug", None).is_ok());
        assert!(validate_frontend("aarch64-apple-darwin", "release", None).is_ok());
    }
}

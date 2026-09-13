#[tauri::command]
pub(super) fn get_release_info() -> serde_json::Value {
    serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/release-info.json")))
        .expect("release identity was validated by build.rs")
}

#[cfg(test)]
#[path = "../release_identity.rs"]
mod identity_tests;

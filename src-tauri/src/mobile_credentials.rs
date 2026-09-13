//! Native, installation-specific keys for encrypted configuration files.
//!
//! The Android bridge is private to Rust. In particular, do not expose its key
//! command through a webview capability or Tauri's native-plugin fallback.

#[cfg(target_os = "android")]
use tauri::{plugin::PluginHandle, Manager};

#[cfg(target_os = "android")]
struct NativeStore(PluginHandle<tauri::Wry>);

/// Register before the application's setup hook first loads configuration.
pub(crate) fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("mobile-credentials")
        .invoke_handler(|invoke| {
            // Returning true is essential: false would forward an unhandled
            // webview invoke to the Android native plugin, exposing the key.
            invoke
                .resolver
                .reject("Credential keys are not available to the webview");
            true
        })
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                let handle = api
                    .register_android_plugin("com.alvit.inverter_dashboard", "SecureStorePlugin")?;
                app.manage(NativeStore(handle));
            }
            #[cfg(target_os = "ios")]
            let _ = (app, api);
            Ok(())
        })
        .build()
}

#[cfg(target_os = "android")]
pub(crate) fn encryption_key(app: &tauri::AppHandle) -> Result<Vec<u8>, String> {
    #[derive(serde::Deserialize)]
    struct KeyResponse {
        key: Vec<u8>,
    }

    let store = app
        .try_state::<NativeStore>()
        .ok_or_else(|| "Android credential storage was not initialized".to_string())?;
    let response: KeyResponse = store
        .0
        .run_mobile_plugin("encryptionKey", ())
        .map_err(|error| format!("Android credential storage unavailable: {error}"))?;
    if response.key.len() != 32 {
        return Err("Android credential storage returned an invalid key length".into());
    }
    Ok(response.key)
}

#[cfg(target_os = "ios")]
pub(crate) fn encryption_key(_app: &tauri::AppHandle) -> Result<Vec<u8>, String> {
    unsafe extern "C" {
        fn inverter_mobile_encryption_key(output: *mut u8, length: usize) -> i32;
    }

    let mut key = vec![0_u8; 32];
    // SAFETY: the native helper writes exactly 32 bytes to this live allocation
    // and never retains its pointer. It returns an OSStatus without key material.
    let status = unsafe { inverter_mobile_encryption_key(key.as_mut_ptr(), key.len()) };
    if status != 0 {
        return Err(format!(
            "iOS Keychain unavailable (OSStatus {status}); saved credentials were not overwritten"
        ));
    }
    Ok(key)
}

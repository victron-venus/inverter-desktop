//! TLS trust shared by remote Gateway HTTPS and inverter MQTT.
//! Explicit roots also avoid reqwest's Android platform verifier, which requires
//! a separate JNI/Kotlin initialization that this application does not install.
use rumqttc::tokio_rustls::rustls;
use std::sync::Arc;

pub(crate) fn client_config() -> Result<rustls::ClientConfig, String> {
    client_config_with_roots(platform_certificates()?)
}

pub(crate) fn client_config_with_roots(
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
) -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    let (accepted, _) = roots.add_parsable_certificates(certs);
    if accepted == 0 {
        return Err(
            "TLS cannot load trusted system certificates; check the device's certificate store"
                .into(),
        );
    }
    Ok(rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| "TLS cannot initialize supported TLS versions")?
    .with_root_certificates(roots)
    .with_no_client_auth())
}

#[cfg(not(target_os = "android"))]
fn platform_certificates() -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, String> {
    let result = rustls_native_certs::load_native_certs();
    // Some system stores contain unusable entries; retain successfully loaded roots.
    if result.certs.is_empty() {
        return Err(
            "TLS cannot load trusted system certificates; check the device's certificate store"
                .into(),
        );
    }
    Ok(result.certs)
}

#[cfg(target_os = "android")]
fn platform_certificates() -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, String> {
    // Android 14+ updates the system roots through the Conscrypt APEX. The
    // rustls-native-certs generic Unix probe otherwise searches Termux paths.
    for directory in [
        "/apex/com.android.conscrypt/cacerts",
        "/system/etc/security/cacerts",
    ] {
        let path = std::path::Path::new(directory);
        if !path.is_dir() {
            continue;
        }
        let result = rustls_native_certs::load_certs_from_paths(None, Some(path));
        if !result.certs.is_empty() {
            return Ok(result.certs);
        }
    }
    Err(
        "TLS cannot load Android system certificates; update the device's security components"
            .into(),
    )
}

//! Native-only password verifiers. Plaintext is accepted only for migration or an
//! explicit authenticated password change; it is never written by current code.
use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Algorithm, Argon2, Params, Version,
};
use serde::{Deserialize, Serialize};

const MEMORY_KIB: u32 = 19 * 1024;
const ITERATIONS: u32 = 2;
const LANES: u32 = 1;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Verifier(String);

impl std::fmt::Debug for Verifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Verifier([REDACTED])")
    }
}

fn algorithm() -> Result<Argon2<'static>, String> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, LANES, Some(32))
        .map_err(|_| "Cannot initialize password protection")?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

impl Verifier {
    fn create(password: &str) -> Result<Self, String> {
        algorithm()?
            .hash_password(password.as_bytes())
            .map(|hash| Self(hash.to_string()))
            .map_err(|_| "Cannot protect authentication password".into())
    }

    fn parsed(&self) -> Result<PasswordHash, String> {
        // Parse and bound the stored parameters before allowing allocation or
        // derivation. This version supports one explicit, upgradeable policy.
        let parsed =
            PasswordHash::new(&self.0).map_err(|_| "Invalid stored authentication verifier")?;
        if parsed.algorithm.as_str() != "argon2id"
            || parsed.version != Some(19)
            || parsed.params.iter().count() != 3
            || parsed.params.get_decimal("m") != Some(MEMORY_KIB)
            || parsed.params.get_decimal("t") != Some(ITERATIONS)
            || parsed.params.get_decimal("p") != Some(LANES)
            || parsed.salt.as_ref().is_none_or(|salt| salt.len() < 16)
            || parsed.hash.as_ref().is_none_or(|hash| hash.len() != 32)
        {
            return Err("Unsupported stored authentication verifier".into());
        }
        Ok(parsed)
    }

    pub(super) fn matches(&self, password: &str) -> Result<bool, String> {
        let parsed = self.parsed()?;
        Ok(algorithm()?
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }
}

/// Convert legacy state before it leaves native storage. A present but corrupt
/// verifier never falls back to a retained plaintext value.
pub(super) fn migrate(config: &mut crate::FullConfig) -> Result<bool, String> {
    if let Some(verifier) = &config.auth_password_verifier {
        verifier.parsed()?;
        return Ok(config.auth_password.take().is_some());
    }
    let Some(password) = config.auth_password.take() else {
        return Ok(false);
    };
    if !password.is_empty() {
        config.auth_password_verifier = Some(Verifier::create(&password)?);
    }
    Ok(true)
}

/// Core settings roundtrips omit private verifier state. A nonempty plaintext
/// input is an explicit password change by the authenticated caller.
pub(super) fn prepare_save(
    incoming: &mut crate::FullConfig,
    current: &crate::FullConfig,
) -> Result<(), String> {
    if incoming.auth_password_verifier.is_some() {
        return Err("Authentication verifiers cannot be supplied through settings".into());
    }
    incoming.auth_password_verifier = current.auth_password_verifier.clone();
    if let Some(password) = incoming
        .auth_password
        .take()
        .filter(|value| !value.is_empty())
    {
        let unchanged = match &incoming.auth_password_verifier {
            Some(verifier) => verifier.matches(&password)?,
            None => false,
        };
        if !unchanged {
            incoming.auth_password_verifier = Some(Verifier::create(&password)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_password_bytes_and_unique_salts() {
        let first = Verifier::create("  пароль🔒\n").unwrap();
        let second = Verifier::create("  пароль🔒\n").unwrap();
        assert_ne!(first, second);
        assert!(first.matches("  пароль🔒\n").unwrap());
        assert!(!first.matches("пароль🔒").unwrap());
        assert!(!format!("{first:?}").contains("argon2"));
    }

    #[test]
    fn migration_is_idempotent_and_corrupt_verifier_has_no_plaintext_fallback() {
        let mut config = crate::FullConfig {
            auth_password: Some("legacy".into()),
            ..Default::default()
        };
        assert!(migrate(&mut config).unwrap());
        assert!(config.auth_password.is_none());
        let verifier = config.auth_password_verifier.clone().unwrap();
        assert!(verifier.matches("legacy").unwrap());
        assert!(!migrate(&mut config).unwrap());
        assert_eq!(config.auth_password_verifier.as_ref(), Some(&verifier));
        config.auth_password_verifier = Some(Verifier("broken".into()));
        config.auth_password = Some("legacy".into());
        assert!(migrate(&mut config).is_err());
    }

    #[test]
    fn stored_parameters_are_bounded_before_hashing() {
        let verifier = Verifier::create("secret").unwrap();
        for changed in [
            verifier.0.replace("m=19456", "m=4294967295"),
            verifier.0.replace("t=2", "t=999999"),
            verifier.0.replace("argon2id", "argon2i"),
            verifier.0.replace("v=19", "v=16"),
        ] {
            assert!(Verifier(changed).matches("secret").is_err());
        }
    }

    #[test]
    fn settings_roundtrip_retains_private_record_and_rejects_injection() {
        let mut current = crate::FullConfig {
            auth_password: Some("current".into()),
            ..Default::default()
        };
        migrate(&mut current).unwrap();
        for password in [None, Some("".into()), Some("current".into())] {
            let mut incoming = crate::FullConfig {
                auth_password: password,
                ..Default::default()
            };
            prepare_save(&mut incoming, &current).unwrap();
            assert_eq!(
                incoming.auth_password_verifier,
                current.auth_password_verifier
            );
            assert!(incoming.auth_password.is_none());
        }
        let mut injection = current.clone();
        assert!(prepare_save(&mut injection, &current).is_err());
        let mut changed = crate::FullConfig {
            auth_password: Some("new".into()),
            ..Default::default()
        };
        prepare_save(&mut changed, &current).unwrap();
        assert!(changed
            .auth_password_verifier
            .as_ref()
            .unwrap()
            .matches("new")
            .unwrap());
        assert!(!changed
            .auth_password_verifier
            .as_ref()
            .unwrap()
            .matches("current")
            .unwrap());
    }
}

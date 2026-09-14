//! Release-owned publisher trust. Packages and application settings cannot add keys.

use super::package::{PublisherTrust, TrustStore};
use serde::Deserialize;

const MAX_POLICY_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublisherPolicy {
    schema_version: u32,
    publishers: Vec<Publisher>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Publisher {
    key_id: String,
    public_key: String,
    plugin_ids: Vec<String>,
}

/// Only a reviewed application release can change this policy. Until real
/// publisher keys are configured, every package is rejected as untrusted.
pub fn embedded_trust() -> Result<TrustStore, String> {
    parse_policy(include_str!("publishers.json"))
}

fn parse_policy(json: &str) -> Result<TrustStore, String> {
    if json.len() > MAX_POLICY_BYTES {
        return Err("Publisher policy exceeds size limit".into());
    }
    let policy: PublisherPolicy =
        serde_json::from_str(json).map_err(|_| "Invalid publisher policy")?;
    if policy.schema_version != 1 || policy.publishers.len() > 64 {
        return Err("Unsupported publisher policy".into());
    }
    let publishers = policy
        .publishers
        .into_iter()
        .map(|publisher| {
            let value = publisher.public_key.as_bytes();
            if value.len() != 64
                || !value
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
                || publisher.plugin_ids.is_empty()
                || publisher.plugin_ids.len() > 64
            {
                return Err("Invalid publisher key or plugin scope".into());
            }
            let mut key = [0_u8; 32];
            for (index, byte) in key.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&publisher.public_key[index * 2..index * 2 + 2], 16)
                    .map_err(|_| "Invalid publisher key")?;
            }
            PublisherTrust::new(publisher.key_id, key, publisher.plugin_ids)
        })
        .collect::<Result<Vec<_>, String>>()?;
    TrustStore::new(publishers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn release_policy_rejects_unsupported_or_unscoped_trust() {
        assert!(embedded_trust().is_ok());
        for policy in [
            json!({"schema_version":2,"publishers":[]}),
            json!({"schema_version":1,"publishers":[],"trust_from_package":true}),
            json!({"schema_version":1,"publishers":[{
                "key_id":"release","public_key":"ff".repeat(32),"plugin_ids":[]
            }]}),
            json!({"schema_version":1,"publishers":[{
                "key_id":"release","public_key":"not-a-key","plugin_ids":["org.example.demo"]
            }]}),
            json!({"schema_version":1,"publishers":[{
                "key_id":"release","public_key":"ff".repeat(32),"plugin_ids":["*"]
            }]}),
        ] {
            assert!(parse_policy(&policy.to_string()).is_err());
        }
    }
}

//! Build-time release identity, shared with unit tests. Never infer a channel from an OS version.
use serde_json::{json, Value};

pub fn release_identity(
    package_version: &str,
    source_sha: &str,
    plan_json: Option<&str>,
) -> Result<Value, String> {
    let package = semver::Version::parse(package_version).map_err(|e| e.to_string())?;
    let base = format!("{}.{}.{}", package.major, package.minor, package.patch);
    let Some(raw) = plan_json else {
        if !package.pre.is_empty() || !package.build.is_empty() {
            return Err("A candidate package requires .release-plan.json".into());
        }
        return Ok(json!({
            "schema_version": 1, "version": base, "base_version": base,
            "channel": "local", "source_sha": source_sha, "build_number": null
        }));
    };
    let plan: Value =
        serde_json::from_str(raw).map_err(|e| format!("Invalid release plan: {e}"))?;
    if plan["schema_version"] != 1 || plan["promotion"] != "final-build" {
        return Err("Unsupported release plan schema or promotion mode".into());
    }
    if source_sha.is_empty() || plan["source_sha"].as_str() != Some(source_sha) {
        return Err("Release plan source_sha does not match the checkout".into());
    }
    if plan["base_version"].as_str() != Some(&base) {
        return Err("Release plan base_version does not match Cargo".into());
    }
    let version = plan["version"]
        .as_str()
        .ok_or("Release plan version is missing")?;
    if version != package_version {
        return Err("Cargo version was not synchronized with the release plan".into());
    }
    let channel = plan["channel"]
        .as_str()
        .ok_or("Release plan channel is missing")?;
    let expected = match channel {
        "stable" => base.clone(),
        "nightly" => {
            let sequence = plan["sequence"]
                .as_str()
                .ok_or("Missing nightly sequence")?;
            let pieces: Vec<_> = sequence.split('.').collect();
            if pieces.len() != 3
                || pieces[0].len() != 14
                || pieces
                    .iter()
                    .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err("Invalid nightly sequence".into());
            }
            format!("{base}-nightly.{sequence}")
        }
        "beta" | "rc" => {
            let sequence = plan["sequence"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or("Release plan sequence must be positive")?;
            format!("{base}-{channel}.{sequence}")
        }
        _ => return Err("Unsupported release plan channel".into()),
    };
    if version != expected || plan["tag"].as_str() != Some(&format!("v{version}")) {
        return Err("Release plan version, tag and channel disagree".into());
    }
    if plan["build_number"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 99_990_000)
        .is_none()
    {
        return Err("Release plan build_number is outside the native version range".into());
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Value {
        json!({"schema_version": 1, "version": "2.5.42-beta.3", "base_version": "2.5.42",
            "channel": "beta", "sequence": 3, "tag": "v2.5.42-beta.3",
            "source_sha": "abc", "promotion": "final-build", "build_number": 2_005_043})
    }

    #[test]
    fn local_build_uses_base_without_claiming_a_release_channel() {
        let info = release_identity("2.5.42", "abc", None).unwrap();
        assert_eq!(info["version"], "2.5.42");
        assert_eq!(info["channel"], "local");
        assert!(release_identity("2.5.42-beta.3", "abc", None).is_err());
    }

    #[test]
    fn candidate_identity_preserves_the_complete_plan() {
        let plan = plan();
        assert_eq!(
            release_identity("2.5.42-beta.3", "abc", Some(&plan.to_string())).unwrap(),
            plan
        );
    }

    #[test]
    fn stale_or_inconsistent_plan_stops_the_build() {
        for (field, value) in [
            ("source_sha", json!("old")),
            ("base_version", json!("2.5.41")),
            ("channel", json!("rc")),
            ("tag", json!("v2.5.42")),
            ("schema_version", json!(2)),
            ("build_number", json!(0)),
        ] {
            let mut invalid = plan();
            invalid[field] = value;
            assert!(
                release_identity("2.5.42-beta.3", "abc", Some(&invalid.to_string())).is_err(),
                "{field}"
            );
        }
        assert!(release_identity("2.5.42", "abc", Some(&plan().to_string())).is_err());
        assert!(release_identity("2.5.42", "abc", Some("invalid")).is_err());
    }
}

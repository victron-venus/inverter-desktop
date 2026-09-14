use percent_encoding::percent_decode_str;
use url::Url;

pub const MAX_URL_BYTES: usize = 2048;

/// Parse the direct Frigate origin without credentials or URL normalization that
/// could change the configured path authority. This module performs no I/O.
pub fn base_url(value: Option<&str>) -> Result<Option<Url>, &'static str> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    if value.len() > MAX_URL_BYTES
        || value.trim() != value
        || value.chars().any(|ch| ch.is_control() || ch == '\\')
    {
        return Err("invalid Frigate base URL");
    }
    let url = Url::parse(value).map_err(|_| "invalid Frigate base URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid Frigate base URL");
    }
    let (_, authority_path) = value.split_once("://").ok_or("invalid Frigate base URL")?;
    let (authority, path) = authority_path
        .split_once('/')
        .unwrap_or((authority_path, ""));
    if authority.is_empty() || authority.contains('@') || !valid_base_path(path) {
        return Err("invalid Frigate base URL");
    }
    Ok(Some(url))
}

fn valid_base_path(path: &str) -> bool {
    path.split('/').all(|segment| {
        let lowered = segment.to_ascii_lowercase();
        if matches!(segment, "." | "..")
            || ["%2e", "%2f", "%5c", "%25"]
                .iter()
                .any(|encoded| lowered.contains(encoded))
        {
            return false;
        }
        percent_decode_str(segment)
            .decode_utf8()
            .is_ok_and(|decoded| !decoded.chars().any(|ch| ch.is_control() || ch == '%'))
    })
}

pub fn clip_url(base: &Url, event_id: &str) -> Option<String> {
    // Ordinary Frigate IDs remain unchanged. Ambiguous separators, percent
    // escapes and dot segments cannot become a request outside the base path.
    if event_id.is_empty()
        || event_id.len() > 128
        || matches!(event_id, "." | "..")
        || event_id
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | '%'))
    {
        return None;
    }
    let mut url = base.clone();
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(["api", "events", event_id, "clip.mp4"]);
    (url.as_str().len() <= MAX_URL_BYTES).then(|| url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_base_disables_clips_and_explicit_prefixes_and_ports_are_preserved() {
        for value in [None, Some(""), Some("   ")] {
            assert!(base_url(value).unwrap().is_none());
        }
        for (base, expected) in [
            (
                "http://frigate.local:5000",
                "http://frigate.local:5000/api/events/123-a/clip.mp4",
            ),
            (
                "https://frigate.local:8443/proxy/frigate/",
                "https://frigate.local:8443/proxy/frigate/api/events/123-a/clip.mp4",
            ),
            (
                "http://[::1]:5000/prefix",
                "http://[::1]:5000/prefix/api/events/123-a/clip.mp4",
            ),
            (
                "https://frigate.local/space%20prefix",
                "https://frigate.local/space%20prefix/api/events/123-a/clip.mp4",
            ),
        ] {
            assert_eq!(
                clip_url(&base_url(Some(base)).unwrap().unwrap(), "123-a").unwrap(),
                expected
            );
        }
    }

    #[test]
    fn event_id_is_one_encoded_segment_without_query_or_fragment_authority() {
        let base = base_url(Some("https://frigate.local/root"))
            .unwrap()
            .unwrap();
        let value = clip_url(&base, "é space?#").unwrap();
        assert_eq!(
            value,
            "https://frigate.local/root/api/events/%C3%A9%20space%3F%23/clip.mp4"
        );
        let parsed = Url::parse(&value).unwrap();
        assert_eq!(parsed.origin(), base.origin());
        assert!(parsed.query().is_none() && parsed.fragment().is_none());
        for id in [".", "..", "a/b", "a\\b", "a%2fb", "a\n", ""] {
            assert!(clip_url(&base, id).is_none(), "ambiguous event ID accepted");
        }
        let long = base_url(Some(&format!("https://frigate.local/{}", "a".repeat(2000))))
            .unwrap()
            .unwrap();
        assert!(clip_url(&long, &"x".repeat(128)).is_none());
    }

    #[test]
    fn invalid_or_ambiguous_bases_are_rejected_without_echoing_input() {
        for value in [
            "ftp://frigate.local",
            "https:///missing-authority",
            "https:frigate.local",
            "https://user:password@frigate.local",
            "https://@frigate.local",
            "https://frigate.local?token=x",
            "https://frigate.local/#fragment",
            " https://frigate.local",
            "https://frigate.local\n",
            "https://frigate.local\\other",
            "https://frigate.local/a/../b",
            "https://frigate.local/a/./b",
            "https://frigate.local/a%2fb",
            "https://frigate.local/a%2Eb",
            "https://frigate.local/%252e",
            "https://frigate.local/%5cb",
            "https://frigate.local/%00",
            "https://frigate.local/%0a",
            "https://frigate.local/%7f",
            "https://frigate.local/%ff",
            "https://frigate.local/%invalid",
        ] {
            assert_eq!(
                base_url(Some(value)).unwrap_err(),
                "invalid Frigate base URL"
            );
        }
        assert!(base_url(Some(&"x".repeat(MAX_URL_BYTES + 1))).is_err());
    }
}

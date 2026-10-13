//! Fuzz oracles call production libraries; no application, worker, socket or
//! credential store is started. Keep inputs and all derived work bounded.
use inverter_camera_common as camera;
use inverter_worker_protocol::{read_frame, HostFrame, MAX_FRAME_BYTES};
use serde_json::{json, Value};
use std::{collections::BTreeSet, io::BufReader};

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
}

fn frame_value(frame: HostFrame<Value>) -> Value {
    match frame {
        HostFrame::Hello {
            protocol_version,
            host_api_version,
            plugin_id,
        } => json!(["hello", protocol_version, host_api_version, plugin_id]),
        HostFrame::Configuration { configuration } => json!(["configuration", configuration]),
        HostFrame::Action {
            request_id,
            action_id,
            params,
            deadline_ms,
            ..
        } => {
            assert!(token(&request_id) && token(&action_id));
            assert!((1..=30_000).contains(&deadline_ms));
            assert!(params.is_object());
            assert!(serde_json::to_vec(&params).unwrap().len() <= 4096);
            // An iterative oracle checks resource bounds independently of the
            // production recursive validator. The timestamp is not serialized.
            let mut pending = vec![(&params, 0)];
            let mut nodes = 0;
            while let Some((value, depth)) = pending.pop() {
                nodes += 1;
                assert!(nodes <= 512 && depth <= 8);
                match value {
                    Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
                    Value::Object(values) => {
                        pending.extend(values.values().map(|v| (v, depth + 1)))
                    }
                    _ => (),
                }
            }
            json!(["action", request_id, action_id, params, deadline_ms])
        }
        HostFrame::Cancel { request_id } => {
            assert!(token(&request_id));
            json!(["cancel", request_id])
        }
        HostFrame::Shutdown {} => json!(["shutdown"]),
    }
}

fn decode(data: &[u8], capacity: usize) -> Result<Vec<Value>, &'static str> {
    let mut reader = BufReader::with_capacity(capacity, data);
    let mut values = Vec::new();
    // Multiple frames exercise newline boundaries without unbounded work.
    for _ in 0..3 {
        match read_frame(&mut reader)? {
            Some(frame) => values.push(frame_value(frame)),
            None => break,
        }
    }
    Ok(values)
}

pub fn worker_frames(data: &[u8]) {
    if data.len() > MAX_FRAME_BYTES + 1 {
        return;
    }
    // Pipe segmentation must never change accepted commands or error classes.
    let expected = decode(data, 4096);
    for capacity in [1, 7, 127] {
        assert_eq!(expected, decode(data, capacity));
    }
    // Also enter semantic command validation with syntactically valid JSON.
    let params = serde_json::from_slice::<Value>(data).unwrap_or_else(|_| json!({"input": data}));
    let mut command = serde_json::to_vec(&json!({
        "type": "action", "request_id": "fuzz-1", "action_id": "camera.refresh",
        "params": params, "deadline_ms": 1
    }))
    .unwrap();
    command.push(b'\n');
    let _ = decode(&command, 17);
}

pub fn camera_topics(data: &[u8]) {
    if data.len() > 32768 {
        return;
    }
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    for provider in ["ring", "kerberos"] {
        if let Ok(filters) = camera::topics(text, provider) {
            assert!(!filters.is_empty() && filters.len() <= 16);
            assert_eq!(filters.iter().collect::<BTreeSet<_>>().len(), filters.len());
            for filter in filters {
                assert!(filter.starts_with(&format!("{provider}/")));
                assert!(!filter.contains('#') && filter.len() <= 512);
                let topic = filter.replace('+', "camera1");
                assert!(camera::topic_matches(&filter, &topic));
                assert!(!camera::topic_matches(&filter, &format!("{topic}/extra")));
                assert!(!camera::topic_matches(&filter, &format!("other/{topic}")));
            }
        }
    }
    if let Ok(labels) = camera::labels(Some(text), camera::valid_identity) {
        assert!(labels.len() <= camera::MAX_CAMERAS);
        for (key, label) in labels {
            assert!(camera::valid_identity(&key));
            assert!(!label.trim().is_empty() && label.len() <= 96);
            assert!(!label.chars().any(char::is_control));
        }
    }
    if let Ok(excluded) = camera::exclusions::parse(Some(text), camera::valid_identity) {
        assert!(excluded.len() <= camera::MAX_CAMERAS);
        assert!(excluded.iter().all(|id| camera::valid_identity(id)));
    }
}

pub fn live_urls(data: &[u8]) {
    if data.len() > 32768 {
        return;
    }
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    const ENDPOINT: &str = "https://ha.invalid/api/camera_proxy_stream/camera.front";
    let configured =
        camera::live_urls::CameraLiveUrls::parse(Some(&json!({"front": ENDPOINT}).to_string()))
            .unwrap();
    // Mutations explore both arbitrary URLs and the token-bearing valid shape.
    for value in [text.to_owned(), format!("{ENDPOINT}?token={text}")] {
        if let Ok(url) = configured.resolve("front", &value) {
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some("ha.invalid"));
            assert_eq!(url.port(), None);
            assert_eq!(url.path(), "/api/camera_proxy_stream/camera.front");
            assert!(url.username().is_empty() && url.password().is_none());
            assert!(url.fragment().is_none());
            let query: Vec<_> = url.query_pairs().collect();
            assert_eq!(query.len(), 1);
            assert_eq!(query[0].0, "token");
            assert!(!query[0].1.is_empty() && query[0].1.len() <= 512);
        }
        assert!(configured.resolve("unconfigured", &value).is_err());
    }
    for mapping in [text.to_owned(), json!({"front": text}).to_string()] {
        if let Ok(urls) = camera::live_urls::CameraLiveUrls::parse(Some(&mapping)) {
            let topics: Vec<_> = urls.topics().collect();
            assert!(topics.len() <= 32);
            for topic in topics {
                let id = topic.strip_prefix(camera::live_urls::TOPIC_PREFIX).unwrap();
                assert!(camera::live_urls::camera_id(id));
            }
            // Diagnostic formatting must not leak configured endpoints.
            assert_eq!(
                format!("{urls:?}"),
                "CameraLiveUrls { endpoints: [redacted] }"
            );
        }
    }
    for allow_query in [false, true] {
        if let Ok(url) = camera::http_url(text, allow_query) {
            assert!(matches!(url.scheme(), "http" | "https"));
            assert!(url.host_str().is_some());
            assert!(url.username().is_empty() && url.password().is_none());
            assert!(url.fragment().is_none());
            assert!(allow_query || url.query().is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_in_corpus_and_protocol_boundaries() {
        for (target, run) in [
            ("worker_frames", worker_frames as fn(&[u8])),
            ("camera_topics", camera_topics as fn(&[u8])),
            ("live_urls", live_urls as fn(&[u8])),
        ] {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("corpus")
                .join(target);
            for seed in std::fs::read_dir(dir).unwrap() {
                run(&std::fs::read(seed.unwrap().path()).unwrap());
            }
            run(b"");
            run(&[0xff, 0x00, b'\n']);
        }
        worker_frames(&vec![b' '; MAX_FRAME_BYTES + 1]);
        let mut boundary = vec![b' '; MAX_FRAME_BYTES];
        boundary.push(b'\n');
        worker_frames(&boundary);
    }
}

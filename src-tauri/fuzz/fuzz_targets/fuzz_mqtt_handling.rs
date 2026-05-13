#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz MQTT topic and payload handling
    if data.len() < 2 {
        return;
    }

    let split_pos = data[0] as usize % data.len();
    let (topic_bytes, payload_bytes) = data.split_at(split_pos);

    if let Ok(topic) = std::str::from_utf8(topic_bytes) {
        // Simulate topic validation
        if topic.starts_with("inverter/") {
            // Simulate payload processing
            if let Ok(payload) = std::str::from_utf8(payload_bytes) {
                // Simulate JSON parsing
                let _ = serde_json::from_str::<serde_json::Value>(payload);
            }
        }
    }
});

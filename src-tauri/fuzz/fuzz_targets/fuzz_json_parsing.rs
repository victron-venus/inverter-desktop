#![no_main]

use libfuzzer_sys::fuzz_target;
use inverter_dashboard::mqtt::InverterState;

fuzz_target!(|data: &[u8]| {
    // Fuzz JSON parsing for InverterState
    if let Ok(json_str) = std::str::from_utf8(data) {
        let _ = serde_json::from_str::<InverterState>(json_str);
    }
});

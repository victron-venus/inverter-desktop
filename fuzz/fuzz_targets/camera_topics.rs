#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| inverter_desktop_fuzz::camera_topics(data));

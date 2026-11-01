#![no_main]
use caskade_discovery::registry::{Limits, RegistryHive, RegistryView, WineRegistry};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let limits = Limits {
        input_bytes: 65536,
        line_bytes: 8192,
        value_bytes: 4096,
        output_bytes: 65536,
        keys: 128,
        values: 256,
    };
    let _ = WineRegistry::parse(
        &text,
        RegistryHive::LocalMachine,
        RegistryView::Native,
        limits,
    );
});

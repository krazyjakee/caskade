#![no_main]
use caskade_discovery::parsers::{self, Limits};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let limits = Limits {
        max_input_bytes: 65536,
        max_string_bytes: 8192,
        max_nodes: 2048,
        max_depth: 32,
    };
    let _ = parsers::lnk::parse(data, &limits);
});

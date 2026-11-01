#![no_main]
use caskade_installer::formats::windows::nsis::{NsisLimits, inspect_bytes};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = inspect_bytes(data, NsisLimits::default());
});

#![no_main]
use caskade_installer::formats::windows::inno::{InnoLimits, inspect_bytes};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = inspect_bytes(data, InnoLimits::default());
});

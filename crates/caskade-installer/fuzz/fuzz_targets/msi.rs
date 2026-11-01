#![no_main]
use caskade_installer::formats::windows::msi::{MsiLimits, inspect_reader};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
fuzz_target!(|data: &[u8]| {
    let _ = inspect_reader(Cursor::new(data), MsiLimits::default());
});

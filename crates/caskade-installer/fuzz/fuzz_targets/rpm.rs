#![no_main]
use caskade_installer::formats::linux::rpm::inspect;
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
fuzz_target!(|data: &[u8]| {
    let _ = inspect(&mut Cursor::new(data));
});

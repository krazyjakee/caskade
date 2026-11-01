#![no_main]
use caskade_installer::formats::linux::deb::{ar_members, inspect, parse_control};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
// ar member table, whole .deb inspection (control/data tar headers) and the
// control-file parser.
fuzz_target!(|data: &[u8]| {
    let _ = ar_members(&mut Cursor::new(data));
    let _ = inspect(&mut Cursor::new(data));
    let _ = parse_control(data);
});

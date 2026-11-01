#![no_main]
use caskade_installer::formats::windows::burn::{inspect, parse_manifest};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
// Whole bundles (PE .wixburn section, attached containers) and raw
// BurnManifest XML.
fuzz_target!(|data: &[u8]| {
    let _ = inspect(&mut Cursor::new(data));
    let _ = parse_manifest(data);
});

#![no_main]
use caskade_installer::formats::macos::pkg::{XarLimits, inspect, parse_toc};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
// Whole XAR packages (header, zlib TOC, PackageInfo/Distribution) and raw
// TOC XML against a heap of plausible size.
fuzz_target!(|data: &[u8]| {
    let _ = inspect(&mut Cursor::new(data), XarLimits::default());
    if let Ok(toc) = parse_toc(data, 1 << 20) {
        for entry in &toc.entries {
            assert!(entry.offset.checked_add(entry.length).is_some_and(|end| end <= 1 << 20));
        }
    }
});

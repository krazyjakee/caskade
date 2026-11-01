#![no_main]
use caskade_installer::formats::linux::appimage::{AppImageLimits, read_metadata};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
// Whole images are fuzzed as given; anything else is treated as the squashfs
// behind a minimal type 2 runtime header whose section table ends at byte 64.
fuzz_target!(|data: &[u8]| {
    let limits = AppImageLimits {
        max_entries: 4096,
        max_metadata_bytes: 64 * 1024,
        max_icon_bytes: 256 * 1024,
    };
    if data.starts_with(b"\x7fELF") {
        let _ = read_metadata(Cursor::new(data), limits);
        return;
    }
    let mut image = vec![0u8; 64];
    image[..4].copy_from_slice(b"\x7fELF");
    image[4] = 2;
    image[5] = 1;
    image[8..11].copy_from_slice(b"AI\x02");
    image[0x28..0x30].copy_from_slice(&64u64.to_le_bytes());
    image.extend_from_slice(data);
    let _ = read_metadata(Cursor::new(image), limits);
});

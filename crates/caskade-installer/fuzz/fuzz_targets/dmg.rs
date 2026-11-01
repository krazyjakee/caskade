#![no_main]
use caskade_installer::formats::macos::dmg::{parse_resource_plist, parse_trailer};
use libfuzzer_sys::fuzz_target;
// UDIF metadata only: the last 512 bytes as the koly trailer (claimed file
// size from the first 8 bytes) and the rest as the resource plist.
fuzz_target!(|data: &[u8]| {
    if data.len() >= 520 {
        let size = u64::from_le_bytes(data[..8].try_into().unwrap());
        let _ = parse_trailer(&data[data.len() - 512..], size);
        let _ = parse_trailer(&data[data.len() - 512..], data.len() as u64);
    }
    let _ = parse_resource_plist(data);
});

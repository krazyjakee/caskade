#![no_main]
use caskade_installer::formats::linux::makeself::parse_header;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let size = data.len() as u64;
    if let Ok(header) = parse_header(data, size) {
        for span in header.spans {
            assert!(span.offset.checked_add(span.length).is_some_and(|end| end <= size));
        }
    }
});

#![no_main]
use caskade_installer::{
    Format,
    formats::archive::{self, ArchiveOptions},
};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
fuzz_target!(|data: &[u8]| {
    let options = ArchiveOptions {
        max_metadata_bytes: 512 * 1024,
        max_entries: 1024,
        max_decoder_bytes: 8 * 1024 * 1024,
        max_unpacked_bytes: 8 * 1024 * 1024,
        max_expansion_ratio: 1000,
        max_link_bytes: 4096,
        ..ArchiveOptions::default()
    };
    // No selector byte: fixture seeds go unchanged into every parser, including
    // when mutations destroy their magic. No extractor or sidecar is invoked.
    for format in [
        Format::Zip,
        Format::SevenZip,
        Format::Tar,
        Format::Gzip,
        Format::Xz,
        Format::Zstd,
        Format::Bzip2,
    ] {
        let _ = archive::list(Cursor::new(data), format, &options);
    }
});

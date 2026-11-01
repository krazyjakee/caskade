#![cfg(feature = "windows-formats")]
use caskade_installer::formats::windows::inno::{InnoLimits, inspect_bytes};

#[test]
fn ambiguous_version_oversized_wizard_image_does_not_allocate() {
    let bytes = include_bytes!("regressions/26-inno-oversized-allocation.exe");
    // A wrong historical-version interpretation reaches the enormous image
    // length. Reject that interpretation before allocation, then retain the
    // valid alternate interpretation used by this actual sliced installer.
    let metadata = inspect_bytes(bytes, InnoLimits::default()).unwrap();
    assert_eq!(metadata.setup_version, "5.6.0");
    assert_eq!(metadata.app_name.as_deref(), Some("Caskade Fixture Game"));
    assert!(metadata.external_slices);
    assert_eq!(metadata.files.len(), 5);
}

#[test]
fn oversized_pascal_string_is_rejected_before_reading_or_allocating() {
    let error = inno::string::PascalString::read_sized(std::io::empty(), u32::MAX).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
}

fn metadata_block(raw: &[u8]) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut crc = flate2::Crc::new();
        crc.update(bytes);
        crc.sum()
    }
    let mut framed = vec![];
    for chunk in raw.chunks(4096) {
        framed.extend(crc(chunk).to_le_bytes());
        framed.extend(chunk);
    }
    let mut header = (framed.len() as u32).to_le_bytes().to_vec();
    header.push(1);
    let mut block = crc(&header).to_le_bytes().to_vec();
    block.extend(header);
    block.extend(framed);
    block
}
fn version(major: u8, minor: u8) -> innospect::Version {
    innospect::Version {
        a: major,
        b: minor,
        c: 9,
        d: 0,
        flags: innospect::VersionFlags::empty(),
        raw_marker: [0; 64],
    }
}

#[test]
fn fallback_rejects_an_oversized_lzma_dictionary_before_decoder_allocation() {
    let mut raw = vec![0x5d];
    raw.extend(u32::MAX.to_le_bytes());
    raw.extend([0; 5]);
    let error =
        innospect::decompress::block::decompress_block(&metadata_block(&raw), 0, &version(6, 4))
            .unwrap_err();
    assert!(matches!(
        error,
        innospect::Error::ResourceLimit {
            what: "LZMA1 dictionary"
        }
    ));
}

#[test]
fn fallback_bounds_decoded_metadata_even_with_small_compressed_input() {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::fast());
    for _ in 0..1024 {
        encoder.write_all(&[0; 65536]).unwrap();
    }
    encoder.write_all(&[0]).unwrap();
    let compressed = encoder.finish().unwrap();
    assert!(compressed.len() < 1024 * 1024);
    // Inno 4.0.9 metadata uses zlib rather than LZMA1.
    let error = innospect::decompress::block::decompress_block(
        &metadata_block(&compressed),
        0,
        &version(4, 0),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        innospect::Error::ResourceLimit {
            what: "decoded stream"
        }
    ));
}

#[test]
fn hostile_pe_data_directory_count_is_rejected_before_allocation() {
    let bytes = include_bytes!("regressions/26-inno-pe-data-directory-allocation.exe");
    // Real probe-fuzzer input previously attempted a 17,805,165,888-byte Vec.
    let mut input = std::io::Cursor::new(bytes);
    let _result = caskade_installer::probe::ProbeEngine::with_builtin_formats(Default::default())
        .probe_reader(&mut input, std::path::Path::new("hostile.exe"));
    let error = inspect_bytes(bytes, InnoLimits::default()).unwrap_err();
    assert!(
        matches!(
            &error,
            caskade_installer::formats::windows::inno::InnoError::Limit(_)
        ),
        "{error:?}"
    );
}
#[test]
fn pe_directory_budget_accepts_the_standard_boundary_and_rejects_more() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/windows/inno60-basic/setup.exe");
    let mut bytes = std::fs::read(path).unwrap();
    let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    let optional = pe + 24;
    let magic = u16::from_le_bytes(bytes[optional..optional + 2].try_into().unwrap());
    let count_at = optional + if magic == 0x20b { 108 } else { 92 };
    bytes[count_at..count_at + 4].copy_from_slice(&16u32.to_le_bytes());
    inspect_bytes(&bytes, InnoLimits::default()).unwrap();
    for count in [17u32, u32::MAX] {
        bytes[count_at..count_at + 4].copy_from_slice(&count.to_le_bytes());
        let result = inspect_bytes(&bytes, InnoLimits::default());
        assert!(
            matches!(
                &result,
                Err(caskade_installer::formats::windows::inno::InnoError::Limit(
                    _
                ))
            ),
            "{result:?}"
        );
    }
}

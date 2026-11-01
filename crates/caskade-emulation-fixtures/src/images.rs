//! Real container formats with original dummy payloads, generated in memory.
use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader, compression::Compressor};
use std::{
    io::{self, Cursor, Write},
    path::Path,
};

/// A minimal ELF-identification header plus genuine SquashFS metadata. This is
/// a static-probe fixture, deliberately not an executable AppImage runtime.
pub(crate) fn appimage(path: &Path, name: &str, id: &str) -> io::Result<()> {
    let mut fs = FilesystemWriter::default();
    fs.set_time(1_700_000_000);
    fs.set_compressor(FilesystemCompressor::new(Compressor::Gzip, None).map_err(io::Error::other)?);
    fs.set_no_padding();
    fs.push_file(
        Cursor::new(format!("[Desktop Entry]\nType=Application\nName={name}\nExec=fixture-emulator\nCategories=Game;Emulator;\n")),
        "fixture.desktop", NodeHeader::default(),
    ).map_err(io::Error::other)?;
    fs.push_file(
        Cursor::new(format!(
            "<component type=\"desktop-application\"><id>{id}</id><name>{name}</name></component>"
        )),
        "fixture.metainfo.xml",
        NodeHeader::default(),
    )
    .map_err(io::Error::other)?;
    let mut image = Cursor::new(vec![]);
    fs.write(&mut image).map_err(io::Error::other)?;
    let mut elf = vec![0u8; 64];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    elf[8..11].copy_from_slice(b"AI\x02");
    elf[0x28..0x30].copy_from_slice(&64u64.to_le_bytes());
    elf[0x34..0x36].copy_from_slice(&64u16.to_le_bytes());
    elf.extend_from_slice(&image.into_inner());
    crate::executable(path, elf)
}

pub(crate) fn zip(path: &Path, entries: &[(&str, &[u8])]) -> io::Result<()> {
    let mut archive = zip::ZipWriter::new(Cursor::new(vec![]));
    for (name, bytes) in entries {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .map_err(io::Error::other)?;
        archive.write_all(bytes)?;
    }
    crate::write(
        path,
        archive.finish().map_err(io::Error::other)?.into_inner(),
    )
}

pub(crate) fn seven_zip(path: &Path, name: &str) -> io::Result<()> {
    let mut archive =
        sevenz_rust2::ArchiveWriter::new(Cursor::new(vec![])).map_err(io::Error::other)?;
    archive
        .push_archive_entry(
            sevenz_rust2::ArchiveEntry {
                name: name.into(),
                has_stream: true,
                ..Default::default()
            },
            Some(Cursor::new(b"Caskade original synthetic ROM payload\n")),
        )
        .map_err(io::Error::other)?;
    crate::write(path, archive.finish()?.into_inner())
}

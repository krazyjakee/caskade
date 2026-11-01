use std::path::Path;

use super::{Detection, Format, PayloadPlatform, ProbeError, ProbeSource, Role};

pub(super) fn detect(source: &mut ProbeSource<'_>) -> Result<Detection, ProbeError> {
    // This fixed-size header plus bounded trailer/offset reads keeps sparse and
    // very large inputs cheap. No read scales with the file's declared size.
    let header = source.read_at(0, 4096)?;
    let (format, platform) = if header.starts_with(b"PK\x03\x04")
        || header.starts_with(b"PK\x05\x06")
        || header.starts_with(b"PK\x07\x08")
    {
        (Format::Zip, PayloadPlatform::Unknown)
    } else if header.starts_with(b"7z\xbc\xaf\x27\x1c") {
        (Format::SevenZip, PayloadPlatform::Unknown)
    } else if header.starts_with(b"Rar!\x1a\x07\x00") || header.starts_with(b"Rar!\x1a\x07\x01\x00")
    {
        (Format::Rar, PayloadPlatform::Unknown)
    } else if header.starts_with(b"\xfd7zXZ\x00") {
        (Format::Xz, PayloadPlatform::Unknown)
    } else if header.starts_with(b"\x1f\x8b\x08") {
        (Format::Gzip, PayloadPlatform::Unknown)
    } else if header.starts_with(b"\x28\xb5\x2f\xfd") {
        (Format::Zstd, PayloadPlatform::Unknown)
    } else if header.starts_with(b"BZh") {
        (Format::Bzip2, PayloadPlatform::Unknown)
    } else if header.get(257..262) == Some(b"ustar") {
        (Format::Tar, PayloadPlatform::Unknown)
    } else if header.starts_with(b"!<arch>\n")
        && header.windows(13).any(|window| window == b"debian-binary")
    {
        (Format::Deb, PayloadPlatform::Linux)
    } else if header.starts_with(b"\xed\xab\xee\xdb") {
        (Format::Rpm, PayloadPlatform::Linux)
    } else if header.starts_with(b"xar!") {
        (Format::Pkg, PayloadPlatform::Macos)
    } else if header.starts_with(b"MSCF") {
        (Format::Cab, PayloadPlatform::Unknown)
    } else if header.starts_with(b"MSWIM\0\0\0") {
        (Format::Wim, PayloadPlatform::Windows)
    } else if header.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
        // CFB also contains Office documents: leave MSI classification to a
        // table-aware format probe rather than guessing from the CFB wrapper.
        let mut detection = Detection::new(Format::Unknown, PayloadPlatform::Unknown, false);
        detection
            .facts
            .insert("container.kind".into(), "cfb".into());
        return Ok(detection);
    } else if header.starts_with(b"\x7fELF") {
        let format = if header.get(8..11) == Some(b"AI\x02") {
            Format::AppImage
        } else {
            Format::Elf
        };
        (format, PayloadPlatform::Linux)
    } else if header.starts_with(b"MZ") && valid_pe(source, &header)? {
        (Format::Pe, PayloadPlatform::Windows)
    } else if [
        b"\xfe\xed\xfa\xce",
        b"\xce\xfa\xed\xfe",
        b"\xfe\xed\xfa\xcf",
        b"\xcf\xfa\xed\xfe",
        b"\xca\xfe\xba\xbe",
        b"\xbe\xba\xfe\xca",
        b"\xca\xfe\xba\xbf",
        b"\xbf\xba\xfe\xca",
    ]
    .iter()
    .any(|magic| header.starts_with(*magic))
    {
        (Format::MachO, PayloadPlatform::Macos)
    } else if header.starts_with(b"#!") {
        (Format::Script, PayloadPlatform::Linux)
    } else if header.starts_with(b"MComprHD") {
        (Format::Chd, PayloadPlatform::Unknown)
    } else if header.starts_with(b"NES\x1a") {
        (
            Format::Rom,
            PayloadPlatform::ConsoleRom {
                platform: Some("nes".into()),
            },
        )
    } else if source.size() >= 512 && source.read_at(source.size() - 512, 4)? == b"koly" {
        (Format::Dmg, PayloadPlatform::Macos)
    } else if source.size() >= 0x8006 && source.read_at(0x8001, 5)? == b"CD001" {
        (Format::Iso, PayloadPlatform::Unknown)
    } else {
        let mut detection = Detection::new(Format::Unknown, PayloadPlatform::Unknown, false);
        if let Some(kind) = infer::get(&header) {
            let format = match kind.extension() {
                "zip" => Format::Zip,
                "7z" => Format::SevenZip,
                "tar" => Format::Tar,
                "gz" => Format::Gzip,
                "bz2" => Format::Bzip2,
                "xz" => Format::Xz,
                "zst" => Format::Zstd,
                "rar" => Format::Rar,
                "cab" => Format::Cab,
                _ => Format::Unknown,
            };
            detection.format = format;
            detection.container = is_container(format);
            detection
                .facts
                .insert("magic.mime".into(), kind.mime_type().into());
            detection
                .facts
                .insert("magic.extension".into(), kind.extension().into());
        }
        return Ok(detection);
    };
    Ok(Detection::new(format, platform, is_container(format)))
}

fn valid_pe(source: &mut ProbeSource<'_>, header: &[u8]) -> Result<bool, ProbeError> {
    let Some(offset) = header.get(60..64) else {
        return Ok(false);
    };
    let offset = u32::from_le_bytes(offset.try_into().expect("four-byte slice")) as u64;
    if offset < 64 || offset.saturating_add(4) > source.size() {
        return Ok(false);
    }
    if let Some(signature) = header.get(offset as usize..).and_then(|tail| tail.get(..4)) {
        return Ok(signature == b"PE\0\0");
    }
    Ok(source.read_at(offset, 4)? == b"PE\0\0")
}

pub(super) fn is_container(format: Format) -> bool {
    matches!(
        format,
        Format::Directory
            | Format::Zip
            | Format::SevenZip
            | Format::Tar
            | Format::Gzip
            | Format::Xz
            | Format::Zstd
            | Format::Bzip2
            | Format::Rar
            | Format::Cab
            | Format::Wim
            | Format::Inno
            | Format::Nsis
            | Format::Msi
            | Format::Burn
            | Format::InstallShield
            | Format::Wise
            | Format::SetupFactory
            | Format::Makeself
            | Format::MojoSetup
            | Format::Deb
            | Format::Rpm
            | Format::Flatpak
            | Format::AppBundle
            | Format::Dmg
            | Format::Pkg
            | Format::Iso
            | Format::Chd
    )
}

pub(super) fn guess_role(name: &Path, format: Format) -> Role {
    let name = name
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if name.starts_with("unins") || name.starts_with("uninstall") {
        return Role::Uninstaller;
    }
    if ["vcredist", "vc_redist", "dxsetup", "dotnet", "directx"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return Role::Redistributable;
    }
    if name.contains("launcher") {
        return Role::Launcher;
    }
    if name.ends_with(".txt")
        || name.ends_with(".md")
        || name.ends_with(".pdf")
        || name.starts_with("readme")
    {
        return Role::Document;
    }
    if name.starts_with("setup")
        || name.starts_with("install")
        || matches!(
            format,
            Format::Inno
                | Format::Nsis
                | Format::Msi
                | Format::Burn
                | Format::InstallShield
                | Format::Wise
                | Format::SetupFactory
                | Format::Makeself
                | Format::MojoSetup
                | Format::Deb
                | Format::Rpm
                | Format::Flatpak
                | Format::Pkg
        )
    {
        return Role::Installer;
    }
    if matches!(
        format,
        Format::Pe
            | Format::Elf
            | Format::MachO
            | Format::AppImage
            | Format::AppBundle
            | Format::Script
    ) {
        return Role::Application;
    }
    if matches!(
        format,
        Format::Rom | Format::Iso | Format::Cue | Format::Bin | Format::Chd
    ) {
        return Role::Data;
    }
    Role::Unknown
}

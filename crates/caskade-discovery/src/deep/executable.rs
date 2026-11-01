use super::control::Control;
use crate::{OperatingSystem, SourceError};
use serde::Deserialize;
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::OnceLock,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Windows,
    Native,
    AppImage,
}
#[derive(Deserialize)]
struct Deny {
    exclude_prefixes: Vec<String>,
    exclude_contains: Vec<String>,
    exclude_directories: Vec<String>,
    lower_contains: Vec<String>,
}
fn deny() -> &'static Deny {
    static DENY: OnceLock<Deny> = OnceLock::new();
    DENY.get_or_init(|| {
        toml::from_str(include_str!(
            "../../../caskade-installer/data/exe-denylist.toml"
        ))
        .expect("embedded deny-list checked by installer")
    })
}
pub(super) fn excluded(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let deny = deny();
    deny.exclude_prefixes.iter().any(|p| stem.starts_with(p))
        || deny.exclude_contains.iter().any(|p| stem.contains(p))
        || path.components().any(|part| {
            deny.exclude_directories
                .iter()
                .any(|p| part.as_os_str().to_string_lossy().eq_ignore_ascii_case(p))
        })
}
pub(super) fn rank(path: &Path, title: &str) -> i32 {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let clean = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let mut score = 100 - (path.components().count() as i32).min(20) * 3;
    if clean(&stem) == clean(title) {
        score += 50;
    }
    if stem.ends_with("-shipping") {
        score += 20;
    }
    if matches!(
        path.file_name().and_then(|s| s.to_str()),
        Some("start.sh" | "run.sh")
    ) {
        score += 30;
    }
    if deny().lower_contains.iter().any(|p| stem.contains(p)) {
        score -= 80;
    }
    score
}
pub(super) fn inspect(
    dir: &cap_std::fs::Dir,
    path: &Path,
    control: &Control<'_>,
) -> Result<Option<Kind>, SourceError> {
    control.check()?;
    if excluded(path) {
        return Ok(None);
    }
    let metadata = dir.metadata(path).map_err(|e| SourceError::io(path, e))?;
    if metadata.len() < 4 {
        return Ok(None);
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let lower = name.to_lowercase();
    #[cfg(unix)]
    let exec = {
        use cap_std::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let exec = matches!(
        control.env.os(),
        OperatingSystem::Linux | OperatingSystem::Macos
    );
    if !lower.ends_with(".exe") && !lower.ends_with(".appimage") && !exec {
        return Ok(None);
    }
    let mut file = control.reader(dir.open(path).map_err(|e| SourceError::io(path, e))?);
    let mut header = [0u8; 64];
    let length = file
        .read(&mut header)
        .map_err(|e| SourceError::io(path, e))?;
    if length >= 64 && header.starts_with(b"MZ") && lower.ends_with(".exe") {
        let offset = u32::from_le_bytes(header[60..64].try_into().expect("64-byte header")) as u64;
        if !(64..=128 * 1024).contains(&offset) {
            return Ok(None);
        }
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| SourceError::io(path, e))?;
        let mut pe = [0; 6];
        if file.read_exact(&mut pe).is_err() || &pe[..4] != b"PE\0\0" {
            control.check()?;
            return Ok(None);
        }
        let arch = match u16::from_le_bytes([pe[4], pe[5]]) {
            0x14c => "x86",
            0x8664 => "x86_64",
            0xaa64 => "aarch64",
            _ => return Ok(None),
        };
        return Ok(matches_arch(control, arch).then_some(Kind::Windows));
    }
    if length >= 20 && header.starts_with(b"\x7fELF") && control.env.os() == OperatingSystem::Linux
    {
        let number = |a, b| {
            if header[5] == 2 {
                u16::from_be_bytes([a, b])
            } else {
                u16::from_le_bytes([a, b])
            }
        };
        if !matches!(header[4], 1 | 2)
            || !matches!(header[5], 1 | 2)
            || !matches!(number(header[16], header[17]), 2 | 3)
        {
            return Ok(None);
        }
        let arch = match number(header[18], header[19]) {
            3 => "x86",
            62 => "x86_64",
            183 => "aarch64",
            _ => return Ok(None),
        };
        if !matches_arch(control, arch) {
            return Ok(None);
        }
        if header[8..11] == *b"AI\x02" {
            return Ok(Some(Kind::AppImage));
        }
        // ET_DYN shared libraries are not game executables merely for having
        // executable bits; ordinary ELF entry points must be non-zero.
        if header[24..32].iter().all(|&b| b == 0) || lower.contains(".so") {
            return Ok(None);
        }
        return Ok(Some(Kind::Native));
    }
    if length >= 4
        && control.env.os() == OperatingSystem::Macos
        && matches!(
            &header[..4],
            b"\xcf\xfa\xed\xfe" | b"\xce\xfa\xed\xfe" | b"\xfe\xed\xfa\xcf" | b"\xfe\xed\xfa\xce"
        )
    {
        return Ok(Some(Kind::Native));
    }
    if header.starts_with(b"#!")
        && exec
        && control.env.os() != OperatingSystem::Windows
        && matches!(name.as_ref(), "start.sh" | "run.sh")
    {
        return Ok(Some(Kind::Native));
    }
    Ok(None)
}
fn matches_arch(control: &Control<'_>, arch: &str) -> bool {
    control
        .options
        .arch
        .as_deref()
        .is_none_or(|wanted| wanted == arch)
}

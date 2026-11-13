//! RetroAchievements' ROM hashes, for the systems whose rules are simple
//! enough to follow exactly (rcheevos' `rc_hash`, documented at
//! <https://docs.retroachievements.org/developer-docs/game-identification.html>):
//! the MD5 of the whole file, of the file without its copier or emulator
//! header, of an N64 ROM in big-endian byte order, or of an arcade set's
//! file name. Discs and the rest are hashed in ways Caskade doesn't compute;
//! those releases are linked by hand.

use md5::{Digest, Md5};
use std::path::Path;

/// The largest ROM file hashed: bigger isn't a cartridge.
const MAX_ROM: u64 = 256 * 1024 * 1024;

/// How RetroAchievements identifies a system's games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The MD5 of the whole file.
    Whole,
    /// NES and Famicom Disk System: without a 16-byte iNES/FDS header.
    Nes,
    /// SNES: without a 512-byte copier header.
    Snes,
    /// PC Engine: without a 512-byte header.
    PcEngine,
    /// Atari Lynx: without a 64-byte `LYNX` header.
    Lynx,
    /// Atari 7800: without a 128-byte `ATARI7800` header.
    Atari7800,
    /// Nintendo 64: in big-endian (`.z64`) byte order.
    N64,
    /// Arcade: the MD5 of the set's file name, without its extension.
    Arcade,
    /// Hashed from a disc's or a package's contents in a way Caskade
    /// doesn't compute.
    Unsupported,
}

/// A Caskade platform's RetroAchievements console id and hash rule; `None`
/// for a platform RetroAchievements doesn't have.
pub fn console(platform_id: &str) -> Option<(u32, Rule)> {
    use Rule::*;
    Some(match platform_id {
        "sega_genesis" => (1, Whole),
        "nintendo_n64" => (2, N64),
        "nintendo_snes" => (3, Snes),
        "nintendo_gb" => (4, Whole),
        "nintendo_gba" => (5, Whole),
        "nintendo_gbc" => (6, Whole),
        "nintendo_nes" | "famicom" | "nintendo_famicom_disk" => (7, Nes),
        "nec_turbografx_16" | "nec_supergrafx" => (8, PcEngine),
        "sega_cd" => (9, Unsupported),
        "sega_32x" => (10, Whole),
        "sega_mastersystem" => (11, Whole),
        "sony_playstation" => (12, Unsupported),
        "atari_lynx" => (13, Lynx),
        "snk_neogeopocket" | "snk_neogeopocket_color" => (14, Whole),
        "sega_gamegear" => (15, Whole),
        "nintendo_gamecube" => (16, Unsupported),
        "atari_jaguar" => (17, Whole),
        "nintendo_ds" => (18, Unsupported),
        "sony_playstation2" => (21, Unsupported),
        "magnavox_odyssey_2" => (23, Whole),
        "pokemon_mini" => (24, Whole),
        "atari_2600" => (25, Whole),
        "arcade" => (27, Arcade),
        "nintendo_virtualboy" => (28, Whole),
        "microsoft_msx" | "microsoft_msx2" => (29, Whole),
        "sega_sg1000" => (33, Whole),
        "sega_saturn" => (39, Unsupported),
        "sega_dreamcast" => (40, Unsupported),
        "sony_psp" => (41, Unsupported),
        "3do" => (43, Unsupported),
        "coleco_vision" => (44, Whole),
        "mattel_intellivision" => (45, Whole),
        "vectrex" => (46, Whole),
        "nec_pcfx" => (49, Unsupported),
        "atari_7800" => (51, Atari7800),
        "bandai_wonderswan" | "bandai_wonderswan_color" => (53, Whole),
        "snk_neogeo_cd" => (56, Unsupported),
        "fairchild_channelf" => (57, Whole),
        "watara_supervision" | "supervision" => (63, Whole),
        "megaduck" | "mega_duck_slash_cougar_boy" => (69, Whole),
        "arduboy" => (71, Whole),
        "wasm4" => (72, Whole),
        "arcadia_2001" => (73, Whole),
        "interton_vc_4000" => (74, Whole),
        "elektor" => (75, Whole),
        "nec_turbografx_cd" => (76, Unsupported),
        "atari_jaguar_cd" => (77, Unsupported),
        "nintendo_dsi" => (78, Unsupported),
        "uzebox" => (80, Whole),
        _ => return None,
    })
}

/// Why a ROM couldn't be hashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashError {
    /// The system is hashed in a way Caskade doesn't compute.
    UnsupportedSystem,
    /// A compressed or archived file: RetroAchievements hashes what is
    /// inside, and Caskade doesn't unpack it.
    Archive,
    TooLarge,
    Read(String),
}

const ARCHIVES: [&str; 5] = ["zip", "7z", "rar", "gz", "chd"];

/// The RetroAchievements hash of the ROM at `path` under `rule`.
pub async fn hash_file(path: &Path, rule: Rule) -> Result<String, HashError> {
    if rule == Rule::Unsupported {
        return Err(HashError::UnsupportedSystem);
    }
    if rule == Rule::Arcade {
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| HashError::Read("no file name".into()))?;
        return Ok(format!("{:x}", Md5::digest(name.as_bytes())));
    }
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ARCHIVES
                .iter()
                .any(|archive| ext.eq_ignore_ascii_case(archive))
        })
    {
        return Err(HashError::Archive);
    }
    let size = tokio::fs::metadata(path)
        .await
        .map_err(|error| HashError::Read(error.to_string()))?
        .len();
    if size > MAX_ROM {
        return Err(HashError::TooLarge);
    }
    let path = path.to_path_buf();
    caskade_jobs::blocking("Hashing a ROM", move || {
        let bytes = std::fs::read(path).map_err(|error| HashError::Read(error.to_string()))?;
        hash_bytes(&bytes, rule)
    })
    .await
    .map_err(|error| HashError::Read(error.to_string()))?
}

/// The RetroAchievements hash of a ROM's bytes under `rule`.
pub fn hash_bytes(bytes: &[u8], rule: Rule) -> Result<String, HashError> {
    let body: std::borrow::Cow<'_, [u8]> = match rule {
        Rule::Whole => bytes.into(),
        Rule::Nes if bytes.starts_with(b"NES\x1a") || bytes.starts_with(b"FDS\x1a") => {
            bytes.get(16..).unwrap_or_default().into()
        }
        Rule::Nes => bytes.into(),
        Rule::Snes | Rule::PcEngine => {
            let block = if rule == Rule::Snes { 0x2000 } else { 0x20000 };
            if bytes.len() % block == 512 {
                bytes[512..].into()
            } else {
                bytes.into()
            }
        }
        Rule::Lynx if bytes.starts_with(b"LYNX\0") => bytes.get(64..).unwrap_or_default().into(),
        Rule::Lynx => bytes.into(),
        Rule::Atari7800 if bytes.get(1..10) == Some(b"ATARI7800") => {
            bytes.get(128..).unwrap_or_default().into()
        }
        Rule::Atari7800 => bytes.into(),
        Rule::N64 => n64_big_endian(bytes).into(),
        Rule::Arcade | Rule::Unsupported => return Err(HashError::UnsupportedSystem),
    };
    Ok(format!("{:x}", Md5::digest(&body)))
}

/// An N64 ROM in big-endian order: `.z64` as it is, `.v64` (byte-swapped
/// pairs) and `.n64` (little-endian words) turned around.
fn n64_big_endian(bytes: &[u8]) -> Vec<u8> {
    match bytes.get(..4) {
        Some([0x37, 0x80, 0x40, 0x12]) => bytes
            .chunks(2)
            .flat_map(|pair| pair.iter().rev().copied())
            .collect(),
        Some([0x40, 0x12, 0x37, 0x80]) => bytes
            .chunks(4)
            .flat_map(|word| word.iter().rev().copied())
            .collect(),
        _ => bytes.to_vec(),
    }
}

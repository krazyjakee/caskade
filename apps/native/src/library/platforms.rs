//! How the library shows a platform (#99, the webview's
//! `views/library/platforms/catalog.ts`): a short title, what kind of
//! machine it is and its hardware render, keyed by the emulation
//! catalogue's platform ids (`crates/caskade-sources/emulation/platforms.toml`)
//! and found by any of its names. The renders are the webview's own files,
//! built in and served as `caskade-asset://platforms/<id>.webp`
//! ([`fetch_art`]).

use caskade_core::model::{GameSort, PlatformSummary};
use caskade_ui_i18n::tp;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One platform's look.
pub struct Look {
    pub id: &'static str,
    pub title: &'static str,
    pub maker: &'static str,
    /// In English; [`kind_label`] translates it where it's shown.
    pub kind: &'static str,
    /// Other names the platform goes by: the catalogue's own, regional ones.
    pub names: &'static [&'static str],
    /// The hardware render (lossless WebP, 480 × 320).
    pub art: Option<&'static [u8]>,
}

/// The neutral platform hardware artwork, for a platform without its own.
const GENERIC_ART: &[u8] =
    include_bytes!("../../../desktop/src/views/library/platforms/art/generic.webp");

/// The host `caskade-asset://` serves the renders under.
pub const ART_HOST: &str = "platforms";

pub const LOOKS: &[Look] = &[
    Look {
        id: "nintendo_nes",
        title: "NES",
        maker: "Nintendo",
        kind: "8-bit console",
        names: &[
            "Nintendo Entertainment System",
            "Famicom",
            "Nintendo Famicom",
        ],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_nes.webp"
        )),
    },
    Look {
        id: "nintendo_snes",
        title: "SNES",
        maker: "Nintendo",
        kind: "16-bit console",
        names: &[
            "Super Nintendo Entertainment System",
            "Super Nintendo",
            "Super Famicom",
        ],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_snes.webp"
        )),
    },
    Look {
        id: "nintendo_n64",
        title: "Nintendo 64",
        maker: "Nintendo",
        kind: "64-bit console",
        names: &["N64"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_n64.webp"
        )),
    },
    Look {
        id: "nintendo_gb",
        title: "Game Boy",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["Nintendo Game Boy", "GB"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_gb.webp"
        )),
    },
    Look {
        id: "nintendo_gbc",
        title: "Game Boy Color",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["Nintendo Game Boy Color", "GBC"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_gbc.webp"
        )),
    },
    Look {
        id: "nintendo_gba",
        title: "Game Boy Advance",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["Nintendo Game Boy Advance", "GBA"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_gba.webp"
        )),
    },
    Look {
        id: "nintendo_ds",
        title: "Nintendo DS",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["NDS"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_ds.webp"
        )),
    },
    Look {
        id: "nintendo_gamecube",
        title: "GameCube",
        maker: "Nintendo",
        kind: "6th gen console",
        names: &["Nintendo GameCube", "GC", "NGC"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_gamecube.webp"
        )),
    },
    Look {
        id: "nintendo_wii",
        title: "Wii",
        maker: "Nintendo",
        kind: "7th gen console",
        names: &["Nintendo Wii"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_wii.webp"
        )),
    },
    Look {
        id: "nintendo_wiiu",
        title: "Wii U",
        maker: "Nintendo",
        kind: "8th gen console",
        names: &["Nintendo Wii U", "WiiU"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_wiiu.webp"
        )),
    },
    Look {
        id: "nintendo_switch",
        title: "Switch",
        maker: "Nintendo",
        kind: "Hybrid console",
        names: &["Nintendo Switch"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_switch.webp"
        )),
    },
    Look {
        id: "sega_mastersystem",
        title: "Master System",
        maker: "Sega",
        kind: "8-bit console",
        names: &["Sega Master System", "SMS", "Mark III"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sega_mastersystem.webp"
        )),
    },
    Look {
        id: "sega_sg1000",
        title: "SG-1000",
        maker: "Sega",
        kind: "8-bit console",
        names: &["Sega SG-1000", "SG1000"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sega_sg1000.webp"
        )),
    },
    Look {
        id: "sega_genesis",
        title: "Genesis",
        maker: "Sega",
        kind: "16-bit console",
        names: &["Sega Genesis", "Mega Drive", "Sega Mega Drive", "MD"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sega_genesis.webp"
        )),
    },
    Look {
        id: "sega_saturn",
        title: "Saturn",
        maker: "Sega",
        kind: "32-bit console",
        names: &["Sega Saturn"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sega_saturn.webp"
        )),
    },
    Look {
        id: "sega_dreamcast",
        title: "Dreamcast",
        maker: "Sega",
        kind: "6th gen console",
        names: &["Sega Dreamcast", "DC"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sega_dreamcast.webp"
        )),
    },
    Look {
        id: "sony_playstation",
        title: "PlayStation",
        maker: "Sony",
        kind: "32-bit console",
        names: &["Sony PlayStation", "PS1", "PSX", "PS"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sony_playstation.webp"
        )),
    },
    Look {
        id: "sony_playstation2",
        title: "PlayStation 2",
        maker: "Sony",
        kind: "6th gen console",
        names: &["Sony PlayStation 2", "PS2"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sony_playstation2.webp"
        )),
    },
    Look {
        id: "sony_playstation3",
        title: "PlayStation 3",
        maker: "Sony",
        kind: "7th gen console",
        names: &["Sony PlayStation 3", "PS3"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sony_playstation3.webp"
        )),
    },
    Look {
        id: "sony_psp",
        title: "PSP",
        maker: "Sony",
        kind: "Handheld",
        names: &["Sony PlayStation Portable", "PlayStation Portable"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sony_psp.webp"
        )),
    },
    Look {
        id: "nintendo_3ds",
        title: "Nintendo 3DS",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["3DS", "N3DS", "New Nintendo 3DS"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_3ds.webp"
        )),
    },
    Look {
        id: "microsoft_xbox",
        title: "Xbox",
        maker: "Microsoft",
        kind: "6th gen console",
        names: &["Microsoft Xbox"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/microsoft_xbox.webp"
        )),
    },
    Look {
        id: "microsoft_xbox360",
        title: "Xbox 360",
        maker: "Microsoft",
        kind: "7th gen console",
        names: &["Microsoft Xbox 360", "X360"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/microsoft_xbox360.webp"
        )),
    },
    Look {
        id: "sony_playstation4",
        title: "PlayStation 4",
        maker: "Sony",
        kind: "8th gen console",
        names: &["Sony PlayStation 4", "PS4"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sony_playstation4.webp"
        )),
    },
    Look {
        id: "arcade",
        title: "Arcade",
        maker: "Arcade",
        kind: "Cabinet",
        names: &["MAME"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/arcade.webp"
        )),
    },
    Look {
        id: "sony_psvita",
        title: "PS Vita",
        maker: "Sony",
        kind: "Handheld",
        names: &["Sony PlayStation Vita", "PlayStation Vita", "Vita", "PSV"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sony_psvita.webp"
        )),
    },
    Look {
        id: "sega_gamegear",
        title: "Game Gear",
        maker: "Sega",
        kind: "Handheld",
        names: &["Sega Game Gear", "GG"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sega_gamegear.webp"
        )),
    },
    Look {
        id: "sega_cd",
        title: "Sega CD",
        maker: "Sega",
        kind: "Console add-on",
        names: &["Mega-CD", "Mega CD", "Sega Mega-CD"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sega_cd.webp"
        )),
    },
    Look {
        id: "sega_32x",
        title: "32X",
        maker: "Sega",
        kind: "Console add-on",
        names: &["Sega 32X"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/sega_32x.webp"
        )),
    },
    Look {
        id: "nintendo_virtualboy",
        title: "Virtual Boy",
        maker: "Nintendo",
        kind: "Stereoscopic console",
        names: &["Nintendo Virtual Boy", "VB"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nintendo_virtualboy.webp"
        )),
    },
    Look {
        id: "atari_2600",
        title: "Atari 2600",
        maker: "Atari",
        kind: "2nd gen console",
        names: &["Atari VCS", "2600"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/atari_2600.webp"
        )),
    },
    Look {
        id: "atari_5200",
        title: "Atari 5200",
        maker: "Atari",
        kind: "2nd gen console",
        names: &["Atari 5200 SuperSystem", "5200"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/atari_5200.webp"
        )),
    },
    Look {
        id: "atari_7800",
        title: "Atari 7800",
        maker: "Atari",
        kind: "3rd gen console",
        names: &["Atari 7800 ProSystem", "7800"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/atari_7800.webp"
        )),
    },
    Look {
        id: "atari_lynx",
        title: "Lynx",
        maker: "Atari",
        kind: "Handheld",
        names: &["Atari Lynx"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/atari_lynx.webp"
        )),
    },
    Look {
        id: "atari_jaguar",
        title: "Jaguar",
        maker: "Atari",
        kind: "64-bit console",
        names: &["Atari Jaguar"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/atari_jaguar.webp"
        )),
    },
    Look {
        id: "nec_pcengine",
        title: "TurboGrafx-16",
        maker: "NEC",
        kind: "16-bit console",
        names: &[
            "NEC TurboGrafx-16",
            "PC Engine",
            "NEC PC Engine",
            "TurboGrafx-16/PC Engine",
            "TG16",
        ],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/nec_pcengine.webp"
        )),
    },
    Look {
        id: "snk_neogeo_aes",
        title: "Neo Geo AES",
        maker: "SNK",
        kind: "16-bit console",
        names: &[
            "SNK Neo Geo AES",
            "SNK Neo Geo",
            "Neo Geo",
            "Neo-Geo",
            "snk_neogeo",
        ],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/snk_neogeo_aes.webp"
        )),
    },
    Look {
        id: "snk_neogeopocketcolor",
        title: "Neo Geo Pocket Color",
        maker: "SNK",
        kind: "Handheld",
        names: &["SNK Neo Geo Pocket Color", "NGPC", "snk_neogeopocket_color"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/snk_neogeopocketcolor.webp"
        )),
    },
    Look {
        id: "bandai_wonderswancolor",
        title: "WonderSwan Color",
        maker: "Bandai",
        kind: "Handheld",
        names: &["Bandai WonderSwan Color", "WSC", "bandai_wonderswan_color"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/bandai_wonderswancolor.webp"
        )),
    },
    Look {
        id: "coleco_colecovision",
        title: "ColecoVision",
        maker: "Coleco",
        kind: "2nd gen console",
        names: &["Coleco ColecoVision", "Coleco Vision"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/coleco_colecovision.webp"
        )),
    },
    Look {
        id: "mattel_intellivision",
        title: "Intellivision",
        maker: "Mattel",
        kind: "2nd gen console",
        names: &["Mattel Intellivision"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/mattel_intellivision.webp"
        )),
    },
    Look {
        id: "panasonic_3do",
        title: "3DO",
        maker: "Panasonic",
        kind: "32-bit console",
        names: &["3DO Interactive Multiplayer", "Panasonic 3DO"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/panasonic_3do.webp"
        )),
    },
    Look {
        id: "commodore_64",
        title: "Commodore 64",
        maker: "Commodore",
        kind: "Home computer",
        names: &["C64", "Commodore C64", "Commodore C64/128/MAX"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/commodore_64.webp"
        )),
    },
    Look {
        id: "commodore_amiga",
        title: "Amiga",
        maker: "Commodore",
        kind: "Home computer",
        names: &["Commodore Amiga"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/commodore_amiga.webp"
        )),
    },
    Look {
        id: "microsoft_msx",
        title: "MSX",
        maker: "MSX",
        kind: "Home computer",
        names: &["Microsoft MSX"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/microsoft_msx.webp"
        )),
    },
    Look {
        id: "pc_dos",
        title: "DOS",
        maker: "PC",
        kind: "Home computer",
        names: &["MS-DOS", "PC DOS", "DOSBox"],
        art: Some(include_bytes!(
            "../../../desktop/src/views/library/platforms/art/pc_dos.webp"
        )),
    },
    Look {
        id: "sony_playstation5",
        title: "PlayStation 5",
        maker: "Sony",
        kind: "9th gen console",
        names: &["Sony PlayStation 5", "PS5"],
        art: None,
    },
    Look {
        id: "xbox_one",
        title: "Xbox One",
        maker: "Microsoft",
        kind: "8th gen console",
        names: &[
            "Microsoft Xbox One",
            "Xbox One S",
            "Xbox One X",
            "microsoft_xboxone",
        ],
        art: None,
    },
    Look {
        id: "xbox_series",
        title: "Xbox Series",
        maker: "Microsoft",
        kind: "9th gen console",
        names: &[
            "Microsoft Xbox Series",
            "Xbox Series X",
            "Xbox Series S",
            "Xbox Series X|S",
            "microsoft_xboxseries",
        ],
        art: None,
    },
    Look {
        id: "nintendo_switch2",
        title: "Switch 2",
        maker: "Nintendo",
        kind: "Hybrid console",
        names: &["Nintendo Switch 2", "NSW2"],
        art: None,
    },
    Look {
        id: "sinclair_zxspectrum",
        title: "ZX Spectrum",
        maker: "Sinclair",
        kind: "Home computer",
        names: &[
            "Sinclair ZX Spectrum",
            "ZX Spectrum 48K",
            "Spectrum",
            "ZX Spectrum +3",
            "sinclair_zxspectrum3",
        ],
        art: None,
    },
    Look {
        id: "amstrad_cpc",
        title: "Amstrad CPC",
        maker: "Amstrad",
        kind: "Home computer",
        names: &["CPC", "Amstrad CPC 464", "CPC464"],
        art: None,
    },
    Look {
        id: "bbcmicro",
        title: "BBC Micro",
        maker: "Acorn",
        kind: "Home computer",
        names: &["BBC Microcomputer System", "Acorn BBC Micro", "BBC Model B"],
        art: None,
    },
    Look {
        id: "atari_8bit",
        title: "Atari 8-bit",
        maker: "Atari",
        kind: "Home computer",
        names: &[
            "Atari 8-bit",
            "Atari 800",
            "Atari 800XL",
            "Atari 400",
            "Atari XL/XE",
            "atari800",
        ],
        art: None,
    },
    Look {
        id: "atari_st",
        title: "Atari ST",
        maker: "Atari",
        kind: "Home computer",
        names: &["Atari ST/STE", "Atari STE", "Atari 520ST", "Atari 1040ST"],
        art: None,
    },
    Look {
        id: "apple_2",
        title: "Apple II",
        maker: "Apple",
        kind: "Home computer",
        names: &["Apple II", "Apple IIe", "Apple 2", "apple_ii"],
        art: None,
    },
    Look {
        id: "sharp_x68000",
        title: "X68000",
        maker: "Sharp",
        kind: "Home computer",
        names: &["Sharp X68000", "X68K"],
        art: None,
    },
    Look {
        id: "nec_pc98",
        title: "PC-98",
        maker: "NEC",
        kind: "Home computer",
        names: &["NEC PC-98", "PC-9801", "NEC PC-9801", "PC-9800 Series"],
        art: None,
    },
    Look {
        id: "commodore_vci20",
        title: "VIC-20",
        maker: "Commodore",
        kind: "Home computer",
        names: &[
            "Commodore VIC20",
            "Commodore VIC-20",
            "VIC20",
            "commodore_vic20",
        ],
        art: None,
    },
    Look {
        id: "commodore_amiga_cd32",
        title: "Amiga CD32",
        maker: "Commodore",
        kind: "32-bit console",
        names: &["Commodore Amiga CD32", "Commodore CD32", "CD32"],
        art: None,
    },
    Look {
        id: "snk_neogeo_cd",
        title: "Neo Geo CD",
        maker: "SNK",
        kind: "CD console",
        names: &["SNK Neo Geo CD", "Neo-Geo CD", "NGCD"],
        art: None,
    },
    Look {
        id: "nec_turbografx_cd",
        title: "TurboGrafx-CD",
        maker: "NEC",
        kind: "CD console",
        names: &[
            "NEC TurboGrafx-CD",
            "PC Engine CD",
            "PC Engine CD-ROM2",
            "PC Engine CD-ROM²",
            "Turbo Duo",
            "PC Engine Duo",
            "nec_pcenginecd",
        ],
        art: None,
    },
    Look {
        id: "bandai_wonderswan",
        title: "WonderSwan",
        maker: "Bandai",
        kind: "Handheld",
        names: &["Bandai WonderSwan", "WS"],
        art: None,
    },
    Look {
        id: "snk_neogeopocket",
        title: "Neo Geo Pocket",
        maker: "SNK",
        kind: "Handheld",
        names: &["SNK Neo Geo Pocket", "NGP"],
        art: None,
    },
    Look {
        id: "vectrex",
        title: "Vectrex",
        maker: "GCE",
        kind: "Vector console",
        names: &["GCE Vectrex", "gce_vectrex"],
        art: None,
    },
    Look {
        id: "magnavox_odyssey_2",
        title: "Odyssey²",
        maker: "Magnavox",
        kind: "2nd gen console",
        names: &[
            "Magnavox Odyssey 2",
            "Magnavox Odyssey²",
            "Odyssey 2",
            "Odyssey2",
            "Philips Videopac G7000",
            "Videopac",
        ],
        art: None,
    },
    Look {
        id: "nintendo_famicom_disk",
        title: "Famicom Disk System",
        maker: "Nintendo",
        kind: "Console add-on",
        names: &[
            "Nintendo Family Computer Disk System",
            "Nintendo Famicom Disk System",
            "Famicom Disk System",
            "FDS",
        ],
        art: None,
    },
    Look {
        id: "pokemon_mini",
        title: "Pokémon mini",
        maker: "Nintendo",
        kind: "Handheld",
        names: &[
            "Pokemon Mini",
            "Nintendo Pokémon mini",
            "Nintendo Pokemon Mini",
            "nintendo_pokemonmini",
        ],
        art: None,
    },
    Look {
        id: "pc_windows",
        title: "Windows",
        maker: "PC",
        kind: "Desktop computer",
        names: &[
            "PC (Windows)",
            "Windows PC",
            "Microsoft Windows",
            "Win32",
            "Windows",
        ],
        art: None,
    },
    Look {
        id: "pc_linux",
        title: "Linux",
        maker: "PC",
        kind: "Desktop computer",
        names: &["PC (Linux)", "Linux PC", "Linux"],
        art: None,
    },
    Look {
        id: "macintosh",
        title: "Macintosh",
        maker: "Apple",
        kind: "Desktop computer",
        names: &[
            "Mac",
            "macOS",
            "Mac OS",
            "Mac OS X",
            "Apple Macintosh",
            "PC (Mac)",
        ],
        art: None,
    },
    Look {
        id: "microsoft_msx2",
        title: "MSX2",
        maker: "MSX",
        kind: "Home computer",
        names: &[
            "Microsoft MSX2",
            "MSX 2",
            "MSX2+",
            "Microsoft MSX2+",
            "msx2plus",
        ],
        art: None,
    },
];

/// `name` folded for matching: lower case, letters and digits only.
fn key(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn by_name() -> &'static HashMap<String, &'static Look> {
    static BY_NAME: OnceLock<HashMap<String, &'static Look>> = OnceLock::new();
    BY_NAME.get_or_init(|| {
        let mut map = HashMap::new();
        for look in LOOKS {
            for name in [look.id, look.title].iter().chain(look.names) {
                map.insert(key(name), look);
            }
        }
        map
    })
}

/// The kinds of machine, translated where they're shown.
pub fn kind_label(kind: &str) -> String {
    match kind {
        "2nd gen console" => tp!("platform kind", "2nd gen console"),
        "3rd gen console" => tp!("platform kind", "3rd gen console"),
        "8-bit console" => tp!("platform kind", "8-bit console"),
        "16-bit console" => tp!("platform kind", "16-bit console"),
        "32-bit console" => tp!("platform kind", "32-bit console"),
        "64-bit console" => tp!("platform kind", "64-bit console"),
        "6th gen console" => tp!("platform kind", "6th gen console"),
        "7th gen console" => tp!("platform kind", "7th gen console"),
        "8th gen console" => tp!("platform kind", "8th gen console"),
        "9th gen console" => tp!("platform kind", "9th gen console"),
        "Cabinet" => tp!("platform kind", "Cabinet"),
        "CD console" => tp!("platform kind", "CD console"),
        "Console add-on" => tp!("platform kind", "Console add-on"),
        "Desktop computer" => tp!("platform kind", "Desktop computer"),
        "Handheld" => tp!("platform kind", "Handheld"),
        "Home computer" => tp!("platform kind", "Home computer"),
        "Hybrid console" => tp!("platform kind", "Hybrid console"),
        "Stereoscopic console" => tp!("platform kind", "Stereoscopic console"),
        "Vector console" => tp!("platform kind", "Vector console"),
        other => other.to_owned(),
    }
}

/// How to show a platform.
#[derive(Debug, Clone, PartialEq)]
pub struct Presentation {
    /// The library's name for it, which the platform filter takes.
    pub name: String,
    pub title: String,
    /// "Nintendo · 8-bit console"; empty for a platform Caskade doesn't know.
    pub subtitle: String,
    /// The hardware render's URL, or the neutral artwork's.
    pub art: String,
    /// Whether `art` is the platform's own.
    pub known: bool,
}

fn art_url(id: &str) -> String {
    format!("{}://{ART_HOST}/{id}.webp", super::art::SCHEME)
}

/// How to show the platform the library calls `name`. Unknown platforms
/// keep their name and get the neutral artwork.
pub fn present(name: &str) -> Presentation {
    let Some(look) = by_name().get(&key(name)) else {
        return Presentation {
            name: name.to_owned(),
            title: name.to_owned(),
            subtitle: String::new(),
            art: art_url("generic"),
            known: false,
        };
    };
    let kind = kind_label(look.kind);
    Presentation {
        name: name.to_owned(),
        title: look.title.to_owned(),
        subtitle: if look.maker == look.title {
            kind
        } else {
            format!("{} · {kind}", look.maker)
        },
        art: art_url(if look.art.is_some() {
            look.id
        } else {
            "generic"
        }),
        known: look.art.is_some(),
    }
}

/// The render at `caskade-asset://platforms/<path>`, if it's one.
pub fn art(path: &str) -> Option<&'static [u8]> {
    let id = path.trim_start_matches('/').strip_suffix(".webp")?;
    if id == "generic" {
        return Some(GENERIC_ART);
    }
    LOOKS.iter().find(|look| look.id == id)?.art
}

/// The platforms in the library's sort: the last played or the most played
/// first, by title among equals and those never played; by title for the
/// title sort.
pub fn sort(mut platforms: Vec<PlatformSummary>, sort: GameSort) -> Vec<PlatformSummary> {
    let title = |platform: &PlatformSummary| present(&platform.name).title.to_lowercase();
    platforms.sort_by(|a, b| {
        let first = match sort {
            GameSort::Recent => b.last_played_at_ms.cmp(&a.last_played_at_ms),
            GameSort::Playtime => b.playtime_ms.cmp(&a.playtime_ms),
            _ => std::cmp::Ordering::Equal,
        };
        first.then_with(|| title(a).cmp(&title(b)))
    });
    platforms
}

/// The words of a PC platform's name ("PC (Microsoft Windows)", Linux).
const PC_WORDS: &[&str] = &["pc", "microsoft", "windows", "win", "linux", "gnu"];

/// Whether `name` is one of the PC platforms a card leaves unsaid ("PC
/// Engine" and "PC-98" are machines of their own), as the webview's
/// `nonPcPlatforms`.
pub fn is_pc(name: &str) -> bool {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    words.is_empty() || words.iter().all(|word| PC_WORDS.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The webview's catalogue, which this one copies until the cutover.
    const WEBVIEW: &str = include_str!("../../../desktop/src/views/library/platforms/catalog.ts");

    #[test]
    fn the_looks_are_the_webviews() {
        for look in LOOKS {
            let at = WEBVIEW
                .find(&format!("  {}: {{", look.id))
                .unwrap_or_else(|| panic!("{} isn't in catalog.ts", look.id));
            let entry = &WEBVIEW[at..];
            let entry = &entry[..entry.find("\n  },").or_else(|| entry.find("},\n")).unwrap()];
            for text in [look.title, look.maker, look.kind].iter().chain(look.names) {
                assert!(
                    entry.contains(&format!("{text:?}")),
                    "{}: {text:?} isn't in catalog.ts's entry",
                    look.id
                );
            }
        }
        let ids =
            WEBVIEW.matches(": {\n    title:").count() + WEBVIEW.matches(": { title:").count();
        assert_eq!(ids, LOOKS.len(), "catalog.ts has looks this doesn't");
    }

    #[test]
    fn platforms_are_found_by_any_name_and_unknown_ones_keep_theirs() {
        let snes = present("Super Nintendo Entertainment System");
        assert_eq!(snes.title, "SNES");
        assert_eq!(snes.subtitle, "Nintendo · 16-bit console");
        assert_eq!(snes.art, "caskade-asset://platforms/nintendo_snes.webp");
        assert!(snes.known);
        assert_eq!(present("super famicom").title, "SNES");
        let odd = present("Tiger Game.com 2");
        assert_eq!(odd.title, "Tiger Game.com 2");
        assert!(odd.subtitle.is_empty());
        assert!(!odd.known);
        assert_eq!(odd.art, "caskade-asset://platforms/generic.webp");
        assert!(art("/nintendo_snes.webp").is_some());
        assert!(art("/generic.webp").is_some());
        assert!(art("/nope.webp").is_none());
    }

    #[test]
    fn pc_platforms_are_told_from_machines_named_pc() {
        assert!(is_pc("PC"));
        assert!(is_pc("PC (Microsoft Windows)"));
        assert!(is_pc("Linux"));
        assert!(!is_pc("PC Engine"));
        assert!(!is_pc("PC-98"));
        assert!(!is_pc("Nintendo 64"));
    }

    #[test]
    fn platforms_sort_by_last_play_playtime_or_title() {
        let platform = |name: &str, playtime_ms: u64, last: Option<i64>| PlatformSummary {
            name: name.to_owned(),
            games: 1,
            playtime_ms,
            last_played_at_ms: last,
        };
        let list = vec![
            platform("Nintendo 64", 10, None),
            platform("Super Nintendo Entertainment System", 5, Some(2)),
            platform("Arcade", 20, Some(1)),
        ];
        let names = |list: Vec<PlatformSummary>| -> Vec<String> {
            list.into_iter()
                .map(|platform| present(&platform.name).title)
                .collect()
        };
        assert_eq!(
            names(sort(list.clone(), GameSort::Recent)),
            ["SNES", "Arcade", "Nintendo 64"]
        );
        assert_eq!(
            names(sort(list.clone(), GameSort::Playtime)),
            ["Arcade", "Nintendo 64", "SNES"]
        );
        assert_eq!(
            names(sort(list, GameSort::Title)),
            ["Arcade", "Nintendo 64", "SNES"]
        );
    }
}

//! How each platform looks, data only: its colours and art.

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
pub(super) const GENERIC_ART: &[u8] = include_bytes!("../../../assets/platforms/generic.webp");

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
            "../../../assets/platforms/nintendo_nes.webp"
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
            "../../../assets/platforms/nintendo_snes.webp"
        )),
    },
    Look {
        id: "nintendo_n64",
        title: "Nintendo 64",
        maker: "Nintendo",
        kind: "64-bit console",
        names: &["N64"],
        art: Some(include_bytes!(
            "../../../assets/platforms/nintendo_n64.webp"
        )),
    },
    Look {
        id: "nintendo_gb",
        title: "Game Boy",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["Nintendo Game Boy", "GB"],
        art: Some(include_bytes!("../../../assets/platforms/nintendo_gb.webp")),
    },
    Look {
        id: "nintendo_gbc",
        title: "Game Boy Color",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["Nintendo Game Boy Color", "GBC"],
        art: Some(include_bytes!(
            "../../../assets/platforms/nintendo_gbc.webp"
        )),
    },
    Look {
        id: "nintendo_gba",
        title: "Game Boy Advance",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["Nintendo Game Boy Advance", "GBA"],
        art: Some(include_bytes!(
            "../../../assets/platforms/nintendo_gba.webp"
        )),
    },
    Look {
        id: "nintendo_ds",
        title: "Nintendo DS",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["NDS"],
        art: Some(include_bytes!("../../../assets/platforms/nintendo_ds.webp")),
    },
    Look {
        id: "nintendo_gamecube",
        title: "GameCube",
        maker: "Nintendo",
        kind: "6th gen console",
        names: &["Nintendo GameCube", "GC", "NGC"],
        art: Some(include_bytes!(
            "../../../assets/platforms/nintendo_gamecube.webp"
        )),
    },
    Look {
        id: "nintendo_wii",
        title: "Wii",
        maker: "Nintendo",
        kind: "7th gen console",
        names: &["Nintendo Wii"],
        art: Some(include_bytes!(
            "../../../assets/platforms/nintendo_wii.webp"
        )),
    },
    Look {
        id: "nintendo_wiiu",
        title: "Wii U",
        maker: "Nintendo",
        kind: "8th gen console",
        names: &["Nintendo Wii U", "WiiU"],
        art: Some(include_bytes!(
            "../../../assets/platforms/nintendo_wiiu.webp"
        )),
    },
    Look {
        id: "nintendo_switch",
        title: "Switch",
        maker: "Nintendo",
        kind: "Hybrid console",
        names: &["Nintendo Switch"],
        art: Some(include_bytes!(
            "../../../assets/platforms/nintendo_switch.webp"
        )),
    },
    Look {
        id: "sega_mastersystem",
        title: "Master System",
        maker: "Sega",
        kind: "8-bit console",
        names: &["Sega Master System", "SMS", "Mark III"],
        art: Some(include_bytes!(
            "../../../assets/platforms/sega_mastersystem.webp"
        )),
    },
    Look {
        id: "sega_sg1000",
        title: "SG-1000",
        maker: "Sega",
        kind: "8-bit console",
        names: &["Sega SG-1000", "SG1000"],
        art: Some(include_bytes!("../../../assets/platforms/sega_sg1000.webp")),
    },
    Look {
        id: "sega_genesis",
        title: "Genesis",
        maker: "Sega",
        kind: "16-bit console",
        names: &["Sega Genesis", "Mega Drive", "Sega Mega Drive", "MD"],
        art: Some(include_bytes!(
            "../../../assets/platforms/sega_genesis.webp"
        )),
    },
    Look {
        id: "sega_saturn",
        title: "Saturn",
        maker: "Sega",
        kind: "32-bit console",
        names: &["Sega Saturn"],
        art: Some(include_bytes!("../../../assets/platforms/sega_saturn.webp")),
    },
    Look {
        id: "sega_dreamcast",
        title: "Dreamcast",
        maker: "Sega",
        kind: "6th gen console",
        names: &["Sega Dreamcast", "DC"],
        art: Some(include_bytes!(
            "../../../assets/platforms/sega_dreamcast.webp"
        )),
    },
    Look {
        id: "sony_playstation",
        title: "PlayStation",
        maker: "Sony",
        kind: "32-bit console",
        names: &["Sony PlayStation", "PS1", "PSX", "PS"],
        art: Some(include_bytes!(
            "../../../assets/platforms/sony_playstation.webp"
        )),
    },
    Look {
        id: "sony_playstation2",
        title: "PlayStation 2",
        maker: "Sony",
        kind: "6th gen console",
        names: &["Sony PlayStation 2", "PS2"],
        art: Some(include_bytes!(
            "../../../assets/platforms/sony_playstation2.webp"
        )),
    },
    Look {
        id: "sony_playstation3",
        title: "PlayStation 3",
        maker: "Sony",
        kind: "7th gen console",
        names: &["Sony PlayStation 3", "PS3"],
        art: Some(include_bytes!(
            "../../../assets/platforms/sony_playstation3.webp"
        )),
    },
    Look {
        id: "sony_psp",
        title: "PSP",
        maker: "Sony",
        kind: "Handheld",
        names: &["Sony PlayStation Portable", "PlayStation Portable"],
        art: Some(include_bytes!("../../../assets/platforms/sony_psp.webp")),
    },
    Look {
        id: "nintendo_3ds",
        title: "Nintendo 3DS",
        maker: "Nintendo",
        kind: "Handheld",
        names: &["3DS", "N3DS", "New Nintendo 3DS"],
        art: Some(include_bytes!(
            "../../../assets/platforms/nintendo_3ds.webp"
        )),
    },
    Look {
        id: "microsoft_xbox",
        title: "Xbox",
        maker: "Microsoft",
        kind: "6th gen console",
        names: &["Microsoft Xbox"],
        art: Some(include_bytes!(
            "../../../assets/platforms/microsoft_xbox.webp"
        )),
    },
    Look {
        id: "microsoft_xbox360",
        title: "Xbox 360",
        maker: "Microsoft",
        kind: "7th gen console",
        names: &["Microsoft Xbox 360", "X360"],
        art: Some(include_bytes!(
            "../../../assets/platforms/microsoft_xbox360.webp"
        )),
    },
    Look {
        id: "sony_playstation4",
        title: "PlayStation 4",
        maker: "Sony",
        kind: "8th gen console",
        names: &["Sony PlayStation 4", "PS4"],
        art: Some(include_bytes!(
            "../../../assets/platforms/sony_playstation4.webp"
        )),
    },
    Look {
        id: "arcade",
        title: "Arcade",
        maker: "Arcade",
        kind: "Cabinet",
        names: &["MAME"],
        art: Some(include_bytes!("../../../assets/platforms/arcade.webp")),
    },
    Look {
        id: "sony_psvita",
        title: "PS Vita",
        maker: "Sony",
        kind: "Handheld",
        names: &["Sony PlayStation Vita", "PlayStation Vita", "Vita", "PSV"],
        art: Some(include_bytes!("../../../assets/platforms/sony_psvita.webp")),
    },
    Look {
        id: "sega_gamegear",
        title: "Game Gear",
        maker: "Sega",
        kind: "Handheld",
        names: &["Sega Game Gear", "GG"],
        art: Some(include_bytes!(
            "../../../assets/platforms/sega_gamegear.webp"
        )),
    },
    Look {
        id: "sega_cd",
        title: "Sega CD",
        maker: "Sega",
        kind: "Console add-on",
        names: &["Mega-CD", "Mega CD", "Sega Mega-CD"],
        art: Some(include_bytes!("../../../assets/platforms/sega_cd.webp")),
    },
    Look {
        id: "sega_32x",
        title: "32X",
        maker: "Sega",
        kind: "Console add-on",
        names: &["Sega 32X"],
        art: Some(include_bytes!("../../../assets/platforms/sega_32x.webp")),
    },
    Look {
        id: "nintendo_virtualboy",
        title: "Virtual Boy",
        maker: "Nintendo",
        kind: "Stereoscopic console",
        names: &["Nintendo Virtual Boy", "VB"],
        art: Some(include_bytes!(
            "../../../assets/platforms/nintendo_virtualboy.webp"
        )),
    },
    Look {
        id: "atari_2600",
        title: "Atari 2600",
        maker: "Atari",
        kind: "2nd gen console",
        names: &["Atari VCS", "2600"],
        art: Some(include_bytes!("../../../assets/platforms/atari_2600.webp")),
    },
    Look {
        id: "atari_5200",
        title: "Atari 5200",
        maker: "Atari",
        kind: "2nd gen console",
        names: &["Atari 5200 SuperSystem", "5200"],
        art: Some(include_bytes!("../../../assets/platforms/atari_5200.webp")),
    },
    Look {
        id: "atari_7800",
        title: "Atari 7800",
        maker: "Atari",
        kind: "3rd gen console",
        names: &["Atari 7800 ProSystem", "7800"],
        art: Some(include_bytes!("../../../assets/platforms/atari_7800.webp")),
    },
    Look {
        id: "atari_lynx",
        title: "Lynx",
        maker: "Atari",
        kind: "Handheld",
        names: &["Atari Lynx"],
        art: Some(include_bytes!("../../../assets/platforms/atari_lynx.webp")),
    },
    Look {
        id: "atari_jaguar",
        title: "Jaguar",
        maker: "Atari",
        kind: "64-bit console",
        names: &["Atari Jaguar"],
        art: Some(include_bytes!(
            "../../../assets/platforms/atari_jaguar.webp"
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
            "../../../assets/platforms/nec_pcengine.webp"
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
            "../../../assets/platforms/snk_neogeo_aes.webp"
        )),
    },
    Look {
        id: "snk_neogeopocketcolor",
        title: "Neo Geo Pocket Color",
        maker: "SNK",
        kind: "Handheld",
        names: &["SNK Neo Geo Pocket Color", "NGPC", "snk_neogeopocket_color"],
        art: Some(include_bytes!(
            "../../../assets/platforms/snk_neogeopocketcolor.webp"
        )),
    },
    Look {
        id: "bandai_wonderswancolor",
        title: "WonderSwan Color",
        maker: "Bandai",
        kind: "Handheld",
        names: &["Bandai WonderSwan Color", "WSC", "bandai_wonderswan_color"],
        art: Some(include_bytes!(
            "../../../assets/platforms/bandai_wonderswancolor.webp"
        )),
    },
    Look {
        id: "coleco_colecovision",
        title: "ColecoVision",
        maker: "Coleco",
        kind: "2nd gen console",
        names: &["Coleco ColecoVision", "Coleco Vision"],
        art: Some(include_bytes!(
            "../../../assets/platforms/coleco_colecovision.webp"
        )),
    },
    Look {
        id: "mattel_intellivision",
        title: "Intellivision",
        maker: "Mattel",
        kind: "2nd gen console",
        names: &["Mattel Intellivision"],
        art: Some(include_bytes!(
            "../../../assets/platforms/mattel_intellivision.webp"
        )),
    },
    Look {
        id: "panasonic_3do",
        title: "3DO",
        maker: "Panasonic",
        kind: "32-bit console",
        names: &["3DO Interactive Multiplayer", "Panasonic 3DO"],
        art: Some(include_bytes!(
            "../../../assets/platforms/panasonic_3do.webp"
        )),
    },
    Look {
        id: "commodore_64",
        title: "Commodore 64",
        maker: "Commodore",
        kind: "Home computer",
        names: &["C64", "Commodore C64", "Commodore C64/128/MAX"],
        art: Some(include_bytes!(
            "../../../assets/platforms/commodore_64.webp"
        )),
    },
    Look {
        id: "commodore_amiga",
        title: "Amiga",
        maker: "Commodore",
        kind: "Home computer",
        names: &["Commodore Amiga"],
        art: Some(include_bytes!(
            "../../../assets/platforms/commodore_amiga.webp"
        )),
    },
    Look {
        id: "microsoft_msx",
        title: "MSX",
        maker: "MSX",
        kind: "Home computer",
        names: &["Microsoft MSX"],
        art: Some(include_bytes!(
            "../../../assets/platforms/microsoft_msx.webp"
        )),
    },
    Look {
        id: "pc_dos",
        title: "DOS",
        maker: "PC",
        kind: "Home computer",
        names: &["MS-DOS", "PC DOS", "DOSBox"],
        art: Some(include_bytes!("../../../assets/platforms/pc_dos.webp")),
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

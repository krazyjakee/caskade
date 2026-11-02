//! The catalogue names a game's system in free text ("Nintendo 64",
//! "PlayStation 1", "PC (MS-DOS / Windows)"). Matching needs Caskade's
//! platform ids, and the Hasheous dumps that know the system's games.

use regex::Regex;
use std::{collections::BTreeSet, sync::LazyLock};

/// Every PC system (DOS, Windows, Linux, Mac) is one family: ports of PC
/// games run on any of them, and the library rarely knows which it was.
pub const PC: &str = "pc";

/// Caskade platform ids that are PC systems.
const PC_PLATFORMS: [&str; 8] = [
    "pc_dos",
    "pc_windows",
    "pc_linux",
    "win3x",
    "win9x",
    "macintosh",
    "windows_apps",
    "pc_booter",
];

/// (pattern over the lowercased name, platform ids). Every matching rule
/// counts, so "Game Boy / Game Boy Color" is both.
const RULES: &[(&str, &[&str])] = &[
    (r"\bgame ?boy advance\b|\bgba\b", &["nintendo_gba"]),
    (r"\bgame ?boy colou?r\b|\bgbc\b", &["nintendo_gbc"]),
    (r"\bgame ?boy\b(?: *(?:/|,|$| \())", &["nintendo_gb"]),
    (r"\bnintendo 64\b|\bn64\b", &["nintendo_n64"]),
    (
        r"\bsuper (?:nintendo|famicom)\b|\bsnes\b",
        &["nintendo_snes"],
    ),
    (
        r"\bnintendo entertainment system\b|\bnes\b|\bfamicom\b",
        &["nintendo_nes"],
    ),
    (r"gamecube", &["nintendo_gamecube"]),
    (r"\bwii ?u\b", &["nintendo_wiiu"]),
    (r"\bwii\b(?: *(?:/|,|$| \())", &["nintendo_wii"]),
    (r"\b3ds\b", &["nintendo_3ds", "new_nintendo_3ds"]),
    (r"\bnintendo ds\b|\bnds\b", &["nintendo_ds"]),
    (r"virtual boy", &["nintendo_virtualboy"]),
    (r"\bswitch\b", &["nintendo_switch"]),
    (r"playstation portable|\bpsp\b", &["sony_psp"]),
    (r"playstation vita|\bps ?vita\b", &["sony_vita"]),
    (r"playstation 2|\bps2\b", &["sony_playstation2"]),
    (r"playstation 3|\bps3\b", &["sony_playstation3"]),
    (r"playstation 4|\bps4\b", &["sony_playstation4"]),
    (r"playstation 5|\bps5\b", &["sony_playstation5"]),
    (
        r"playstation(?: 1)?\b(?: *(?:/|,|$| \())|\bps1\b|\bpsx\b",
        &["sony_playstation"],
    ),
    (r"xbox 360", &["microsoft_xbox360"]),
    (r"xbox one", &["xbox_one"]),
    (r"xbox series", &["xbox_series"]),
    (r"\bxbox\b(?: *(?:/|,|$| \())", &["microsoft_xbox"]),
    (r"mega ?-?cd|sega cd", &["sega_cd"]),
    (r"\b32x\b", &["sega_32x"]),
    (r"genesis|mega drive", &["sega_genesis"]),
    (r"master system", &["sega_mastersystem"]),
    (r"game gear", &["sega_gamegear"]),
    (r"saturn", &["sega_saturn"]),
    (r"dreamcast", &["sega_dreamcast"]),
    (r"neo ?geo", &["neogeomvs", "snk_neogeo_aes"]),
    (
        r"arcade|\bcps-?[123]\b|cp system|sega model|naomi|quicksilver",
        &["arcade"],
    ),
    (r"amiga", &["commodore_amiga"]),
    (r"commodore 64|\bc64\b", &["commodore_64"]),
    (r"n-gage", &["ngage"]),
    (r"\b3do\b", &["3do"]),
    (r"atari 2600", &["atari_2600"]),
    (r"atari lynx", &["atari_lynx"]),
    (r"atari 8-bit", &["atari_8bit"]),
    (r"zx spectrum", &["sinclair_zxspectrum"]),
    (r"bbc micro", &["bbcmicro"]),
    (r"apple ii\b", &["apple_2"]),
    (r"\bj2me\b|java (?:me|mobile|2 micro)", &["j2me"]),
    (r"\bandroid\b", &["android"]),
    (r"\bios\b|iphone|ipad", &["ios"]),
    (
        r"\bdos\b|windows|^pc\b|\bpc\b|macintosh|mac ?os|^macos|\blinux\b",
        &[PC],
    ),
];

static COMPILED: LazyLock<Vec<(Regex, &'static [&'static str])>> = LazyLock::new(|| {
    RULES
        .iter()
        .map(|(pattern, ids)| (Regex::new(pattern).expect("valid console rule"), *ids))
        .collect()
});

/// The platform families a catalogue system name stands for; empty when
/// none is known.
pub fn families(console: &str) -> BTreeSet<String> {
    // Xbox Live Arcade is a store, not arcade hardware.
    let text = console.to_lowercase().replace("xbox live arcade", "");
    COMPILED
        .iter()
        .filter(|(rule, _)| rule.is_match(&text))
        .flat_map(|(_, ids)| ids.iter().map(|id| (*id).to_owned()))
        .collect()
}

/// The family of a library platform: a Caskade platform id, or a platform
/// name a source wrote (RomM writes names).
pub fn family_of_platform(platform: &str) -> BTreeSet<String> {
    if PC_PLATFORMS.contains(&platform) {
        return BTreeSet::from([PC.to_owned()]);
    }
    if platform
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return BTreeSet::from([platform.to_owned()]);
    }
    families(platform)
}

/// The Hasheous dumps that list a family's games.
pub fn hasheous_dumps(family: &str) -> &'static [&'static str] {
    match family {
        "nintendo_gba" => &["Nintendo Game Boy Advance"],
        "nintendo_gbc" => &["Nintendo Game Boy Color"],
        "nintendo_gb" => &["Nintendo Game Boy"],
        "nintendo_n64" => &["Nintendo 64"],
        "nintendo_snes" => &["Nintendo Super Nintendo Entertainment System"],
        "nintendo_nes" => &[
            "Nintendo Entertainment System",
            "Nintendo Famicom Disk System",
        ],
        "nintendo_gamecube" => &["Nintendo GameCube"],
        "nintendo_wiiu" => &["Nintendo WiiU"],
        "nintendo_wii" => &["Nintendo Wii"],
        "nintendo_3ds" | "new_nintendo_3ds" => &["Nintendo 3DS"],
        "nintendo_ds" => &["Nintendo DS"],
        "nintendo_virtualboy" => &["Nintendo Virtual Boy"],
        "sony_psp" => &["Sony Playstation Portable"],
        "sony_vita" => &["Sony PlayStation Vita"],
        "sony_playstation" => &["Sony PlayStation"],
        "sony_playstation2" => &["Sony PlayStation 2"],
        "sony_playstation3" => &["Sony Playstation 3"],
        "sony_playstation4" => &["Sony Playstation 4"],
        "sony_playstation5" => &["Sony Playstation 5"],
        "microsoft_xbox360" => &["Microsoft Xbox 360", "Xbox 360"],
        "microsoft_xbox" => &["Microsoft Xbox"],
        "xbox_one" => &["Microsoft Xbox One"],
        "sega_cd" => &["Sega Mega CD _ Sega CD"],
        "sega_32x" => &["Sega 32X"],
        "sega_genesis" => &["Sega Mega Drive _ Genesis"],
        "sega_mastersystem" => &["Sega Master System"],
        "sega_gamegear" => &["Sega Game Gear"],
        "sega_saturn" => &["Sega Saturn"],
        "sega_dreamcast" => &["Sega Dreamcast"],
        "arcade" => &["Arcade"],
        "commodore_amiga" => &["Commodore Amiga"],
        "commodore_64" => &["Commodore 64"],
        PC => &["Microsoft Windows", "Microsoft DOS", "Apple Mac"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(console: &str) -> Vec<String> {
        families(console).into_iter().collect()
    }

    #[test]
    fn free_text_systems_become_platform_families() {
        assert_eq!(ids("Nintendo 64"), ["nintendo_n64"]);
        assert_eq!(ids("PlayStation 1"), ["sony_playstation"]);
        assert_eq!(ids("Sony PlayStation 2"), ["sony_playstation2"]);
        assert_eq!(ids("Sony PlayStation"), ["sony_playstation"]);
        assert_eq!(
            ids("Game Boy / Game Boy Color / Game Boy Advance"),
            ["nintendo_gb", "nintendo_gba", "nintendo_gbc"]
        );
        assert_eq!(ids("Nintendo Game Boy Color"), ["nintendo_gbc"]);
        assert_eq!(ids("Microsoft Xbox 360"), ["microsoft_xbox360"]);
        assert_eq!(ids("Xbox"), ["microsoft_xbox"]);
        assert_eq!(ids("Nintendo Wii U"), ["nintendo_wiiu"]);
        assert_eq!(ids("Nintendo Wii (WiiWare)"), ["nintendo_wii"]);
        assert_eq!(ids("Sega Mega Drive / Genesis"), ["sega_genesis"]);
        assert_eq!(ids("PC (MS-DOS / Windows)"), [PC]);
        assert_eq!(ids("Apple Macintosh (classic Mac OS)"), [PC]);
        assert_eq!(ids("Capcom CP System II (arcade)"), ["arcade"]);
        assert!(ids("Unix / Multi-platform").is_empty());
    }

    #[test]
    fn library_platforms_join_the_same_families() {
        assert_eq!(
            family_of_platform("pc_windows"),
            BTreeSet::from([PC.to_owned()])
        );
        assert_eq!(
            family_of_platform("nintendo_n64"),
            BTreeSet::from(["nintendo_n64".to_owned()])
        );
        assert_eq!(
            family_of_platform("Nintendo 64"),
            BTreeSet::from(["nintendo_n64".to_owned()])
        );
    }
}

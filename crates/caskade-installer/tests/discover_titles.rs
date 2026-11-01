//! Literal installer names captured from public upstream user reports and
//! gogcheck's README on 2026-10-26. Only names and provenance URLs are stored;
//! no installers or copyrighted game assets are redistributed. URL-encoded
//! filenames in reports are percent-decoded before recording the literal name.
//! Expected values describe filename fallback labels, not restored marketing
//! punctuation; structured game metadata has higher priority in discovery.

use caskade_installer::discover::clean_title;

const CASES: [(&str, &str, &str); 100] = [
    (
        "setup_medieval_ii_total_war_gog_v1_(86987).exe",
        "Medieval II Total War",
        "https://github.com/dscharrer/innoextract/issues/211",
    ),
    (
        "setup_syberia_the_world_before_1.2.40404_(64bit)_(56798).exe",
        "Syberia The World Before",
        "https://github.com/dscharrer/innoextract/issues/185",
    ),
    (
        "setup_divinity_original_sin_2.5.0.11.exe",
        "Divinity Original Sin",
        "https://github.com/dscharrer/innoextract/issues/163",
    ),
    (
        "setup_the_witcher_3_wild_hunt_2.0.0.28.exe",
        "The Witcher 3 Wild Hunt",
        "https://github.com/dscharrer/innoextract/issues/134",
    ),
    (
        "setup_dungeon_siege_1.11.1_amd_rdna_fix_(52932).exe",
        "Dungeon Siege",
        "https://github.com/Joshspeakman/dungeon-siege-macos/issues/5",
    ),
    (
        "setup_interstate76_2.1.0.17.exe",
        "Interstate 76",
        "https://github.com/therealjkvalentine/i76-everywhere/issues/105",
    ),
    (
        "setup_interstate76_nitro_pack_2.1.0.17.exe",
        "Interstate 76 Nitro Pack",
        "https://github.com/therealjkvalentine/i76-everywhere/issues/105",
    ),
    (
        "setup_tainted_grail_the_fall_of_avalon_-_bonus_content_1_1.25_(92478).exe",
        "Tainted Grail The Fall Of Avalon - Bonus Content 1",
        "https://github.com/arelate/vangogh/issues/220",
    ),
    (
        "setup_tainted_grail_the_fall_of_avalon_-_sanctuary_of_sarras_1.30b_(94289).exe",
        "Tainted Grail The Fall Of Avalon - Sanctuary Of Sarras",
        "https://github.com/arelate/vangogh/issues/220",
    ),
    (
        "setup_tainted_grail_the_fall_of_avalon_1.30b_(64bit)_(94289).exe",
        "Tainted Grail The Fall Of Avalon",
        "https://github.com/arelate/vangogh/issues/220",
    ),
    (
        "setup_grim_dawn_1.3.1.1_(64bit)_(94412).exe",
        "Grim Dawn",
        "https://github.com/arelate/vangogh/issues/220",
    ),
    (
        "setup_carmageddon_2.0.0.63.exe",
        "Carmageddon",
        "https://github.com/armac911/Carmageddon-Android-Port/pull/1",
    ),
    (
        "setup_lords_of_magic_special_edition_3.01_(19308).exe",
        "Lords Of Magic Special Edition",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_lords_of_the_realm_iii_1.01_(19330).exe",
        "Lords Of The Realm III",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_unreal_2_-_the_awakening_special_edition_v2001_(19575).exe",
        "Unreal 2 - The Awakening Special Edition",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_heroes_chronicles_chapter2_2.1.0.43.exe",
        "Heroes Chronicles Chapter 2",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_heroes_chronicles_chapter3_2.1.0.41.exe",
        "Heroes Chronicles Chapter 3",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_heroes_chronicles_chapter4_2.1.0.42.exe",
        "Heroes Chronicles Chapter 4",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_heroes_chronicles_chapter5_2.1.0.42.exe",
        "Heroes Chronicles Chapter 5",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_heroes_chronicles_chapter6_2.1.0.42.exe",
        "Heroes Chronicles Chapter 6",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_heroes_chronicles_chapter7_2.1.0.42.exe",
        "Heroes Chronicles Chapter 7",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_heroes_chronicles_chapter8_2.1.0.42.exe",
        "Heroes Chronicles Chapter 8",
        "https://github.com/hippie68/gogcheck/issues/9",
    ),
    (
        "setup_empire_earth_gold_2.0.0.2974_gog_v3_(78415).exe",
        "Empire Earth Gold",
        "https://github.com/00xJS/Empire-Earth/issues/2",
    ),
    (
        "setup_theme_hospital_v3_(28027).exe",
        "Theme Hospital",
        "https://github.com/boburning/portcove/issues/1950",
    ),
    (
        "setup_terraria_v1.4.4.9_v4_(60319).exe",
        "Terraria",
        "https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/issues/3596",
    ),
    (
        "setup_terroir_1.0_(32bit)_(32337).exe",
        "Terroir",
        "https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/issues/3596",
    ),
    (
        "setup_terroir_1.0_(64bit)_(32337).exe",
        "Terroir",
        "https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/issues/3596",
    ),
    (
        "setup_ziggurat_2018-05-08_(20608).exe",
        "Ziggurat",
        "https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/issues/3596",
    ),
    (
        "setup_colin2005.exe",
        "Colin 2005",
        "https://github.com/akitaonrails/distrobox-gaming/issues/4",
    ),
    (
        "setup_carmageddon_max_pack_1.0_gog_(77629).exe",
        "Carmageddon Max Pack",
        "https://github.com/dethrace-labs/dethrace/issues/1150",
    ),
    (
        "setup_besiege_v1.26-20573_64bit_(71347).exe",
        "Besiege",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_cult_of_the_lamb_1.3.3.206_(70534).exe",
        "Cult Of The Lamb",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_cult_of_the_lamb_cthulhu_follower_form_1.3.3.206_(70534).exe",
        "Cult Of The Lamb Cthulhu Follower Form",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_cult_of_the_lamb_cultist_pack_1.3.3.206_(70534).exe",
        "Cult Of The Lamb Cultist Pack",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_cult_of_the_lamb_heretic_pack_1.3.3.206_(70534).exe",
        "Cult Of The Lamb Heretic Pack",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_cult_of_the_lamb_sinful_pack_1.3.3.206_(70534).exe",
        "Cult Of The Lamb Sinful Pack",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_interstellaria_2.8.0.10.exe",
        "Interstellaria",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_towerfall_ascension_2.5.0.6.exe",
        "Towerfall Ascension",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_towerfall_ascension_dark_world_2.4.0.5.exe",
        "Towerfall Ascension Dark World",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_carmageddon2_2.1.0.28.exe",
        "Carmageddon 2",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_gnomoria_2.0.0.2.exe",
        "Gnomoria",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_no_mans_sky_2.11.0.14.exe",
        "No Mans Sky",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_slime_rancher_2.0.0.1.exe",
        "Slime Rancher",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_butcher_2.4.0.7.exe",
        "Butcher",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_cossacks3_2.5.0.9.exe",
        "Cossacks 3",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_spore_collection_2.0.0.5.exe",
        "Spore Collection",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_prison_architect_2.12.0.16.exe",
        "Prison Architect",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_ftl_-_advanced_edition_1.5.13.exe",
        "FTL - Advanced Edition",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_fallout2_2.1.0.17.exe",
        "Fallout 2",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_stasis_2.0.0.4.exe",
        "Stasis",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_sw_jedi_academy_2.0.0.4.exe",
        "SW Jedi Academy",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_the_witcher_3_wild_hunt_2.0.0.40.exe",
        "The Witcher 3 Wild Hunt",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_kerbal_space_program_2.4.0.6.exe",
        "Kerbal Space Program",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_darkest_dungeon_2.9.0.13.exe",
        "Darkest Dungeon",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_tes_morrowind_goty_2.0.0.7.exe",
        "TES Morrowind GOTY",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_saints_row2_2.1.0.5.exe",
        "Saints Row 2",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_rollercoaster_tycoon3_2.0.0.13.exe",
        "Rollercoaster Tycoon 3",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_fallout2_2.0.0.10.exe",
        "Fallout 2",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_pop_the_two_thrones_2.0.0.5.exe",
        "POP The Two Thrones",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_stronghold_crusader_extreme_hd_2.0.0.6.exe",
        "Stronghold Crusader Extreme HD",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_baldurs_gate_2.0.0.20.exe",
        "Baldurs Gate",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_blade_of_darkness_2.0.0.5.exe",
        "Blade Of Darkness",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_system_shock2_2.0.0.9.exe",
        "System Shock 2",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_hotline_miami_2.0.0.4.exe",
        "Hotline Miami",
        "https://github.com/hippie68/gogcheck/issues/14",
    ),
    (
        "setup_doom3_bfg_edition_1.14_20230201_(62122).exe",
        "Doom 3 BFG Edition",
        "https://github.com/Sude-/lgogdownloader/issues/327",
    ),
    (
        "setup_serious_sam_the_first_encounter_1.05_(21759).exe",
        "Serious Sam The First Encounter",
        "https://github.com/fgsfdsfgs/Serious-Engine/issues/1",
    ),
    (
        "setup_serious_sam_the_second_encounter_1.07_(21759).exe",
        "Serious Sam The Second Encounter",
        "https://github.com/fgsfdsfgs/Serious-Engine/issues/1",
    ),
    (
        "setup_heroes_of_might_and_magic_3_complete_4.0_(3.2)_gog_0.1_(77075).exe",
        "Heroes Of Might And Magic 3 Complete",
        "https://github.com/vcmi/vcmi/issues/7004",
    ),
    (
        "setup_syberia_1.0.0_hotfix4_(53936).exe",
        "Syberia",
        "https://github.com/lutris/lutris/issues/5546",
    ),
    (
        "setup_yakuza_kiwami_2_1.004a_(64bit)_(65330).exe",
        "Yakuza Kiwami 2",
        "https://github.com/Toylerrr/GOGtoGV/pull/7",
    ),
    (
        "setup_rayman_2_gog-1_(17797).exe",
        "Rayman 2",
        "https://github.com/spitfirex86/Ray2Fix/issues/12",
    ),
    (
        "setup_doom2_2.0.0.6.exe",
        "Doom 2",
        "https://github.com/dscharrer/innoextract/pull/53",
    ),
    (
        "setup_olliolli_world_1.0_security_patch_(85641).exe",
        "Olliolli World",
        "https://github.com/lutris/lutris/issues/6858",
    ),
    (
        "setup_delta_force_1.00.03.03p_(10720).exe",
        "Delta Force",
        "https://github.com/x64dbg/x64dbg/issues/2256",
    ),
    (
        "setup_star_trektm_starfleet_command_iii_1.01.534_rdna_fix_(52907).exe",
        "Star Trek Starfleet Command III",
        "https://github.com/jrobio/lutris_installers/issues/1",
    ),
    (
        "setup_beneath_a_steel_sky_1.0_(20270).exe",
        "Beneath A Steel Sky",
        "https://github.com/noosxe/gogo/issues/56",
    ),
    (
        "setup_jill_of_the_jungle_1.0_(24660).exe",
        "Jill Of The Jungle",
        "https://github.com/dscharrer/innoextract/issues/79",
    ),
    (
        "setup_ultima_iv_-_quest_of_the_avatar_1.0_(22309).exe",
        "Ultima IV - Quest Of The Avatar",
        "https://github.com/dscharrer/innoextract/issues/79",
    ),
    (
        "setup_star_warstm_dark_forces_1.0.2_(20338).exe",
        "Star Wars Dark Forces",
        "https://github.com/TheForceEngine/TheForceEngine/issues/391",
    ),
    (
        "setup_warhammer_shadow_of_the_horned_rat_1.0_gog_v5_(90032).exe",
        "Warhammer Shadow Of The Horned Rat",
        "https://github.com/pgrudzien12/openHornedRat/pull/454",
    ),
    (
        "setup_they_are_billions_1.1.4.10_(64bit)_(44651).exe",
        "They Are Billions",
        "https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/issues/5270",
    ),
    (
        "setup_zeus_and_poseidon_2.1.0.10.exe",
        "Zeus And Poseidon",
        "https://github.com/nic0lasm/zeus-poseidon-arm64/issues/134",
    ),
    (
        "setup_little_big_adventure_2_1.0_(28192).exe",
        "Little Big Adventure 2",
        "https://github.com/LBALab/lba2remake/issues/312",
    ),
    (
        "setup_heroes_of_might_and_magic_1.2_(28044).exe",
        "Heroes Of Might And Magic",
        "https://github.com/lutris/agora/issues/5",
    ),
    (
        "setup_vampire_the_masquerade_-_bloodlines_1.2_(up_10.2)_(28160).exe",
        "Vampire The Masquerade - Bloodlines",
        "https://github.com/soulcharmer/gog_bloodlines_cz/issues/490",
    ),
    (
        "setup_nox_1.2_gog_v2_(93536).exe",
        "Nox",
        "https://github.com/sookyboo/nox-decomp/issues/17",
    ),
    (
        "setup_the_witcher_2_-_assassins_of_kings_enhanced_edition_3.5.0.26g_(64109).exe",
        "The Witcher 2 - Assassins Of Kings Enhanced Edition",
        "https://github.com/ValveSoftware/Proton/issues/10309",
    ),
    (
        "setup_fallout_1.2_(27130).exe",
        "Fallout",
        "https://github.com/alexbatalov/fallout1-ce/issues/163",
    ),
    (
        "setup_pharaoh_gold_2.1.0.15.exe",
        "Pharaoh Gold",
        "https://github.com/noosxe/gogo/pull/33",
    ),
    (
        "setup_cyberpunk_2077_1.63_hotfix_(64bit)_(65540).exe",
        "Cyberpunk 2077",
        "https://github.com/Sude-/lgogdownloader/issues/242",
    ),
    (
        "setup_chaos_overlords_2.1.0.17.exe",
        "Chaos Overlords",
        "https://github.com/kibertoad/chaos-overlords-new-chrome/pull/606",
    ),
    (
        "setup_wing_commander_1.01_gog_v1_(76764).exe",
        "Wing Commander",
        "https://github.com/Sude-/lgogdownloader/issues/291",
    ),
    (
        "setup_x-com_terror_from_the_deep_2.1_(28046).exe",
        "X-Com Terror From The Deep",
        "https://github.com/OpenXcom/OpenXcom/issues/1411",
    ),
    (
        "setup_warcraft_ii_2.02_v4_(28734).exe",
        "Warcraft II",
        "https://github.com/Wargus/stratagus/issues/423",
    ),
    (
        "setup_alan_wake_1.1.exe",
        "Alan Wake",
        "https://github.com/lazardjokovic/discwright/pull/83",
    ),
    (
        "setup_baldurs_gate_3_release_-_v4.1.1.5849914_(64bit)_(76068).exe",
        "Baldurs Gate 3",
        "https://github.com/lutris/wine/issues/170",
    ),
    (
        "setup_banished_64_1.0.7_(14938).exe",
        "Banished",
        "https://github.com/sharkwouter/minigalaxy/issues/605",
    ),
    (
        "setup_ftl_advanced_edition_1.6.13b_(36400).exe",
        "FTL Advanced Edition",
        "https://github.com/hippie68/gogcheck#readme",
    ),
    (
        "setup_terraria_v1.4.1.2_(42619).exe",
        "Terraria",
        "https://github.com/hippie68/gogcheck#readme",
    ),
    (
        "setup_the_witcher_adventure_game_1.2.5a_(12082).exe",
        "The Witcher Adventure Game",
        "https://github.com/hippie68/gogcheck#readme",
    ),
];

#[test]
fn one_hundred_observed_store_filenames_clean_to_curated_titles() {
    for (filename, expected, provenance) in CASES {
        assert!(provenance.starts_with("https://"));
        assert_eq!(clean_title(filename), expected, "{filename} ({provenance})");
    }
}

#[test]
fn unicode_platforms_archives_and_title_numbers_are_preserved() {
    for (name, expected) in [
        (
            "setup_hollow_knight_1.5.78.11833_(64bit)_(52133).exe",
            "Hollow Knight",
        ),
        ("Hades_II_windows_x86_64.zip", "Hades II"),
        ("游戏_linux_x86_64.tar.xz", "游戏"),
        ("The Witcher 3.app", "The Witcher 3"),
        ("DOOM_64_Linux.AppImage", "DOOM 64"),
        ("VA-11_HALL-A_v1.0.zip", "VA-11 HALL-A"),
        ("2077.exe", "2077"),
        ("", ""),
    ] {
        assert_eq!(clean_title(name), expected, "{name}");
    }
}

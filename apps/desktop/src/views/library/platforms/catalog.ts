import type { MessageDescriptor } from "@lingui/core";
import { msg, t } from "@lingui/core/macro";
import type { LibrarySort, PlatformSummary } from "../../../api/types";

/** How the library shows a platform: a short title, what kind of machine it
 * is and its hardware render (`art/<id>.webp`). Keyed by the emulation
 * catalogue's platform ids (`crates/caskade-sources/emulation/platforms.toml`). */
interface PlatformLook {
  title: string;
  maker: string;
  kind: string;
  /** Other names the platform goes by: the catalogue's own, regional ones. */
  names: string[];
}

const looks: Record<string, PlatformLook> = {
  nintendo_nes: {
    title: "NES",
    maker: "Nintendo",
    kind: "8-bit console",
    names: ["Nintendo Entertainment System", "Famicom", "Nintendo Famicom"],
  },
  nintendo_snes: {
    title: "SNES",
    maker: "Nintendo",
    kind: "16-bit console",
    names: ["Super Nintendo Entertainment System", "Super Nintendo", "Super Famicom"],
  },
  nintendo_n64: { title: "Nintendo 64", maker: "Nintendo", kind: "64-bit console", names: ["N64"] },
  nintendo_gb: {
    title: "Game Boy",
    maker: "Nintendo",
    kind: "Handheld",
    names: ["Nintendo Game Boy", "GB"],
  },
  nintendo_gbc: {
    title: "Game Boy Color",
    maker: "Nintendo",
    kind: "Handheld",
    names: ["Nintendo Game Boy Color", "GBC"],
  },
  nintendo_gba: {
    title: "Game Boy Advance",
    maker: "Nintendo",
    kind: "Handheld",
    names: ["Nintendo Game Boy Advance", "GBA"],
  },
  nintendo_ds: { title: "Nintendo DS", maker: "Nintendo", kind: "Handheld", names: ["NDS"] },
  nintendo_gamecube: {
    title: "GameCube",
    maker: "Nintendo",
    kind: "6th gen console",
    names: ["Nintendo GameCube", "GC", "NGC"],
  },
  nintendo_wii: {
    title: "Wii",
    maker: "Nintendo",
    kind: "7th gen console",
    names: ["Nintendo Wii"],
  },
  nintendo_wiiu: {
    title: "Wii U",
    maker: "Nintendo",
    kind: "8th gen console",
    names: ["Nintendo Wii U", "WiiU"],
  },
  nintendo_switch: {
    title: "Switch",
    maker: "Nintendo",
    kind: "Hybrid console",
    names: ["Nintendo Switch"],
  },
  sega_mastersystem: {
    title: "Master System",
    maker: "Sega",
    kind: "8-bit console",
    names: ["Sega Master System", "SMS", "Mark III"],
  },
  sega_sg1000: {
    title: "SG-1000",
    maker: "Sega",
    kind: "8-bit console",
    names: ["Sega SG-1000", "SG1000"],
  },
  sega_genesis: {
    title: "Genesis",
    maker: "Sega",
    kind: "16-bit console",
    names: ["Sega Genesis", "Mega Drive", "Sega Mega Drive", "MD"],
  },
  sega_saturn: { title: "Saturn", maker: "Sega", kind: "32-bit console", names: ["Sega Saturn"] },
  sega_dreamcast: {
    title: "Dreamcast",
    maker: "Sega",
    kind: "6th gen console",
    names: ["Sega Dreamcast", "DC"],
  },
  sony_playstation: {
    title: "PlayStation",
    maker: "Sony",
    kind: "32-bit console",
    names: ["Sony PlayStation", "PS1", "PSX", "PS"],
  },
  sony_playstation2: {
    title: "PlayStation 2",
    maker: "Sony",
    kind: "6th gen console",
    names: ["Sony PlayStation 2", "PS2"],
  },
  sony_playstation3: {
    title: "PlayStation 3",
    maker: "Sony",
    kind: "7th gen console",
    names: ["Sony PlayStation 3", "PS3"],
  },
  sony_psp: {
    title: "PSP",
    maker: "Sony",
    kind: "Handheld",
    names: ["Sony PlayStation Portable", "PlayStation Portable"],
  },
  nintendo_3ds: {
    title: "Nintendo 3DS",
    maker: "Nintendo",
    kind: "Handheld",
    names: ["3DS", "N3DS", "New Nintendo 3DS"],
  },
  microsoft_xbox: {
    title: "Xbox",
    maker: "Microsoft",
    kind: "6th gen console",
    names: ["Microsoft Xbox"],
  },
  microsoft_xbox360: {
    title: "Xbox 360",
    maker: "Microsoft",
    kind: "7th gen console",
    names: ["Microsoft Xbox 360", "X360"],
  },
  sony_playstation4: {
    title: "PlayStation 4",
    maker: "Sony",
    kind: "8th gen console",
    names: ["Sony PlayStation 4", "PS4"],
  },
  arcade: { title: "Arcade", maker: "Arcade", kind: "Cabinet", names: ["MAME"] },
  sony_psvita: {
    title: "PS Vita",
    maker: "Sony",
    kind: "Handheld",
    names: ["Sony PlayStation Vita", "PlayStation Vita", "Vita", "PSV"],
  },
  sega_gamegear: {
    title: "Game Gear",
    maker: "Sega",
    kind: "Handheld",
    names: ["Sega Game Gear", "GG"],
  },
  sega_cd: {
    title: "Sega CD",
    maker: "Sega",
    kind: "Console add-on",
    names: ["Mega-CD", "Mega CD", "Sega Mega-CD"],
  },
  sega_32x: { title: "32X", maker: "Sega", kind: "Console add-on", names: ["Sega 32X"] },
  nintendo_virtualboy: {
    title: "Virtual Boy",
    maker: "Nintendo",
    kind: "Stereoscopic console",
    names: ["Nintendo Virtual Boy", "VB"],
  },
  atari_2600: {
    title: "Atari 2600",
    maker: "Atari",
    kind: "2nd gen console",
    names: ["Atari VCS", "2600"],
  },
  atari_5200: {
    title: "Atari 5200",
    maker: "Atari",
    kind: "2nd gen console",
    names: ["Atari 5200 SuperSystem", "5200"],
  },
  atari_7800: {
    title: "Atari 7800",
    maker: "Atari",
    kind: "3rd gen console",
    names: ["Atari 7800 ProSystem", "7800"],
  },
  atari_lynx: { title: "Lynx", maker: "Atari", kind: "Handheld", names: ["Atari Lynx"] },
  atari_jaguar: {
    title: "Jaguar",
    maker: "Atari",
    kind: "64-bit console",
    names: ["Atari Jaguar"],
  },
  nec_pcengine: {
    title: "TurboGrafx-16",
    maker: "NEC",
    kind: "16-bit console",
    names: ["NEC TurboGrafx-16", "PC Engine", "NEC PC Engine", "TurboGrafx-16/PC Engine", "TG16"],
  },
  snk_neogeo_aes: {
    title: "Neo Geo AES",
    maker: "SNK",
    kind: "16-bit console",
    names: ["SNK Neo Geo AES", "SNK Neo Geo", "Neo Geo", "Neo-Geo", "snk_neogeo"],
  },
  snk_neogeopocketcolor: {
    title: "Neo Geo Pocket Color",
    maker: "SNK",
    kind: "Handheld",
    names: ["SNK Neo Geo Pocket Color", "NGPC", "snk_neogeopocket_color"],
  },
  bandai_wonderswancolor: {
    title: "WonderSwan Color",
    maker: "Bandai",
    kind: "Handheld",
    names: ["Bandai WonderSwan Color", "WSC", "bandai_wonderswan_color"],
  },
  coleco_colecovision: {
    title: "ColecoVision",
    maker: "Coleco",
    kind: "2nd gen console",
    names: ["Coleco ColecoVision", "Coleco Vision"],
  },
  mattel_intellivision: {
    title: "Intellivision",
    maker: "Mattel",
    kind: "2nd gen console",
    names: ["Mattel Intellivision"],
  },
  panasonic_3do: {
    title: "3DO",
    maker: "Panasonic",
    kind: "32-bit console",
    names: ["3DO Interactive Multiplayer", "Panasonic 3DO"],
  },
  commodore_64: {
    title: "Commodore 64",
    maker: "Commodore",
    kind: "Home computer",
    names: ["C64", "Commodore C64", "Commodore C64/128/MAX"],
  },
  commodore_amiga: {
    title: "Amiga",
    maker: "Commodore",
    kind: "Home computer",
    names: ["Commodore Amiga"],
  },
  microsoft_msx: {
    title: "MSX",
    maker: "MSX",
    kind: "Home computer",
    names: ["Microsoft MSX"],
  },
  pc_dos: {
    title: "DOS",
    maker: "PC",
    kind: "Home computer",
    names: ["MS-DOS", "PC DOS", "DOSBox"],
  },
  sony_playstation5: {
    title: "PlayStation 5",
    maker: "Sony",
    kind: "9th gen console",
    names: ["Sony PlayStation 5", "PS5"],
  },
  xbox_one: {
    title: "Xbox One",
    maker: "Microsoft",
    kind: "8th gen console",
    names: ["Microsoft Xbox One", "Xbox One S", "Xbox One X", "microsoft_xboxone"],
  },
  xbox_series: {
    title: "Xbox Series",
    maker: "Microsoft",
    kind: "9th gen console",
    names: [
      "Microsoft Xbox Series",
      "Xbox Series X",
      "Xbox Series S",
      "Xbox Series X|S",
      "microsoft_xboxseries",
    ],
  },
  nintendo_switch2: {
    title: "Switch 2",
    maker: "Nintendo",
    kind: "Hybrid console",
    names: ["Nintendo Switch 2", "NSW2"],
  },
  sinclair_zxspectrum: {
    title: "ZX Spectrum",
    maker: "Sinclair",
    kind: "Home computer",
    names: [
      "Sinclair ZX Spectrum",
      "ZX Spectrum 48K",
      "Spectrum",
      "ZX Spectrum +3",
      "sinclair_zxspectrum3",
    ],
  },
  amstrad_cpc: {
    title: "Amstrad CPC",
    maker: "Amstrad",
    kind: "Home computer",
    names: ["CPC", "Amstrad CPC 464", "CPC464"],
  },
  bbcmicro: {
    title: "BBC Micro",
    maker: "Acorn",
    kind: "Home computer",
    names: ["BBC Microcomputer System", "Acorn BBC Micro", "BBC Model B"],
  },
  atari_8bit: {
    title: "Atari 8-bit",
    maker: "Atari",
    kind: "Home computer",
    names: ["Atari 8-bit", "Atari 800", "Atari 800XL", "Atari 400", "Atari XL/XE", "atari800"],
  },
  atari_st: {
    title: "Atari ST",
    maker: "Atari",
    kind: "Home computer",
    names: ["Atari ST/STE", "Atari STE", "Atari 520ST", "Atari 1040ST"],
  },
  apple_2: {
    title: "Apple II",
    maker: "Apple",
    kind: "Home computer",
    names: ["Apple II", "Apple IIe", "Apple 2", "apple_ii"],
  },
  sharp_x68000: {
    title: "X68000",
    maker: "Sharp",
    kind: "Home computer",
    names: ["Sharp X68000", "X68K"],
  },
  nec_pc98: {
    title: "PC-98",
    maker: "NEC",
    kind: "Home computer",
    names: ["NEC PC-98", "PC-9801", "NEC PC-9801", "PC-9800 Series"],
  },
  commodore_vci20: {
    title: "VIC-20",
    maker: "Commodore",
    kind: "Home computer",
    names: ["Commodore VIC20", "Commodore VIC-20", "VIC20", "commodore_vic20"],
  },
  commodore_amiga_cd32: {
    title: "Amiga CD32",
    maker: "Commodore",
    kind: "32-bit console",
    names: ["Commodore Amiga CD32", "Commodore CD32", "CD32"],
  },
  snk_neogeo_cd: {
    title: "Neo Geo CD",
    maker: "SNK",
    kind: "CD console",
    names: ["SNK Neo Geo CD", "Neo-Geo CD", "NGCD"],
  },
  nec_turbografx_cd: {
    title: "TurboGrafx-CD",
    maker: "NEC",
    kind: "CD console",
    names: [
      "NEC TurboGrafx-CD",
      "PC Engine CD",
      "PC Engine CD-ROM2",
      "PC Engine CD-ROM²",
      "Turbo Duo",
      "PC Engine Duo",
      "nec_pcenginecd",
    ],
  },
  bandai_wonderswan: {
    title: "WonderSwan",
    maker: "Bandai",
    kind: "Handheld",
    names: ["Bandai WonderSwan", "WS"],
  },
  snk_neogeopocket: {
    title: "Neo Geo Pocket",
    maker: "SNK",
    kind: "Handheld",
    names: ["SNK Neo Geo Pocket", "NGP"],
  },
  vectrex: {
    title: "Vectrex",
    maker: "GCE",
    kind: "Vector console",
    names: ["GCE Vectrex", "gce_vectrex"],
  },
  magnavox_odyssey_2: {
    title: "Odyssey²",
    maker: "Magnavox",
    kind: "2nd gen console",
    names: [
      "Magnavox Odyssey 2",
      "Magnavox Odyssey²",
      "Odyssey 2",
      "Odyssey2",
      "Philips Videopac G7000",
      "Videopac",
    ],
  },
  nintendo_famicom_disk: {
    title: "Famicom Disk System",
    maker: "Nintendo",
    kind: "Console add-on",
    names: [
      "Nintendo Family Computer Disk System",
      "Nintendo Famicom Disk System",
      "Famicom Disk System",
      "FDS",
    ],
  },
  pokemon_mini: {
    title: "Pokémon mini",
    maker: "Nintendo",
    kind: "Handheld",
    names: [
      "Pokemon Mini",
      "Nintendo Pokémon mini",
      "Nintendo Pokemon Mini",
      "nintendo_pokemonmini",
    ],
  },
  pc_windows: {
    title: "Windows",
    maker: "PC",
    kind: "Desktop computer",
    names: ["PC (Windows)", "Windows PC", "Microsoft Windows", "Win32", "Windows"],
  },
  pc_linux: {
    title: "Linux",
    maker: "PC",
    kind: "Desktop computer",
    names: ["PC (Linux)", "Linux PC", "Linux"],
  },
  macintosh: {
    title: "Macintosh",
    maker: "Apple",
    kind: "Desktop computer",
    names: ["Mac", "macOS", "Mac OS", "Mac OS X", "Apple Macintosh", "PC (Mac)"],
  },
  microsoft_msx2: {
    title: "MSX2",
    maker: "MSX",
    kind: "Home computer",
    names: ["Microsoft MSX2", "MSX 2", "MSX2+", "Microsoft MSX2+", "msx2plus"],
  },
};

const art = import.meta.glob<string>("./art/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
});
const artFor = (id: string) => art[`./art/${id}.webp`];

const key = (name: string) => name.toLowerCase().replace(/[^a-z0-9]+/g, "");
const byName = new Map<string, string>();
for (const [id, look] of Object.entries(looks))
  for (const name of [id, look.title, ...look.names]) byName.set(key(name), id);

export interface PlatformPresentation {
  /** The library's name for it, which the platform filter takes. */
  name: string;
  title: string;
  /** "Nintendo · 8-bit console"; empty for a platform Caskade doesn't know. */
  subtitle: string;
  /** The hardware render, or the neutral platform hardware artwork. */
  art: string | undefined;
  /** Whether `art` is the platform's own. */
  known: boolean;
}

/** The kinds of machine above, translated where they're shown. */
const kindLabels: Record<string, MessageDescriptor> = {
  "2nd gen console": msg({ message: "2nd gen console", context: "platform kind" }),
  "3rd gen console": msg({ message: "3rd gen console", context: "platform kind" }),
  "8-bit console": msg({ message: "8-bit console", context: "platform kind" }),
  "16-bit console": msg({ message: "16-bit console", context: "platform kind" }),
  "32-bit console": msg({ message: "32-bit console", context: "platform kind" }),
  "64-bit console": msg({ message: "64-bit console", context: "platform kind" }),
  "6th gen console": msg({ message: "6th gen console", context: "platform kind" }),
  "7th gen console": msg({ message: "7th gen console", context: "platform kind" }),
  "8th gen console": msg({ message: "8th gen console", context: "platform kind" }),
  "9th gen console": msg({ message: "9th gen console", context: "platform kind" }),
  Cabinet: msg({ message: "Cabinet", context: "platform kind" }),
  "CD console": msg({ message: "CD console", context: "platform kind" }),
  "Console add-on": msg({ message: "Console add-on", context: "platform kind" }),
  "Desktop computer": msg({ message: "Desktop computer", context: "platform kind" }),
  Handheld: msg({ message: "Handheld", context: "platform kind" }),
  "Home computer": msg({ message: "Home computer", context: "platform kind" }),
  "Hybrid console": msg({ message: "Hybrid console", context: "platform kind" }),
  "Stereoscopic console": msg({ message: "Stereoscopic console", context: "platform kind" }),
  "Vector console": msg({ message: "Vector console", context: "platform kind" }),
};

/** How to show the platform the library calls `name`. Unknown platforms
 * keep their name and get neutral platform hardware artwork. */
export function presentPlatform(name: string): PlatformPresentation {
  const id = byName.get(key(name));
  const look = id ? looks[id] : undefined;
  if (!id || !look)
    return { name, title: name, subtitle: "", art: artFor("generic"), known: false };
  const label = kindLabels[look.kind];
  const kind = label ? t(label) : look.kind;
  return {
    name,
    title: look.title,
    subtitle: look.maker === look.title ? kind : `${look.maker} · ${kind}`,
    art: artFor(id) ?? artFor("generic"),
    known: Boolean(artFor(id)),
  };
}

/** The platforms in the library's sort: the last played or the most played
 * first, by title among equals and those never played; by title for the
 * title sort. */
export function sortPlatforms(platforms: PlatformSummary[], sort: LibrarySort) {
  const title = (platform: PlatformSummary) => presentPlatform(platform.name).title;
  const byTitle = (a: PlatformSummary, b: PlatformSummary) =>
    title(a).localeCompare(title(b), undefined, { sensitivity: "base" });
  return platforms.toSorted((a, b) => {
    if (sort === "recent") {
      const order = (b.lastPlayedAt ?? "").localeCompare(a.lastPlayedAt ?? "");
      if (order !== 0) return order;
    } else if (sort === "playtime" && a.playtimeSeconds !== b.playtimeSeconds) {
      return b.playtimeSeconds - a.playtimeSeconds;
    }
    return byTitle(a, b);
  });
}

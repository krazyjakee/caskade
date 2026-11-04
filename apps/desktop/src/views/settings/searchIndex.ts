/** Settings search (#46): what each section holds, as a static table of
 * translatable labels and the other words people look for them by. The
 * search never reads the panels: a label here is written as its panel shows
 * it (same message and context, so it shares the translation), which is also
 * how the panel's row is found and highlighted once the section opens.
 *
 * Add a row here when a panel gains a preference worth finding. Words are one
 * translatable, comma-separated list per entry. */
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import type { SettingsTab } from "../SettingsView";

export interface SettingsSearchEntry {
  tab: SettingsTab;
  /** The sub-section inside the tab (`?section=`) where it lives. */
  section?: string;
  /** The preference or group as its panel labels it. */
  label: MessageDescriptor;
  /** Other words for it, separated by commas. */
  words?: MessageDescriptor;
}

/** Other words for each section as a whole, separated by commas. */
export const sectionWords: Readonly<Record<SettingsTab, MessageDescriptor>> = {
  general: msg({
    message:
      "app, language, region, startup, start, close, tray, minimize, quit, time format, cache, first-run setup, console mode, desktop mode, app updates, Caskade updates, self-update, new version, game updates, update games, GitHub releases",
    comment: "Search words for Settings → General, separated by commas",
  }),
  "library-discovery": msg({
    message:
      "scan, scanning, discovery, find games, detect games, import, launchers, folders, watch folders, hidden games, ignored games, play history, deep scan",
    comment: "Search words for Settings → Library Discovery, separated by commas",
  }),
  launchers: msg({
    message:
      "launchers, launcher discovery, Steam, GOG, Epic, Heroic, Lutris, itch, Battle.net, sources, detect games",
    comment: "Search words for Settings → Launchers, separated by commas",
  }),
  scripts: msg({
    message: "lifecycle scripts, before launch, after exit, hooks, commands, shell",
    comment: "Search words for Settings → Scripts, separated by commas",
  }),
  emulators: msg({
    message:
      "emulators, emulation, ROMs, retro, consoles, platforms, BIOS, firmware, RetroArch, cores, scanners, RomM, profiles",
    comment: "Search words for Settings → Emulation, separated by commas",
  }),
  appearance: msg({
    message:
      "look, theme, colour, color, accent, dark, light, density, card size, scale, zoom, animations, motion, effects, custom CSS, style",
    comment: "Search words for Settings → Appearance, separated by commas",
  }),
  storage: msg({
    message:
      "disk, drive, space, install location, install folder, library folders, move games, default location",
    comment: "Search words for Settings → Storage, separated by commas",
  }),
  installer: msg({
    message: "install, installers, downloads, torrents, magnet links, Wine, Proton, compatibility",
    comment: "Search words for Settings → Installer, separated by commas",
  }),
  plugins: msg({
    message: "add-ons, addons, extensions, catalogue, catalog, themes, permissions",
    comment: "Search words for Settings → Plugins, separated by commas",
  }),
  stores: msg({
    message:
      "stores, Steam, GOG, Epic, sign in, log in, owned games, library import, play time, achievements",
    comment: "Search words for Settings → Store accounts, separated by commas",
  }),
  devices: msg({
    message:
      "sync, pair, pairing, other computers, saves, save games, save sync, cloud saves, save versions, copies, shares, relays, network, downloads from devices",
    comment: "Search words for Settings → Devices, separated by commas",
  }),
  streaming: msg({
    message:
      "stream, remote play, play on TV, Moonlight, Sunshine, host, quality, bitrate, resolution, frame rate",
    comment: "Search words for Settings → Streaming, separated by commas",
  }),
  backup: msg({
    message: "backups, back up, restore, archive, export, snapshot, recovery",
    comment: "Search words for Settings → Backup, separated by commas",
  }),
  accounts: msg({
    message:
      "users, people, family, kids, children, parental controls, age ratings, PIN, permissions, admin",
    comment: "Search words for Settings → Accounts, separated by commas",
  }),
  profile: msg({
    message: "name, avatar, picture, PIN, me, my account",
    comment: "Search words for Settings → My profile, separated by commas",
  }),
  statistics: msg({
    message: "stats, CPU, memory, RAM, network, performance, usage, system monitor",
    comment: "Search words for Settings → Statistics, separated by commas",
  }),
  about: msg({
    message:
      "version, licenses, licences, legal, credits, privacy, terms, open source, IGDB, Hasheous",
    comment: "Search words for Settings → About, separated by commas",
  }),
};

/** The preferences and groups inside the sections. */
export const settingsSearchIndex: readonly SettingsSearchEntry[] = [
  // General
  { tab: "general", label: msg`Language`, words: msg`translation, locale` },
  { tab: "general", label: msg`Time format`, words: msg`clock, 24-hour, 12-hour` },
  { tab: "general", label: msg`Start in`, words: msg`console mode, desktop mode, big picture` },
  { tab: "general", label: msg`Launch on system startup`, words: msg`autostart, login` },
  { tab: "general", label: msg`Start minimized` },
  { tab: "general", label: msg`Open to`, words: msg`first view, home page` },
  { tab: "general", label: msg`When a game launches`, words: msg`minimize, hide window` },
  { tab: "general", label: msg`Close button`, words: msg`tray, quit, close to tray` },
  { tab: "general", label: msg`Confirm before quitting` },
  { tab: "general", label: msg`Cache folder` },
  {
    tab: "general",
    label: msg`Fetch artwork and game info online`,
    words: msg`metadata, IGDB, covers, offline`,
  },
  { tab: "general", label: msg`First-run setup`, words: msg`onboarding, welcome` },
  {
    tab: "general",
    label: msg`Caskade updates`,
    words: msg({
      message: "app updates, update Caskade, new version, self-update",
      comment: "Search words for Caskade's own updates, separated by commas",
    }),
  },
  // Library Discovery
  { tab: "library-discovery", label: msg`Automatic Library Discovery` },
  { tab: "library-discovery", label: msg`Enable library discovery`, words: msg`scan, find games` },
  { tab: "library-discovery", label: msg`Full scan on startup` },
  { tab: "library-discovery", label: msg`Background refresh`, words: msg`rescan` },
  { tab: "library-discovery", label: msg`Watch library folders` },
  {
    tab: "library-discovery",
    label: msg`Import existing play history`,
    words: msg`play time, playtime`,
  },
  {
    tab: "library-discovery",
    label: msg`Operating-system checks`,
    words: msg`desktop entries, Flatpak, Start Menu, Wine, Proton`,
  },
  { tab: "library-discovery", label: msg`Optional deep scans` },
  {
    tab: "library-discovery",
    label: msg`Folder to search`,
    words: msg`scan folder, custom folder`,
  },
  { tab: "library-discovery", label: msg`Ignored results` },
  { tab: "library-discovery", label: msg`Hidden games`, words: msg`unhide` },
  // Launchers
  {
    tab: "launchers",
    label: msg`Launchers`,
    words: msg`launchers, Steam, GOG, Epic, Heroic, Lutris`,
  },
  // Scripts
  { tab: "scripts", label: msg`Global scripts`, words: msg`before launch, after exit` },
  // Emulation
  { tab: "emulators", label: msg`Desktop full screen`, words: msg`fullscreen` },
  {
    tab: "emulators",
    section: "emulators",
    label: msg({ message: "Emulators", context: "emulation section" }),
    words: msg`profiles, arguments`,
  },
  {
    tab: "emulators",
    section: "browse",
    label: msg`Get emulators`,
    words: msg`install emulators, download emulators, RetroArch`,
  },
  {
    tab: "emulators",
    section: "platforms",
    label: msg({ message: "Platforms", context: "emulation section" }),
    words: msg`BIOS, firmware, default emulator, consoles`,
  },
  {
    tab: "emulators",
    section: "scanners",
    label: msg`ROM Scanners`,
    words: msg`ROM folders, scan ROMs`,
  },
  { tab: "emulators", section: "scanners", label: msg`Global exclusions` },
  { tab: "emulators", section: "romm", label: msg`RomM Servers`, words: msg`RomM` },
  { tab: "emulators", section: "romm", label: msg`Download Cache` },
  // Appearance
  { tab: "appearance", label: msg`Theme`, words: msg`dark, light, skin` },
  { tab: "appearance", label: msg`Accent colour`, words: msg`color, colour` },
  { tab: "appearance", label: msg`Density`, words: msg`compact, comfortable` },
  { tab: "appearance", label: msg`Card size` },
  { tab: "appearance", label: msg`Interface scale`, words: msg`zoom, text size, bigger` },
  { tab: "appearance", label: msg`Show titles on artwork` },
  { tab: "appearance", label: msg`Animations`, words: msg`motion` },
  { tab: "appearance", label: msg`Reduce effects`, words: msg`performance, motion` },
  { tab: "appearance", label: msg`Custom CSS`, words: msg`style, stylesheet` },
  // Storage
  { tab: "storage", label: msg`Storage locations`, words: msg`drives, disks, folders` },
  { tab: "storage", label: msg`Install location preference`, words: msg`default location` },
  { tab: "storage", label: msg`Folder-name template` },
  // Installer
  {
    tab: "installer",
    label: msg({ message: "Compatibility runner", context: "settings" }),
    words: msg`Wine, Proton, umu`,
  },
  { tab: "installer", label: msg({ message: "Concurrent downloads", context: "settings" }) },
  { tab: "installer", label: msg({ message: "Keep downloads", context: "settings" }) },
  {
    tab: "installer",
    label: msg({ message: "Windows install method", context: "settings" }),
  },
  { tab: "installer", label: msg({ message: "Running installers", context: "settings" }) },
  {
    tab: "installer",
    label: msg`Magnet links and torrent files`,
    words: msg`torrents, magnet, BitTorrent`,
  },
  {
    tab: "general",
    label: msg`Game updates`,
    words: msg({
      message: "update games, game patches, GitHub releases",
      comment: "Search words for updates to installed games, separated by commas",
    }),
  },
  {
    tab: "general",
    label: msg({ message: "Check for game updates", context: "settings" }),
  },
  {
    tab: "general",
    label: msg({ message: "Offer updates before playing", context: "settings" }),
  },
  // Plugins
  { tab: "plugins", label: msg`Installed plugins`, words: msg`add-ons, extensions` },
  // Store accounts
  { tab: "stores", label: msg`Refresh in the background` },
  { tab: "stores", label: msg`Import play time`, words: msg`playtime, hours played` },
  { tab: "stores", label: msg`Games that leave the library` },
  // Devices
  { tab: "devices", label: msg`This device`, words: msg`device name` },
  { tab: "devices", label: msg`Your devices`, words: msg`paired devices` },
  { tab: "devices", label: msg`Pair a device`, words: msg`pairing, link, add device` },
  { tab: "devices", label: msg`Saves`, words: msg`save games, cloud saves, save sync` },
  { tab: "devices", label: msg`Versions to keep`, words: msg`save versions, save history` },
  {
    tab: "devices",
    label: msg`Connect only directly`,
    words: msg`network, internet, relay, server`,
  },
  { tab: "devices", label: msg`Downloads from my devices`, words: msg`peer downloads` },
  { tab: "devices", label: msg`Accept copies from my devices` },
  { tab: "devices", label: msg`Play requests from my devices`, words: msg`remote play` },
  { tab: "devices", label: msg`Copies and shares`, words: msg`share a game` },
  // Streaming
  {
    tab: "streaming",
    label: msg`Let other devices play`,
    words: msg`host, hosting, Sunshine`,
  },
  {
    tab: "streaming",
    label: msg`Play from other devices`,
    words: msg`client, Moonlight, pair`,
  },
  { tab: "streaming", label: msg`Stream quality`, words: msg`bitrate, resolution, frame rate` },
  { tab: "streaming", label: msg`Streaming updates` },
  { tab: "streaming", label: msg`Add a device by its address`, words: msg`IP address` },
  // Backup
  { tab: "backup", label: msg`Back up`, words: msg`backup now` },
  { tab: "backup", label: msg`Backup folder` },
  {
    tab: "backup",
    label: msg`Automatic backups`,
    words: msg`schedule, daily, weekly, backups to keep`,
  },
  { tab: "backup", label: msg`Restore`, words: msg`restore a backup, recover` },
  // Accounts
  { tab: "accounts", label: msg`Age ratings`, words: msg`parental controls, PEGI, ESRB` },
  { tab: "accounts", label: msg`Rating board` },
  // My profile
  { tab: "profile", label: msg({ message: "Name", context: "account" }) },
  { tab: "profile", label: msg`Avatar`, words: msg`picture` },
  { tab: "profile", label: msg`PIN`, words: msg`password, lock` },
  // Statistics
  { tab: "statistics", label: msg`CPU usage` },
  { tab: "statistics", label: msg`Memory used`, words: msg`RAM` },
  { tab: "statistics", label: msg`Network traffic` },
  // About
  { tab: "about", label: msg`Credits` },
  { tab: "about", section: "licenses", label: msg`Licenses`, words: msg`licences` },
  { tab: "about", section: "packages", label: msg`Open-source packages` },
  { tab: "about", section: "terms", label: msg`Terms and conditions` },
  { tab: "about", section: "privacy", label: msg`Privacy policy` },
];

/** Lower case without accents, so "Écran" finds "ecran". */
export function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/** Whether every word of `query` appears somewhere in `texts`. */
export function matches(query: string, texts: readonly string[]): boolean {
  const terms = fold(query).split(/\s+/).filter(Boolean);
  if (terms.length === 0) return false;
  const haystack = texts.map(fold).join("\n");
  return terms.every((term) => haystack.includes(term));
}

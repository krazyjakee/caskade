//! Settings search (#46, ported in #103): what each section holds, as a
//! static table of translatable labels and the other words people look for
//! them by (the webview's `searchIndex.ts`, entry for entry, with the same
//! messages and contexts, so they share translations). The search never
//! reads the panels: a label here is written as its panel shows it, which
//! is also how the panel's row is found and highlighted once the section
//! opens (`ui::Found`).
//!
//! Add a row here when a panel gains a preference worth finding. Words are
//! one translatable, comma-separated list per entry.

use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::{SECTIONS, Section};
use crate::ui::interact::{Keys, use_interaction};
use crate::ui::{SearchField, fold};

/// A preference or group inside a section.
#[derive(Clone, Copy)]
pub struct Entry {
    pub tab: &'static str,
    /// The sub-section inside the tab (`?section=`) where it lives.
    pub section: Option<&'static str>,
    /// The preference or group as its panel labels it.
    pub label: fn() -> String,
    /// Other words for it, separated by commas.
    pub words: Option<fn() -> String>,
}

/// Whether every word of `query` appears somewhere in `texts`.
pub fn matches(query: &str, texts: &[&str]) -> bool {
    let query = fold(query);
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return false;
    }
    let haystack = texts
        .iter()
        .map(|text| fold(text))
        .collect::<Vec<_>>()
        .join("\n");
    terms.iter().all(|term| haystack.contains(term))
}

/// A search result: a section, or a preference in one.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub key: String,
    pub tab: &'static str,
    pub section: Option<&'static str>,
    /// The preference's label as its panel shows it; none for a section.
    pub label: Option<String>,
    /// What the result shows: the section's or the preference's name.
    pub title: String,
    /// The section a preference is in.
    pub within: Option<String>,
}

/// The most results shown.
const MOST: usize = 30;

/// The results for `query` among `sections` (the ones this account sees):
/// sections first, then preferences, each those whose name matches before
/// those found by another word.
pub fn search_settings(query: &str, sections: &[&Section]) -> Vec<SearchResult> {
    let mut found: Vec<(SearchResult, bool)> = sections
        .iter()
        .filter_map(|section| {
            let title = (section.label)();
            let words = section_words(section.id);
            if !matches(query, &[&title, &words]) {
                return None;
            }
            let named = matches(query, &[&title]);
            Some((
                SearchResult {
                    key: section.id.to_owned(),
                    tab: section.id,
                    section: None,
                    label: None,
                    title,
                    within: None,
                },
                named,
            ))
        })
        .collect();
    let mut preferences: Vec<(SearchResult, bool)> = INDEX
        .iter()
        .filter_map(|entry| {
            let within = sections.iter().find(|section| section.id == entry.tab)?;
            let label = (entry.label)();
            let words = entry.words.map(|words| words()).unwrap_or_default();
            if !matches(query, &[&label, &words]) {
                return None;
            }
            let named = matches(query, &[&label]);
            Some((
                SearchResult {
                    key: format!("{}\n{}\n{label}", entry.tab, entry.section.unwrap_or("")),
                    tab: entry.tab,
                    section: entry.section,
                    label: Some(label.clone()),
                    title: label,
                    within: Some((within.label)()),
                },
                named,
            ))
        })
        .collect();
    // Stable: names first, the table's order within each.
    found.sort_by_key(|(_, named)| !named);
    preferences.sort_by_key(|(_, named)| !named);
    found
        .into_iter()
        .chain(preferences)
        .map(|(result, _)| result)
        .take(MOST)
        .collect()
}

/// The search above the section list: a field, and in place of the list
/// while it has a query, the results. Enter opens the first, Escape clears
/// it; a pad types in it on the on-screen keyboard like any other field.
#[component]
pub fn SettingsSearch(
    query: String,
    on_query: EventHandler<String>,
    sections: Vec<&'static str>,
    on_open: EventHandler<SearchResult>,
) -> Element {
    let shown: Vec<&Section> = SECTIONS
        .iter()
        .filter(|section| sections.contains(&section.id))
        .collect();
    let results = if query.trim().is_empty() {
        Vec::new()
    } else {
        search_settings(&query, &shown)
    };
    let first = results.first().cloned();
    rsx! {
        div { class: "flex min-h-0 flex-col gap-2 px-2 pt-2",
            SearchField {
                value: query.clone(),
                on_input: move |value: String| on_query.call(value),
                label: t!("Search settings"),
                placeholder: t!("Search settings"),
                on_submit: move |_| {
                    if let Some(first) = first.clone() {
                        on_open.call(first);
                    }
                },
            }
            if !query.trim().is_empty() {
                if results.is_empty() {
                    p { role: "status", class: "px-1 text-sm text-fg-muted",
                        {t!("No settings match “{query}”.", query = query.clone())}
                    }
                } else {
                    ul {
                        "aria-label": t!("Settings search results"),
                        class: "-mx-1 flex min-h-0 flex-col gap-1 overflow-y-auto overscroll-contain p-1",
                        for result in results {
                            li { key: "{result.key}",
                                ResultButton { result, on_open }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ResultButton(result: SearchResult, on_open: EventHandler<SearchResult>) -> Element {
    let interaction = use_interaction();
    let name = match &result.within {
        Some(within) => t!(
            note = "A settings search result: a preference and its section",
            "{title} in {section}",
            title = result.title.clone(),
            section = within.clone()
        ),
        None => result.title.clone(),
    };
    let open = {
        let result = result.clone();
        use_callback(move |()| on_open.call(result.clone()))
    };
    rsx! {
        button {
            r#type: "button",
            "aria-label": "{name}",
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            class: "flex w-full cursor-pointer flex-col items-start rounded-control border border-transparent px-3 py-2 text-left text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) outline-none data-focus-visible:focus-ring data-hovered:bg-surface-raised data-hovered:text-fg",
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onclick: move |_| open(()),
            onkeydown: move |event: KeyboardEvent| {
                if event.key() == Key::Named(NamedKey::Escape) {
                    return;
                }
                interaction.key_down(&event, Keys::EnterAndSpace, || open(()));
            },
            onkeyup: move |event| interaction.key_up(&event, || open(())),
            span { class: "min-w-0 break-words text-fg", "{result.title}" }
            if let Some(within) = &result.within {
                span { "aria-hidden": "true", class: "text-xs text-fg-muted", "{within}" }
            }
        }
    }
}

/// Other words for each section as a whole, separated by commas.
pub fn section_words(tab: &str) -> String {
    match tab {
        "general" => t!(
            note = "Search words for Settings → General, separated by commas",
            "app, language, region, startup, start, close, tray, minimize, quit, time format, cache, first-run setup, console mode, desktop mode, app updates, Caskade updates, self-update, new version, game updates, update games, GitHub releases"
        ),
        "library-discovery" => t!(
            note = "Search words for Settings → Library Discovery, separated by commas",
            "scan, scanning, discovery, find games, detect games, import, launchers, folders, watch folders, hidden games, ignored games, play history, deep scan"
        ),
        "launchers" => t!(
            note = "Search words for Settings → Launchers, separated by commas",
            "launchers, launcher discovery, Steam, GOG, Epic, Heroic, Lutris, itch, Battle.net, sources, detect games"
        ),
        "scripts" => t!(
            note = "Search words for Settings → Scripts, separated by commas",
            "lifecycle scripts, before launch, after exit, hooks, commands, shell"
        ),
        "emulators" => t!(
            note = "Search words for Settings → Emulation, separated by commas",
            "emulators, emulation, ROMs, retro, consoles, platforms, BIOS, firmware, RetroArch, cores, scanners, RomM, profiles"
        ),
        "appearance" => t!(
            note = "Search words for Settings → Appearance, separated by commas",
            "look, theme, colour, color, accent, dark, light, density, card size, scale, zoom, animations, motion, effects, custom CSS, style"
        ),
        "storage" => t!(
            note = "Search words for Settings → Storage, separated by commas",
            "disk, drive, space, install location, install folder, library folders, move games, default location"
        ),
        "installer" => t!(
            note = "Search words for Settings → Installer, separated by commas",
            "install, installers, downloads, torrents, magnet links, Wine, Proton, compatibility"
        ),
        "plugins" => t!(
            note = "Search words for Settings → Plugins, separated by commas",
            "add-ons, addons, extensions, catalogue, catalog, themes, permissions, uninstall, remove"
        ),
        "stores" => t!(
            note = "Search words for Settings → Store accounts, separated by commas",
            "stores, Steam, GOG, Epic, sign in, log in, owned games, library import, play time, achievements"
        ),
        "devices" => t!(
            note = "Search words for Settings → Devices, separated by commas",
            "sync, pair, pairing, other computers, saves, save games, save sync, cloud saves, save versions, copies, shares, relays, network, downloads from devices"
        ),
        "streaming" => t!(
            note = "Search words for Settings → Streaming, separated by commas",
            "stream, remote play, play on TV, Moonlight, Sunshine, host, quality, bitrate, resolution, frame rate"
        ),
        "backup" => t!(
            note = "Search words for Settings → Backup, separated by commas",
            "backups, back up, restore, archive, export, snapshot, recovery"
        ),
        "accounts" => t!(
            note = "Search words for Settings → Accounts, separated by commas",
            "users, people, family, kids, children, parental controls, age ratings, PIN, permissions, admin"
        ),
        "profile" => t!(
            note = "Search words for Settings → My profile, separated by commas",
            "name, avatar, picture, PIN, me, my account"
        ),
        "statistics" => t!(
            note = "Search words for Settings → Statistics, separated by commas",
            "stats, CPU, memory, RAM, network, performance, usage, system monitor"
        ),
        "about" => t!(
            note = "Search words for Settings → About, separated by commas",
            "version, licenses, licences, legal, credits, privacy, terms, open source, IGDB, Hasheous"
        ),
        _ => String::new(),
    }
}

/// The preferences and groups inside the sections.
pub const INDEX: [Entry; 125] = [
    Entry {
        tab: "general",
        section: None,
        label: || t!("Language"),
        words: Some(|| t!("translation, locale")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Time format"),
        words: Some(|| t!("clock, 24-hour, 12-hour")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Start in"),
        words: Some(|| t!("console mode, desktop mode, big picture")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Launch on system startup"),
        words: Some(|| t!("autostart, login")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Start minimized"),
        words: None,
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Open to"),
        words: Some(|| t!("first view, home page")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("When a game launches"),
        words: Some(|| t!("minimize, hide window")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Close button"),
        words: Some(|| t!("tray, quit, close to tray")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Confirm before quitting"),
        words: None,
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Cache folder"),
        words: None,
    },
    Entry {
        tab: "general",
        section: None,
        label: || tp!("settings", "Check every (hours)"),
        words: Some(|| t!("update interval")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Fetch artwork and game info online"),
        words: Some(|| t!("metadata, IGDB, covers, offline")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Show ports of games"),
        words: Some(|| t!("decompilation, recompilation, source port, Decomp Buddy")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("First-run setup"),
        words: Some(|| t!("onboarding, welcome")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Caskade updates"),
        words: Some(|| {
            t!(
                note = "Search words for Caskade's own updates, separated by commas",
                "app updates, update Caskade, new version, self-update"
            )
        }),
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Automatic Library Discovery"),
        words: None,
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Enable library discovery"),
        words: Some(|| t!("scan, find games")),
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Full scan on startup"),
        words: None,
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Background refresh"),
        words: Some(|| t!("rescan")),
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Watch library folders"),
        words: None,
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Import existing play history"),
        words: Some(|| t!("play time, playtime")),
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Operating-system checks"),
        words: Some(|| t!("desktop entries, Flatpak, Start Menu, Wine, Proton")),
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Optional deep scans"),
        words: None,
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Custom folders"),
        words: None,
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("External drives"),
        words: Some(|| t!("USB, removable")),
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Portable executables"),
        words: None,
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Wine and Proton prefixes"),
        words: None,
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Full disk scan"),
        words: None,
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Folder to search"),
        words: Some(|| t!("scan folder, custom folder")),
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Ignored results"),
        words: None,
    },
    Entry {
        tab: "library-discovery",
        section: None,
        label: || t!("Hidden games"),
        words: Some(|| t!("unhide")),
    },
    Entry {
        tab: "launchers",
        section: None,
        label: || t!("Launchers"),
        words: Some(|| t!("launchers, Steam, GOG, Epic, Heroic, Lutris")),
    },
    Entry {
        tab: "scripts",
        section: None,
        label: || t!("Global scripts"),
        words: Some(|| t!("before launch, after exit")),
    },
    Entry {
        tab: "emulators",
        section: None,
        label: || t!("Desktop full screen"),
        words: Some(|| t!("fullscreen")),
    },
    Entry {
        tab: "emulators",
        section: Some("emulators"),
        label: || tp!("emulation section", "Emulators"),
        words: Some(|| t!("profiles, arguments")),
    },
    Entry {
        tab: "emulators",
        section: Some("browse"),
        label: || t!("Get emulators"),
        words: Some(|| t!("install emulators, download emulators, RetroArch")),
    },
    Entry {
        tab: "emulators",
        section: Some("platforms"),
        label: || tp!("emulation section", "Platforms"),
        words: Some(|| t!("BIOS, firmware, default emulator, consoles")),
    },
    Entry {
        tab: "emulators",
        section: Some("scanners"),
        label: || t!("ROM scanners"),
        words: Some(|| t!("ROM folders, scan ROMs")),
    },
    Entry {
        tab: "emulators",
        section: Some("scanners"),
        label: || t!("Global exclusions"),
        words: None,
    },
    Entry {
        tab: "emulators",
        section: Some("romm"),
        label: || t!("RomM servers"),
        words: Some(|| t!("RomM")),
    },
    Entry {
        tab: "emulators",
        section: Some("romm"),
        label: || t!("Download cache"),
        words: None,
    },
    Entry {
        tab: "emulators",
        section: Some("romm"),
        label: || t!("Size limit"),
        words: Some(|| t!("cache size")),
    },
    Entry {
        tab: "emulators",
        section: Some("romm"),
        label: || t!("Verify downloads"),
        words: Some(|| t!("checksum")),
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Theme"),
        words: Some(|| t!("dark, light, skin")),
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Accent colour"),
        words: Some(|| t!("color, colour")),
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Density"),
        words: Some(|| t!("compact, comfortable")),
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Card size"),
        words: None,
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Interface scale"),
        words: Some(|| t!("zoom, text size, bigger")),
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Show titles on artwork"),
        words: None,
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Animations"),
        words: Some(|| t!("motion")),
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Reduce effects"),
        words: Some(|| t!("performance, motion")),
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Custom CSS"),
        words: Some(|| t!("style, stylesheet")),
    },
    Entry {
        tab: "appearance",
        section: None,
        label: || t!("Disable custom CSS"),
        words: None,
    },
    Entry {
        tab: "storage",
        section: None,
        label: || t!("Storage locations"),
        words: Some(|| t!("drives, disks, folders")),
    },
    Entry {
        tab: "storage",
        section: None,
        label: || t!("Install location preference"),
        words: Some(|| t!("default location")),
    },
    Entry {
        tab: "storage",
        section: None,
        label: || t!("Folder-name template"),
        words: None,
    },
    Entry {
        tab: "storage",
        section: None,
        label: || t!("Always use the default location"),
        words: Some(|| t!("install picker")),
    },
    Entry {
        tab: "installer",
        section: None,
        label: || tp!("settings", "Compatibility runner"),
        words: Some(|| t!("Wine, Proton, umu")),
    },
    Entry {
        tab: "installer",
        section: None,
        label: || tp!("settings", "Concurrent downloads"),
        words: None,
    },
    Entry {
        tab: "installer",
        section: None,
        label: || tp!("settings", "Keep downloads"),
        words: None,
    },
    Entry {
        tab: "installer",
        section: None,
        label: || tp!("settings", "Windows install method"),
        words: None,
    },
    Entry {
        tab: "installer",
        section: None,
        label: || tp!("settings", "Running installers"),
        words: None,
    },
    Entry {
        tab: "installer",
        section: None,
        label: || t!("Magnet links and torrent files"),
        words: Some(|| t!("torrents, magnet, BitTorrent")),
    },
    Entry {
        tab: "installer",
        section: None,
        label: || t!("Open them with Caskade"),
        words: Some(|| t!("default torrent app")),
    },
    Entry {
        tab: "general",
        section: None,
        label: || t!("Game updates"),
        words: Some(|| {
            t!(
                note = "Search words for updates to installed games, separated by commas",
                "update games, game patches, GitHub releases"
            )
        }),
    },
    Entry {
        tab: "general",
        section: None,
        label: || tp!("settings", "Check for game updates"),
        words: None,
    },
    Entry {
        tab: "general",
        section: None,
        label: || tp!("settings", "Offer updates before playing"),
        words: None,
    },
    Entry {
        tab: "plugins",
        section: None,
        label: || t!("Installed plugins"),
        words: Some(|| t!("add-ons, extensions")),
    },
    Entry {
        tab: "stores",
        section: None,
        label: || t!("Refresh in the background"),
        words: None,
    },
    Entry {
        tab: "stores",
        section: None,
        label: || t!("How often"),
        words: Some(|| t!("refresh interval")),
    },
    Entry {
        tab: "stores",
        section: None,
        label: || t!("Import play time"),
        words: Some(|| t!("playtime, hours played")),
    },
    Entry {
        tab: "stores",
        section: None,
        label: || t!("Games that leave the library"),
        words: None,
    },
    Entry {
        tab: "stores",
        section: None,
        label: || t!("Show achievements"),
        words: None,
    },
    Entry {
        tab: "stores",
        section: None,
        label: || t!("Notify new unlocks"),
        words: Some(|| t!("achievement notifications")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("This device"),
        words: Some(|| t!("device name")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Your devices"),
        words: Some(|| t!("paired devices")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Pair a device"),
        words: Some(|| t!("pairing, link, add device")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Saves"),
        words: Some(|| t!("save games, cloud saves, save sync")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Versions to keep"),
        words: Some(|| t!("save versions, save history")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Connect only directly"),
        words: Some(|| t!("network, internet, relay, server")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Downloads from my devices"),
        words: Some(|| t!("peer downloads")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Accept copies from my devices"),
        words: None,
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Play requests from my devices"),
        words: Some(|| t!("remote play")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Copies and shares"),
        words: Some(|| t!("share a game")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Library"),
        words: Some(|| t!("sync")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Play history"),
        words: Some(|| t!("sync, play time")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Accounts and PINs"),
        words: Some(|| t!("sync")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Your artwork"),
        words: Some(|| t!("sync, covers")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Achievement links you made"),
        words: Some(|| t!("sync")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("RomM servers"),
        words: Some(|| t!("sync")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Appearance"),
        words: Some(|| t!("sync, theme")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Launch actions"),
        words: Some(|| t!("sync")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Game scripts"),
        words: Some(|| t!("sync")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Preferred emulators"),
        words: Some(|| t!("sync")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Emulator configuration"),
        words: Some(|| t!("sync, RetroArch config")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Plugins and themes"),
        words: Some(|| t!("sync, add-ons")),
    },
    Entry {
        tab: "devices",
        section: None,
        label: || t!("Save locations list"),
        words: Some(|| t!("save manifest")),
    },
    Entry {
        tab: "streaming",
        section: None,
        label: || t!("Let other devices play"),
        words: Some(|| t!("host, hosting, Sunshine")),
    },
    Entry {
        tab: "streaming",
        section: None,
        label: || t!("Play from other devices"),
        words: Some(|| t!("client, Moonlight, pair")),
    },
    Entry {
        tab: "streaming",
        section: None,
        label: || t!("Stream quality"),
        words: Some(|| t!("bitrate, resolution, frame rate")),
    },
    Entry {
        tab: "streaming",
        section: None,
        label: || t!("Streaming updates"),
        words: None,
    },
    Entry {
        tab: "streaming",
        section: None,
        label: || t!("Add a device by its address"),
        words: Some(|| t!("IP address")),
    },
    Entry {
        tab: "streaming",
        section: None,
        label: || t!("Let other devices stream games from this one"),
        words: Some(|| t!("host")),
    },
    Entry {
        tab: "streaming",
        section: None,
        label: || t!("Resolution"),
        words: None,
    },
    Entry {
        tab: "streaming",
        section: None,
        label: || t!("Frame rate"),
        words: Some(|| t!("fps")),
    },
    Entry {
        tab: "streaming",
        section: None,
        label: || t!("Bitrate"),
        words: None,
    },
    Entry {
        tab: "backup",
        section: None,
        label: || t!("Back up"),
        words: Some(|| t!("backup now")),
    },
    Entry {
        tab: "backup",
        section: None,
        label: || t!("Backup folder"),
        words: None,
    },
    Entry {
        tab: "backup",
        section: None,
        label: || t!("Back up automatically"),
        words: Some(|| t!("daily, weekly, schedule")),
    },
    Entry {
        tab: "backup",
        section: None,
        label: || t!("Automatic backups to keep"),
        words: Some(|| t!("retention")),
    },
    Entry {
        tab: "backup",
        section: None,
        label: || t!("Automatic backups"),
        words: Some(|| t!("schedule, daily, weekly, backups to keep")),
    },
    Entry {
        tab: "backup",
        section: None,
        label: || t!("Restore"),
        words: Some(|| t!("restore a backup, recover")),
    },
    Entry {
        tab: "accounts",
        section: None,
        label: || t!("Age ratings"),
        words: Some(|| t!("parental controls, PEGI, ESRB")),
    },
    Entry {
        tab: "accounts",
        section: None,
        label: || t!("Rating board"),
        words: None,
    },
    Entry {
        tab: "profile",
        section: None,
        label: || tp!("account", "Name"),
        words: None,
    },
    Entry {
        tab: "profile",
        section: None,
        label: || t!("Avatar"),
        words: Some(|| t!("picture")),
    },
    Entry {
        tab: "profile",
        section: None,
        label: || t!("PIN"),
        words: Some(|| t!("password, lock")),
    },
    Entry {
        tab: "statistics",
        section: None,
        label: || t!("CPU usage"),
        words: None,
    },
    Entry {
        tab: "statistics",
        section: None,
        label: || t!("Memory used"),
        words: Some(|| t!("RAM")),
    },
    Entry {
        tab: "statistics",
        section: None,
        label: || t!("Network traffic"),
        words: None,
    },
    Entry {
        tab: "about",
        section: None,
        label: || t!("Credits"),
        words: None,
    },
    Entry {
        tab: "about",
        section: Some("licenses"),
        label: || t!("Licenses"),
        words: Some(|| t!("licences")),
    },
    Entry {
        tab: "about",
        section: Some("packages"),
        label: || t!("Open-source packages"),
        words: None,
    },
    Entry {
        tab: "about",
        section: Some("terms"),
        label: || t!("Terms and conditions"),
        words: None,
    },
    Entry {
        tab: "about",
        section: Some("privacy"),
        label: || t!("Privacy policy"),
        words: None,
    },
];

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

mod looks;
use looks::{GENERIC_ART, LOOKS, Look};

/// The host `caskade-asset://` serves the renders under.
pub const ART_HOST: &str = "platforms";

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

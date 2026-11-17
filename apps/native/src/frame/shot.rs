//! The `shell` reference screen (#96): the real frame, over the fixture
//! library, open on the library's grid, and `home` (#98), open on Home, for
//! the theme screenshots (`crate::shots`). Their data loads after the first
//! render, so a shot waits until the cards are there ([`ready`],
//! [`home_ready`]).
//!
//! The website's captures (#150) are Home over a sample library of their
//! own ([`open_website`]): eight games with Steam's public cover art, which
//! `scripts/website-art.sh` downloads beforehand.

use crate::data::{DataSource, use_data_source};
use crate::input::InputRoot;
use blitz_dom::{BaseDocument, LocalName};
use caskade_ui_data::fixtures::FixtureGame;
use dioxus_native::prelude::*;
use std::path::Path;
use std::sync::OnceLock;

/// The fixture library the screen shows: the webview's 24 plain games and
/// the emulated ones.
static SOURCE: OnceLock<DataSource> = OnceLock::new();

/// The website's sample library, once [`open_website`] has made it.
static WEBSITE: OnceLock<DataSource> = OnceLock::new();

/// The website's sample games, as the retired webview capture had them:
/// title, Steam app id (its cover is `<id>.jpg` in the art folder) and
/// genre.
pub const WEBSITE_GAMES: [(&str, u32, &str); 8] = [
    ("Hades", 1145360, "Action"),
    ("Hollow Knight", 367520, "Adventure"),
    ("Stardew Valley", 413150, "Simulation"),
    ("Celeste", 504230, "Platformer"),
    ("Portal 2", 620, "Puzzle"),
    ("Terraria", 105600, "Adventure"),
    ("Dead Cells", 588650, "Action"),
    ("No Man’s Sky", 275850, "Exploration"),
];

fn fixtures() -> DataSource {
    SOURCE
        .get_or_init(|| {
            caskade_app::runtime::block_on(DataSource::fixtures(
                caskade_ui_data::fixtures::DEFAULT_COUNT,
            ))
            .expect("the fixture library")
        })
        .clone()
}

fn website() -> DataSource {
    WEBSITE
        .get()
        .expect("open_website before the website's screens")
        .clone()
}

pub fn screen() -> Element {
    framed("/library?filter=all&sort=title&layout=grid", fixtures)
}

/// The `home` reference screen (#98): the same frame opened on Home.
pub fn home() -> Element {
    framed("/", fixtures)
}

/// Home over the website's sample library, on the desktop.
pub fn website_home() -> Element {
    framed("/", website)
}

/// Home over the website's sample library in console mode: the mode is set
/// before the frame decides it.
pub fn website_console_home() -> Element {
    use_hook(|| crate::console::set_mode(crate::console::Mode::Console, false));
    framed("/", website)
}

fn framed(start: &'static str, source: fn() -> DataSource) -> Element {
    use_data_source(source);
    rsx! {
        InputRoot {
            super::Frame { start }
        }
    }
}

/// The website's games, played most recently first, the first three
/// favourites.
fn website_games() -> Vec<FixtureGame> {
    WEBSITE_GAMES
        .iter()
        .zip(0u32..)
        .map(|(&(title, steam, genre), index)| FixtureGame {
            id: format!("website-{steam}"),
            entry_id: format!("website-entry-{steam}"),
            title: title.into(),
            genre,
            platform: "PC".into(),
            installed: true,
            emulated: false,
            favorite: index < 3,
            playtime_ms: u64::from(8 - index) * 3_600_000,
            last_played_days_ago: Some(index),
            release_date: None,
        })
        .collect()
}

/// Makes the website's sample library: [`WEBSITE_GAMES`], each with its
/// cover from `art` (`<steam id>.jpg`) imported into an artwork cache at
/// `cache`, which the screens' images are then served from.
pub fn open_website(art: &Path, cache: &Path) -> Result<(), String> {
    let artwork = caskade_core::artwork::ArtworkCache::new(cache)
        .map_err(|error| format!("{}: {error}", cache.display()))?;
    let mut covers = Vec::new();
    for &(title, steam, _) in &WEBSITE_GAMES {
        let path = art.join(format!("{steam}.jpg"));
        let bytes = std::fs::read(&path).map_err(|error| {
            format!(
                "{title}'s cover, {}: {error} (scripts/website-art.sh downloads it)",
                path.display()
            )
        })?;
        covers.push((format!("website-{steam}"), bytes));
    }
    let importer = artwork.clone();
    let fixtures = caskade_app::runtime::block_on(async move {
        let fixtures = caskade_ui_data::fixtures::open_games(website_games())
            .await
            .map_err(|error| error.to_string())?;
        for (game_id, bytes) in covers {
            let cached = importer
                .import_bytes(bytes)
                .await
                .map_err(|error| format!("{game_id}'s cover: {error}"))?;
            fixtures
                .store
                .upsert_artwork(&cached.record(
                    &game_id,
                    caskade_core::model::ArtworkKind::Cover,
                    "steam",
                ))
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok::<_, String>(fixtures)
    })?;
    crate::library::use_art_cache(artwork);
    let source = DataSource {
        library: fixtures.library,
        settings: fixtures.settings,
        themes: fixtures.themes,
        discovery: fixtures.discovery,
        stores: fixtures.stores,
        plugins: fixtures.plugins,
        addons: fixtures.addons,
        jobs: fixtures.jobs,
        job_fakes: None,
        plugin_ui: fixtures.plugin_ui,
        plugin_fake: Some(fixtures.plugin_fake),
        events: fixtures.events,
    };
    WEBSITE
        .set(source)
        .map_err(|_| "the website's library is already open".to_owned())
}

/// Whether the screen has its data: the grid shows games.
pub fn ready(doc: &BaseDocument) -> bool {
    let mut found = false;
    doc.visit(|_, node| {
        found |= node.attr(LocalName::from("data-index")).is_some();
    });
    found
}

/// Whether Home shows its hero and at least `cards` games.
fn shelves_ready(doc: &BaseDocument, cards: usize) -> bool {
    let mut shown = 0;
    let mut hero = false;
    doc.visit(|_, node| {
        shown += usize::from(node.attr(LocalName::from("data-cs")) == Some("game-card"));
        hero |= node.attr(LocalName::from("data-part")) == Some("eyebrow");
    });
    hero && shown >= cards
}

/// Whether Home has its data: its shelves show games.
pub fn home_ready(doc: &BaseDocument) -> bool {
    shelves_ready(doc, 24)
}

/// Whether the website's Home has its games and every cover on it has
/// loaded.
pub fn website_ready(doc: &BaseDocument) -> bool {
    let mut covers = 0;
    let mut loading = false;
    doc.visit(|_, node| {
        let Some(element) = node.element_data() else {
            return;
        };
        let artwork = node
            .attr(LocalName::from("src"))
            .is_some_and(|src| src.starts_with("caskade-asset:"));
        if element.name.local.as_ref() == "img" && artwork {
            covers += 1;
            loading |= element.raster_image_data().is_none();
        }
    });
    shelves_ready(doc, WEBSITE_GAMES.len()) && covers >= WEBSITE_GAMES.len() && !loading
}

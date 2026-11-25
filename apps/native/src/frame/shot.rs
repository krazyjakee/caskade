//! The `shell` reference screen (#96): the real frame, over the fixture
//! library, open on the library's grid, and `home` (#98), open on Home, for
//! the theme screenshots (`crate::shots`). Their data loads after the first
//! render, so a shot waits until the cards are there ([`ready`],
//! [`home_ready`]).
//!
//! The views' screens (#153) are the same frame open on another page
//! ([`PAGES`], [`page`]; Settings' sections too), first-run setup over a
//! library of its own ([`onboarding`]), the game editor over scripted data
//! ([`editor`]) and Home in console mode ([`console_home`]). Nothing reaches
//! the app's services: the Store and a game's page show what fixtures mode
//! has.
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
        if &*element.name.local == "img" && artwork {
            covers += 1;
            loading |= element.raster_image_data().is_none();
        }
    });
    shelves_ready(doc, WEBSITE_GAMES.len()) && covers >= WEBSITE_GAMES.len() && !loading
}

/// The page the next [`page`] screen opens on.
static PAGE: std::sync::Mutex<&'static str> = std::sync::Mutex::new("/");

/// The views' screens in every theme (#153): the screen's name, the page
/// it opens on, and an attribute and value there once the page has its
/// data.
pub const PAGES: &[(&str, &str, (&str, &str))] = &[
    ("game", "/games/fixture-1", ("data-cs", "hero")),
    ("store", "/store?tab=browse", ("data-view", "store")),
    ("collections", "/collections", ("data-view", "collections")),
    ("activity", "/activity?tab=jobs", ("data-view", "activity")),
    (
        "settings-general",
        "/settings?tab=general",
        ("id", "settings-tab-general-panel"),
    ),
];

/// Opens the next [`page`] screen on `href`.
pub fn open_page(href: &'static str) {
    *caskade_core::lock(&PAGE) = href;
}

/// The frame over the fixture library, open on the page [`open_page`] set.
pub fn page() -> Element {
    framed(*caskade_core::lock(&PAGE), fixtures)
}

/// Whether an element of `doc` has attribute `name` set to `value`.
pub fn has(doc: &BaseDocument, name: &str, value: &str) -> bool {
    let mut found = false;
    doc.visit(|_, node| {
        found |= node.attr(LocalName::from(name)) == Some(value);
    });
    found
}

/// Home in console mode over the fixture library: the mode is set before
/// the frame decides it.
pub fn console_home() -> Element {
    use_hook(|| crate::console::set_mode(crate::console::Mode::Console, false));
    framed("/", fixtures)
}

/// Whether console mode's Home has its shelves.
pub fn console_ready(doc: &BaseDocument) -> bool {
    has(doc, "data-mode", "console") && shelves_ready(doc, 8)
}

/// A library of its own with setup still to do, its sounds off.
static FIRST_RUN: OnceLock<DataSource> = OnceLock::new();

fn first_run() -> DataSource {
    FIRST_RUN
        .get_or_init(|| {
            caskade_app::runtime::block_on(async {
                let source = DataSource::fixtures(3).await.expect("the fixture library");
                source
                    .settings
                    .update("core.onboarding", |mut current| {
                        current["completed"] = serde_json::json!(false);
                        current["sounds"] = serde_json::json!(false);
                        current
                    })
                    .await
                    .expect("setup still to do");
                source
            })
        })
        .clone()
}

/// First-run setup, on its welcome.
pub fn onboarding() -> Element {
    framed("/", first_run)
}

/// Whether setup shows its welcome.
pub fn onboarding_ready(doc: &BaseDocument) -> bool {
    has(doc, "data-step", "welcome")
}

/// An answer the editor gets at once.
fn answer<T: 'static>(value: T) -> crate::game::launch::Answer<T> {
    Box::pin(async move { Ok(value) })
}

/// A change the screen's editor can't make: it only shows.
fn refused<T: 'static>() -> crate::game::launch::Answer<T> {
    Box::pin(async {
        Err(caskade_core::error::ErrorReport::from(
            "Screenshots change nothing.".to_owned(),
        ))
    })
}

/// The editor's data for its screen: `fixture-1` as the fixtures have it,
/// no artwork, actions or save locations to show, and every change refused.
fn still_edit_data() -> crate::game::editor::data::EditData {
    use crate::game::editor::data::{EditData, SaveLocationsState};
    use std::rc::Rc;
    EditData {
        state: Rc::new(|_| {
            let mut game = caskade_core::model::Game::new("Fixture Game 0002");
            game.id = "fixture-1".to_owned();
            answer(Some(caskade_core::model::GameEditState {
                automatic: game.clone(),
                game,
                manual: Vec::new(),
            }))
        }),
        save: Rc::new(|_, _, _| refused()),
        artwork: Rc::new(|_| answer(Some(Vec::new()))),
        set_artwork: Rc::new(|_, _, _| refused()),
        actions: Rc::new(|_| answer(Vec::new())),
        save_action: Rc::new(|_, _| refused()),
        delete_action: Rc::new(|_, _| refused()),
        order_actions: Rc::new(|_, _, _| refused()),
        action_settings: Rc::new(|_, _, _| refused()),
        proton: Rc::new(|| answer(Default::default())),
        saves: Rc::new(|_| {
            answer(SaveLocationsState {
                entry_id: String::new(),
                revision: 0,
                preferences: caskade_core::saves::SavePreferences::default(),
                matched: caskade_core::saves::SaveMatch::Unknown,
                automatic: Vec::new(),
                locations: Vec::new(),
                warnings: Vec::new(),
            })
        }),
        edit_saves: Rc::new(|_, _, _| refused()),
    }
}

/// The game editor (#100) open on `fixture-1`'s details, over the fixture
/// library.
pub fn editor() -> Element {
    use_data_source(fixtures);
    use_context_provider(still_edit_data);
    rsx! {
        InputRoot {
            crate::ui::ToastRegion {
                crate::game::editor::GameEditor {
                    game_id: "fixture-1",
                    title: "Fixture Game 0002",
                    on_close: |_| {},
                }
            }
        }
    }
}

/// Whether the editor shows the game's details.
pub fn editor_ready(doc: &BaseDocument) -> bool {
    has(doc, "data-cs", "game-editor") && has(doc, "data-field", "title")
}

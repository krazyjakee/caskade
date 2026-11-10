//! The hooks in a real Dioxus `VirtualDom`, without a window, over the
//! fixture library (the real core and app layer): a view lists, pages,
//! searches and changes library data, and a core write reaches it.

use super::*;
use caskade_core::model::{GameFilter, GameSort};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The fixture library the test's app reads.
static SOURCE: OnceLock<DataSource> = OnceLock::new();

/// What the view showed at a render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    /// `fixture-1`'s favourite flag.
    favorite: bool,
    /// How many favourites the page counts.
    favorites: u64,
    /// How many games the search for `fixture-1`'s title found.
    found: usize,
}

static SEEN: Mutex<Vec<Option<Shown>>> = Mutex::new(Vec::new());

fn app() -> Element {
    use_data_source(|| SOURCE.get().expect("the fixture library").clone());
    rsx! { Watched {} }
}

#[component]
fn Watched() -> Element {
    let game = use_game("fixture-1".into());
    let query = use_signal(|| ListGames {
        filter: GameFilter::Favorites,
        sort: GameSort::Title,
        limit: 500,
        ..ListGames::default()
    });
    let page = use_library_page(query.into());
    let text = use_signal(|| "Fixture Game 0002".to_owned());
    let search = use_library_search(text.into());
    let shown = (|| {
        let favorite = game.read().as_ref()?.as_ref().ok()?.as_ref()?.game.favorite;
        let page = page.read();
        let page = page.as_ref()?.as_ref().ok()?;
        assert!(page.items.len() <= caskade_ui_data::PAGE_LIMIT as usize);
        let found = search.read().as_ref()?.as_ref().ok()?.items.len();
        Some(Shown {
            favorite,
            favorites: page.total,
            found,
        })
    })();
    SEEN.lock().unwrap().push(shown);
    rsx! { "{shown:?}" }
}

/// Renders until the view has shown `want`, or fails after a while.
async fn until_shown(dom: &mut VirtualDom, want: Shown) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while SEEN.lock().unwrap().last() != Some(&Some(want)) {
            dom.wait_for_work().await;
            dom.render_immediate(&mut dioxus_core::NoOpMutations);
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never showed {want:?}: {:?}", SEEN.lock().unwrap()));
}

#[test]
fn a_view_lists_searches_and_changes_and_a_core_write_updates_it() {
    let source = caskade_app::runtime::block_on(DataSource::fixtures(
        caskade_ui_data::fixtures::DEFAULT_COUNT,
    ))
    .unwrap();
    let library = source.library.clone();
    let _ = SOURCE.set(source);
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    // Dioxus's executor, as the window's would be: a thread of its own.
    let ui = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    ui.block_on(async {
        // fixture-0, -3, … -21 are favourites; fixture-1 isn't.
        until_shown(
            &mut dom,
            Shown {
                favorite: false,
                favorites: 8,
                found: 1,
            },
        )
        .await;
        // A change, as a view's toggle makes it.
        change(async move { library.set_favorite("fixture-1", true).await })
            .await
            .unwrap();
        until_shown(
            &mut dom,
            Shown {
                favorite: true,
                favorites: 9,
                found: 1,
            },
        )
        .await;
    });
}

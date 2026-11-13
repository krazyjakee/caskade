//! Saving the library's view as a smart collection (#99), headless over the
//! fixture library, as `LibraryView.test.tsx` has it.

use crate::frame::tests::{ONE, Window, source};
use crate::router::Route;
use caskade_core::model::{CollectionKind, GameFilter, GameSort};

#[test]
fn the_librarys_view_saves_as_a_smart_collection() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    window.go("/library?filter=installed&sort=playtime&layout=grid&q=fixture");
    window.settle_until("the library", |window| {
        window.find("data-part", "save-collection").is_some()
    });
    let save = window.get("data-part", "save-collection");
    window.click(save);
    window.settle_until("the collection dialog", |window| {
        window.find("data-cs", "collection-dialog").is_some()
    });
    let dialog = window.get("data-cs", "collection-dialog");
    // Smart, with the view's rules and how many games they find.
    let rules = window
        .find_in(dialog, "data-part", "rules")
        .expect("its rules");
    assert!(
        window.text(rules).contains("Installed"),
        "{}",
        window.text(rules)
    );
    window.settle_until("the count", |window| {
        window
            .find("data-part", "matches")
            .is_some_and(|matches| window.text(matches).contains("match now"))
    });
    // Name is the dialog's first field.
    let name = window
        .find_in(dialog, "data-part", "input")
        .expect("the name field");
    window.focus(name);
    window.type_text("Fixtures I play");
    let create = window.find_in(dialog, "data-part", "save").expect("Create");
    window.click(create);
    window.settle_until("the new collection", |window| {
        matches!(window.route(), Route::Collection { .. })
    });
    let Route::Collection { id } = window.route() else {
        unreachable!()
    };
    let saved = caskade_app::runtime::block_on(source().library.collections())
        .unwrap()
        .into_iter()
        .find(|collection| collection.id == id)
        .expect("the saved collection");
    assert_eq!(saved.name, "Fixtures I play");
    match saved.kind {
        CollectionKind::Smart { query } => {
            assert_eq!(query.filter, GameFilter::Installed);
            assert_eq!(query.sort, GameSort::Playtime);
            assert_eq!(query.query.as_deref(), Some("fixture"));
        }
        CollectionKind::Manual => panic!("saved as manual"),
    }
}

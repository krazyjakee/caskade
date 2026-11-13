//! Add game: a title, a link or a file, added and opened.

use super::*;

#[test]
fn a_game_by_title_is_added_and_opened() {
    let _one = caskade_core::lock(&ONE);
    let mut window = Window::open();
    window.choose(1);
    assert_eq!(
        window.attr("[data-cs=manual-game]", "data-mode").as_deref(),
        Some("title")
    );
    assert_eq!(
        window.text("[data-cs=manual-game] h2"),
        "Add a game by title"
    );
    assert!(
        window.find("[data-part=target]").is_none(),
        "no target to choose"
    );
    // Nothing to add without a title.
    assert!(window.attr("#manual-add", "aria-disabled").as_deref() == Some("true"));
    window.focus("#manual-title-input");
    window.type_text("Shelf Cartridge Quest");
    assert_eq!(window.text("#manual-add"), "Add game");
    window.press("#manual-add");
    window.until("the game opened", |w| !w.opened().is_empty());
    assert!(
        window.find("[data-cs=manual-game]").is_none(),
        "the dialog closed"
    );
    let id = window.opened()[0].clone();
    assert_eq!(game_title(&id).as_deref(), Some("Shelf Cartridge Quest"));
}

#[test]
fn a_title_already_in_the_library_asks_and_can_be_kept_apart() {
    let _one = caskade_core::lock(&ONE);
    let title = a_fixture_title();
    let mut window = Window::open();
    window.choose(1);
    window.focus("#manual-title-input");
    window.type_text(&title);
    // Enter in the title adds, as the webview's form submits.
    window.key(Key::Named(NamedKey::Enter));
    window.until("the duplicates", |w| {
        w.find("[data-part=duplicates]").is_some()
    });
    assert_eq!(
        window.text("[data-part=duplicates] h3"),
        "This may already be in your library"
    );
    let line = window.text("[data-part=duplicate]");
    assert!(
        line.contains(&title) && line.contains("has the same title"),
        "{line}"
    );
    // A title-only record has nothing to join to a game.
    assert_eq!(window.count("[data-part=link]"), 0);
    assert_eq!(window.text("#manual-add"), "Keep separate");
    assert!(window.opened().is_empty(), "nothing added yet");
    window.press("#manual-add");
    window.until("the game opened", |w| !w.opened().is_empty());
    let id = window.opened()[0].clone();
    assert_eq!(game_title(&id).as_deref(), Some(title.as_str()));
}

#[test]
fn a_uri_is_resolved_summarised_added_and_then_offered_to_join() {
    let _one = caskade_core::lock(&ONE);
    let uri = "steam://rungameid/620";
    let mut window = Window::open();
    window.choose(0);
    assert_eq!(
        window.attr("[data-cs=manual-game]", "data-mode").as_deref(),
        Some("target")
    );
    assert_eq!(
        window.text("[data-cs=manual-game] h2"),
        "Add a game on this computer"
    );
    window.focus("#manual-target-input");
    window.type_text(uri);
    // Leaving the field resolves it, as the webview's blur does.
    window.key(Key::Named(NamedKey::Tab));
    window.until("the summary", |w| w.find("[data-part=summary]").is_some());
    let summary = window.text("[data-part=summary]");
    assert!(
        summary.contains("Opens") && summary.contains(uri),
        "{summary}"
    );
    // A URI suggests no title.
    assert!(window.attr("#manual-add", "aria-disabled").as_deref() == Some("true"));
    window.focus("#manual-title-input");
    window.type_text("Portal Two By Hand");
    window.press("#manual-add");
    window.until("the game opened", |w| !w.opened().is_empty());
    let first = window.opened()[0].clone();
    let library = source().library.clone();
    let game = {
        let first = first.clone();
        caskade_app::runtime::block_on(async move { library.game(&first).await })
            .unwrap()
            .unwrap()
    };
    assert!(
        game.entries.iter().any(
            |entry| matches!(&entry.launch, Some(LaunchSpec::Uri { uri: found }) if found == uri)
        ),
        "the entry starts the URI"
    );

    // The same URI again: it may be that game, and can join it.
    window.choose(0);
    window.focus("#manual-target-input");
    window.type_text(uri);
    window.key(Key::Named(NamedKey::Enter));
    window.until("the summary", |w| w.find("[data-part=summary]").is_some());
    window.focus("#manual-title-input");
    window.type_text("Portal Two Again");
    window.press("#manual-add");
    window.until("the duplicates", |w| {
        w.find("[data-part=duplicate]").is_some()
    });
    let line = window.text("[data-part=duplicate]");
    assert!(line.contains("starts the same program"), "{line}");
    window.press(&format!("#manual-link-{first}"));
    window.until("the game opened", |w| w.opened().len() == 2);
    assert_eq!(window.opened()[1], first, "joined to the first game");
}

#[test]
fn a_target_that_cant_start_a_game_says_why() {
    let _one = caskade_core::lock(&ONE);
    let mut window = Window::open();
    window.choose(0);
    window.focus("#manual-target-input");
    window.type_text("/no/such/game.sh");
    window.key(Key::Named(NamedKey::Enter));
    window.until("the error", |w| w.find("[data-part=error]").is_some());
    assert_eq!(
        window.attr("[data-part=error]", "role").as_deref(),
        Some("alert")
    );
    assert!(!window.text("[data-part=error]").is_empty());
    assert!(window.find("[data-part=summary]").is_none());
    window.focus("#manual-title-input");
    window.type_text("Ghost");
    assert!(
        window.attr("#manual-add", "aria-disabled").as_deref() == Some("true"),
        "nothing to start"
    );
    // Cancel closes it with nothing added.
    window.press("#manual-cancel");
    window.until("the dialog closed", |w| {
        w.find("[data-cs=manual-game]").is_none()
    });
    assert!(window.opened().is_empty());
}

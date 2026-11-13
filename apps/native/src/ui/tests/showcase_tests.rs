//! Hero, shelf and avatars.

use super::*;

fn hero_game() -> HeroGame {
    HeroGame {
        title: "Hollow Pines".to_owned(),
        genre: "Adventure".to_owned(),
        status: GameStatus::Installed,
        art: (200, 260),
        description: Some("A forest that remembers.".to_owned()),
        playtime_hours: Some(12.0),
        last_played: Some("2 days ago".to_owned()),
        size_gb: Some(4.2),
    }
}

#[component]
fn Showcase() -> Element {
    let mut chosen = use_signal(|| Some("avatar:droid".to_owned()));
    rsx! {
        InputRoot {
            Hero { id: "hero", game: hero_game(), eyebrow: "Continue playing", action: "Play" }
            Hero {
                id: "store-hero",
                game: HeroGame { playtime_hours: None, last_played: None, ..hero_game() },
                actions: rsx! { button { id: "buy", "Buy" } },
            }
            Shelf {
                id: "shelf",
                title: "Recently played",
                icon: crate::ui::icons::CLOCK,
                subtitle: "3 games",
                on_view_all: |_| did("view all"),
                GameCard { id: "shelved", game: card_game("g5", "Tidewater") }
            }
            Avatar { id: "known", avatar: "avatar:astronaut", name: "Ada Lovelace", label: "Ada", size: AvatarSize::Lg }
            Avatar { id: "unknown", avatar: "avatar:gone", name: "Ada Lovelace" }
            AvatarPicker {
                id: "picker",
                value: chosen(),
                options: avatars().take(4).collect::<Vec<_>>(),
                on_change: move |id: String| {
                    did(format!("chose {id}"));
                    chosen.set(Some(id));
                },
                on_cancel: |_| did("cancel"),
            }
        }
    }
}

fn showcase() -> Element {
    rsx! { Showcase {} }
}

/// The picker's options, in order.
fn avatar_options(window: &Window) -> Vec<NodeId> {
    let picker = window.id("picker");
    let doc = window.doc.inner.borrow();
    let mut options = Vec::new();
    doc.visit(|id, node| {
        if node.attr(LocalName::from("data-part")) == Some("option")
            && node.parent.is_some_and(|parent| parent == picker)
        {
            options.push(id);
        }
    });
    options
}

#[test]
fn a_hero_lists_its_facts_and_offers_the_webviews_actions() {
    let _one = one_window();
    let window = Window::open(showcase);
    assert_eq!(window.attr("hero", "data-cs").as_deref(), Some("hero"));
    let hero = window.heard("hero");
    assert!(hero.contains(r#"Heading "Hollow Pines""#), "{hero}");
    // Playtime hides the size, as on the webview.
    assert!(
        hero.contains("Adventure")
            && hero.contains("12h played")
            && hero.contains("Last played 2 days ago"),
        "{hero}"
    );
    assert!(!hero.contains("4.2 GB"), "{hero}");
    for button in ["Play", "More launch options", "View Details", "More"] {
        assert!(
            hero.contains(&format!(r#"Button "{button}""#)),
            "{button}: {hero}"
        );
    }
    // Without playtime the size shows, and `actions` replaces the buttons.
    let store = window.heard("store-hero");
    assert!(
        store.contains("4.2 GB") && store.contains(r#"Button "Buy""#),
        "{store}"
    );
    assert!(!store.contains("View Details"), "{store}");
}

#[test]
fn a_shelf_titles_its_cards_and_views_them_all() {
    let _one = one_window();
    let mut window = Window::open(showcase);
    let shelf = window.heard("shelf");
    assert!(shelf.contains(r#"Heading "Recently played""#), "{shelf}");
    assert!(shelf.contains(r#"Link "Tidewater""#), "{shelf}");
    let view_all = {
        let doc = window.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if node.attr(LocalName::from("data-part")) == Some("view-all") {
                found = Some(id);
            }
        });
        found.unwrap()
    };
    window.click_node(view_all);
    assert_eq!(take_done(), ["view all"]);
}

#[test]
fn an_avatar_is_its_picture_or_its_names_initials() {
    let _one = one_window();
    let window = Window::open(showcase);
    assert_eq!(
        window.attr("known", "data-source").as_deref(),
        Some("image")
    );
    assert_eq!(window.attr("known", "data-size").as_deref(), Some("lg"));
    assert_eq!(window.heard("known").trim(), r#"Image "Ada""#);
    // An avatar this release doesn't ship: the initials, and decorative.
    assert_eq!(
        window.attr("unknown", "data-source").as_deref(),
        Some("initials")
    );
    assert_eq!(
        window.attr("unknown", "aria-hidden").as_deref(),
        Some("true")
    );
    let doc = window.doc.inner.borrow();
    let unknown = doc.get_node(window.id("unknown")).unwrap();
    assert_eq!(unknown.text_content(), "AL");
}

#[test]
fn the_avatar_picker_chooses_by_mouse_arrows_space_and_a_pad() {
    let _one = one_window();
    let mut window = Window::open(showcase);
    // default, astronaut, droid (chosen), rocket.
    let options = avatar_options(&window);
    assert_eq!(options.len(), 4);
    window.click_node(options[3]);
    assert_eq!(take_done(), ["chose avatar:rocket"]);
    // Arrows move and choose, wrapping around.
    window.key(Key::Named(NamedKey::ArrowRight));
    assert_eq!(take_done(), ["chose avatar:default"]);
    assert_eq!(window.focused(), Some(avatar_options(&window)[0]));
    window.key(Key::Named(NamedKey::ArrowLeft));
    assert_eq!(take_done(), ["chose avatar:rocket"]);
    // Space and a pad's A choose the focused one.
    let options = avatar_options(&window);
    window.focus_node(options[1]);
    window.key(space());
    assert_eq!(take_done(), ["chose avatar:astronaut"]);
    window.focus_node(options[2]);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["chose avatar:droid"]);
    // Escape and a pad's B leave without a change.
    window.key(Key::Named(NamedKey::Escape));
    assert_eq!(take_done(), ["cancel"]);
    window.pad(Action::Back);
    assert_eq!(take_done(), ["cancel"]);
}

#[test]
fn the_avatar_picker_as_a_screen_reader_hears_it() {
    let _one = one_window();
    let window = Window::open(showcase);
    let picker = window.heard("picker");
    assert!(picker.starts_with(r#"RadioGroup "Avatar""#), "{picker}");
    assert!(
        picker.contains(r#"RadioButton "Droid" toggled=True"#),
        "{picker}"
    );
    assert!(
        picker.contains(r#"RadioButton "Rocket" toggled=False"#),
        "{picker}"
    );
    assert_eq!(
        window.attr("picker", "data-cs").as_deref(),
        Some("avatar-picker")
    );
    assert_eq!(window.attr("picker", "data-size").as_deref(), Some("lg"));
    let options = avatar_options(&window);
    let doc = window.doc.inner.borrow();
    let chosen = doc.get_node(options[2]).unwrap();
    assert_eq!(chosen.attr(LocalName::from("data-selected")), Some("true"));
    assert_eq!(chosen.attr(LocalName::from("tabindex")), Some("0"));
}

//! Menus, selects, dialogs and tooltips.

use super::*;

fn menu_items() -> Vec<MenuEntry> {
    let mut remove = MenuItem::new("remove", "Remove");
    remove.danger = true;
    let mut hidden = MenuItem::new("hide", "Hide");
    hidden.disabled = true;
    vec![
        MenuEntry::Item(MenuItem::new("play", "Play")),
        MenuEntry::Item(MenuItem::new("favourite", "Favourite")),
        MenuEntry::Item(hidden),
        MenuEntry::Separator,
        MenuEntry::Section {
            title: Some("Manage".into()),
            items: vec![MenuItem::new("edit", "Edit details"), remove],
        },
    ]
}

#[component]
fn Overlays() -> Element {
    let mut sort = use_signal(|| Some("title".to_owned()));
    let mut asking = use_signal(|| false);
    rsx! {
        InputRoot {
            MenuButton { id: "more", label: "More", items: menu_items(), on_action: |id| did(format!("menu {id}")) }
            Select {
                id: "sort",
                label: "Sort by",
                options: vec![
                    SelectOption::new("title", "Title"),
                    SelectOption::new("played", "Last played"),
                    SelectOption::new("added", "Recently added"),
                ],
                value: sort(),
                on_change: move |value: String| { did(format!("sort {value}")); sort.set(Some(value)) },
            }
            Button { id: "ask", on_press: move |_| asking.set(true), "Remove game" }
            Tooltip { text: "Opens the store page",
                Button { id: "store", "Store" }
            }
            if asking() {
                ConfirmDialog {
                    title: "Remove Hollow Pines?",
                    confirm_label: "Remove",
                    on_confirm: move |_| { did("removed"); asking.set(false) },
                    on_close: move |_| { did("kept"); asking.set(false) },
                    p { "Its saves stay." }
                }
            }
        }
    }
}

fn overlays() -> Element {
    rsx! { Overlays {} }
}

#[test]
fn a_menu_opens_moves_types_ahead_runs_and_gives_focus_back() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    window.focus("more");
    window.key(ENTER);
    assert!(window.exists("menu"));
    assert_eq!(
        window.attr("more", "aria-expanded").as_deref(),
        Some("true")
    );
    assert_eq!(window.focus_text(), "Play");
    window.key(Key::Named(NamedKey::ArrowDown));
    assert_eq!(window.focus_text(), "Favourite");
    // Disabled items are skipped.
    window.key(Key::Named(NamedKey::ArrowDown));
    assert_eq!(window.focus_text(), "Edit details");
    window.key(Key::Named(NamedKey::End));
    assert_eq!(window.focus_text(), "Remove");
    window.key(Key::Named(NamedKey::ArrowDown));
    assert_eq!(window.focus_text(), "Play");
    window.key(Key::Character("e".into()));
    assert_eq!(window.focus_text(), "Edit details");
    window.key(ENTER);
    assert_eq!(take_done(), ["menu edit"]);
    assert!(!window.exists("menu"));
    assert_eq!(window.focus_text(), "More");
    // Escape, and a pad's Back, close it without choosing.
    window.key(Key::Named(NamedKey::ArrowDown));
    assert!(window.exists("menu"));
    window.key(Key::Named(NamedKey::Escape));
    assert!(!window.exists("menu"));
    assert_eq!(window.focus_text(), "More");
    window.pad(Action::Confirm);
    assert!(window.exists("menu"));
    window.pad(Action::NavDown);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["menu favourite"]);
    window.click("more");
    window.pad(Action::Back);
    assert!(!window.exists("menu"));
    assert!(take_done().is_empty());
}

#[test]
fn a_menu_as_a_screen_reader_hears_it() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    assert_eq!(
        window.heard("more"),
        "Button \"More\" expanded=false popup=Menu [focus click expand]\n"
    );
    window.click("more");
    let doc = window.doc.inner.borrow();
    let tree = crate::a11y_snapshot::snapshot(&doc);
    assert!(
        tree.contains(
            "Menu \"More\"\n  MenuItem \"Play\" [click]\n  MenuItem \"Favourite\" [click]\n  \
             MenuItem \"Hide\" disabled\n  Splitter\n  Group \"Manage\"\n    \
             MenuItem \"Edit details\" [click]\n    MenuItem \"Remove\" [click]\n"
        ),
        "{tree}"
    );
}

#[test]
fn a_select_opens_on_its_choice_and_changes_it() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    let trigger = {
        let doc = window.doc.inner.borrow();
        let select = window.id("sort");
        doc.get_node(select).unwrap().children[1]
    };
    window.doc.inner.borrow_mut().set_focus_to(trigger);
    window.key(space());
    assert!(window.exists("listbox"));
    assert_eq!(window.focus_text(), "Title");
    window.key(Key::Named(NamedKey::ArrowDown));
    window.key(ENTER);
    assert_eq!(take_done(), ["sort played"]);
    assert!(!window.exists("listbox"));
    assert!(window.focus_text().starts_with("Last played"));
    // Opened again, focus is on the new choice; Escape keeps it.
    window.pad(Action::Confirm);
    assert_eq!(window.focus_text(), "Last played");
    window.key(Key::Named(NamedKey::Escape));
    assert!(take_done().is_empty());
}

#[test]
fn a_confirm_dialog_starts_on_cancel_and_closes_on_escape_or_back() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    window.click("ask");
    assert!(window.exists("alertdialog"));
    assert_eq!(window.focus_text(), "Cancel");
    window.key(ENTER);
    assert_eq!(take_done(), ["kept"]);
    assert!(!window.exists("alertdialog"));
    assert_eq!(window.focus_text(), "Remove game");
    window.key(ENTER);
    window.key(Key::Named(NamedKey::Escape));
    assert_eq!(take_done(), ["kept"]);
    window.pad(Action::Confirm);
    window.pad(Action::Back);
    assert_eq!(take_done(), ["kept"]);
    window.pad(Action::Confirm);
    window.pad(Action::NavRight);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["removed"]);
}

#[test]
fn a_dialog_as_a_screen_reader_hears_it() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    window.click("ask");
    let doc = window.doc.inner.borrow();
    let tree = crate::a11y_snapshot::snapshot(&doc);
    assert!(
        tree.contains("AlertDialog \"Remove Hollow Pines?\" modal\n"),
        "{tree}"
    );
}

#[test]
fn a_tooltip_shows_on_focus_and_describes_its_trigger() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    assert!(!window.exists("tooltip"));
    // Focus on its trigger (the input layer's, as a click or a pad puts it).
    window.click("store");
    assert!(window.exists("tooltip"));
    assert!(window.heard("store").contains("Button \"Store\""));
    window.key(Key::Named(NamedKey::Escape));
    assert!(!window.exists("tooltip"));
    // Hovering shows it after a moment.
    window.hover("ask");
    window.hover("store");
    std::thread::sleep(Duration::from_millis(800));
    window.settle();
    assert!(window.exists("tooltip"));
}

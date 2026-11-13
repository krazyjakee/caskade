//! Text fields, search and combobox.

use super::*;

#[component]
fn Fields() -> Element {
    let mut name = use_signal(String::new);
    let mut query = use_signal(|| "pines".to_owned());
    let mut platform = use_signal(|| None::<String>);
    rsx! {
        InputRoot {
            TextField {
                id: "name",
                label: "Name",
                description: "As it shows in the library.",
                value: name(),
                on_input: move |value: String| { did(format!("name {value}")); name.set(value) },
                on_submit: move |value: String| did(format!("submit {value}")),
            }
            SearchField {
                id: "search",
                label: "Search your library",
                value: query(),
                on_input: move |value: String| { did(format!("search {value:?}")); query.set(value) },
            }
            Combobox {
                label: "Platform",
                options: vec![
                    SelectOption::new("snes", "SNES"),
                    SelectOption::new("n64", "Nintendo 64"),
                    SelectOption::new("ds", "Nintendo DS"),
                ],
                value: platform(),
                on_change: move |value: String| { did(format!("platform {value}")); platform.set(Some(value)) },
            }
        }
    }
}

fn fields() -> Element {
    rsx! { Fields {} }
}

#[test]
fn a_text_field_types_submits_and_opens_its_edit_menu() {
    let _one = one_window();
    let mut window = Window::open(fields);
    let field = window.id("name");
    window.focus_node(field);
    window.key(Key::Character("H".into()));
    window.key(Key::Character("i".into()));
    assert_eq!(take_done(), ["name H", "name Hi"]);
    window.key(ENTER);
    assert_eq!(take_done(), ["submit Hi"]);
    window.key(Key::Named(NamedKey::ContextMenu));
    assert!(window.exists("menu"));
    assert_eq!(window.focus_text(), "Cut");
    window.key(Key::Named(NamedKey::Escape));
    assert!(!window.exists("menu"));
    assert_eq!(window.focused_id().as_deref(), Some("name"));
    window.pad(Action::Menu);
    assert!(window.exists("menu"));
    window.pad(Action::Back);
    let heard = window.heard("name");
    assert!(heard.starts_with("TextInput \"Name\""), "{heard}");
}

#[test]
fn a_search_field_clears_on_escape_and_its_button() {
    let _one = one_window();
    let mut window = Window::open(fields);
    let field = window.id("search");
    window.focus_node(field);
    window.key(Key::Named(NamedKey::Escape));
    assert_eq!(take_done(), ["search \"\""]);
    let heard = window.heard("search");
    assert!(heard.contains("\"Search your library\""), "{heard}");
    assert_eq!(window.attr("search", "value").as_deref(), Some(""));
    window.key(Key::Character("z".into()));
    assert_eq!(take_done(), ["search \"z\""]);
    let clear = {
        let doc = window.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if node.attr(LocalName::from("aria-label")) == Some("Clear search") {
                found = Some(id);
            }
        });
        found.expect("a clear button while there's text")
    };
    window.click_node(clear);
    assert_eq!(take_done(), ["search \"\""]);
    // A pad's A on the clear button clears too.
    window.focus_node(field);
    window.key(Key::Character("q".into()));
    assert_eq!(take_done(), ["search \"q\""]);
    let clear = {
        let doc = window.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if node.attr(LocalName::from("aria-label")) == Some("Clear search") {
                found = Some(id);
            }
        });
        found.expect("a clear button while there's text")
    };
    window.focus_node(clear);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["search \"\""]);
}

#[test]
fn a_combobox_filters_moves_and_chooses_with_focus_in_the_field() {
    let _one = one_window();
    let mut window = Window::open(fields);
    let field = {
        let doc = window.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if node.attr(LocalName::from("role")) == Some("combobox") {
                found = Some(id);
            }
        });
        found.unwrap()
    };
    window.focus_node(field);
    window.key(Key::Character("n".into()));
    assert!(window.exists("listbox"));
    window.key(Key::Named(NamedKey::ArrowDown));
    let doc_active = {
        let doc = window.doc.inner.borrow();
        doc.get_node(field)
            .and_then(|n| n.attr(LocalName::from("aria-activedescendant")))
            .map(str::to_owned)
    };
    assert!(doc_active.is_some_and(|id| id.ends_with("-1")));
    window.key(ENTER);
    assert_eq!(take_done(), ["platform n64"]);
    assert!(!window.exists("listbox"));
    assert_eq!(window.focused(), Some(field));
}

#[test]
fn a_combobox_opens_on_its_first_option_and_answers_a_pad() {
    let _one = one_window();
    let mut window = Window::open(fields);
    let field = {
        let doc = window.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if node.attr(LocalName::from("role")) == Some("combobox") {
                found = Some(id);
            }
        });
        found.unwrap()
    };
    let active = |window: &Window| {
        let doc = window.doc.inner.borrow();
        doc.get_node(field)
            .and_then(|node| node.attr(LocalName::from("aria-activedescendant")))
            .map(|id| id.rsplit('-').next().unwrap_or_default().to_owned())
    };
    window.focus_node(field);
    window.key(Key::Named(NamedKey::ArrowDown));
    assert!(window.exists("listbox"));
    assert_eq!(active(&window).as_deref(), Some("0"));
    window.key(Key::Named(NamedKey::Escape));
    assert!(!window.exists("listbox"));
    assert_eq!(window.focused(), Some(field));
    window.pad(Action::NavDown);
    assert_eq!(active(&window).as_deref(), Some("0"));
    window.pad(Action::NavUp);
    assert_eq!(active(&window).as_deref(), Some("2"));
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["platform ds"]);
    assert!(!window.exists("listbox"));
    assert_eq!(window.focused(), Some(field));
}

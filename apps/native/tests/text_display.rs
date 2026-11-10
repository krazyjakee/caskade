//! What Blitz shows of text that doesn't fit and of empty fields (#80),
//! headless: a title cut with `text-overflow: ellipsis` (Tailwind's
//! `truncate`) is laid out with its ellipsis and one that fits isn't, and
//! a field's placeholder shows only while it's empty and nothing is being
//! composed in it (#84's IME). `tests/text_pixels.rs` compares how both
//! look with Chromium.
//!
//! `cargo test -p caskade-native --test text_display`

use blitz_dom::{Document, DocumentConfig, LocalName};
use blitz_traits::events::{
    BlitzAccessibilityAction, BlitzAccessibilityEvent, BlitzImeEvent, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;

const TRUNCATE: &str =
    "width: 120px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;";

fn open(app: fn() -> Element) -> DioxusDocument {
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    settle(&mut doc);
    doc
}

fn settle(doc: &mut DioxusDocument) {
    for _ in 0..3 {
        Document::poll(doc, None);
        doc.inner.borrow_mut().resolve(0.0);
    }
}

fn send(doc: &mut DioxusDocument, event: UiEvent) {
    Document::handle_ui_event(doc, event);
    settle(doc);
}

fn titles() -> Element {
    rsx! {
        p { id: "long", style: TRUNCATE, "A Title Far Too Long For Its Card" }
        p { id: "short", style: TRUNCATE, "Short" }
    }
}

#[test]
fn a_cut_title_is_laid_out_with_its_ellipsis() {
    let doc = open(titles);
    let doc = doc.inner.borrow();
    let overflow = |id: &str| {
        let node = doc
            .get_node(doc.get_element_by_id(id.trim_start_matches('#')).unwrap())
            .unwrap();
        let layout = node
            .element_data()
            .and_then(|element| element.inline_layout_data.as_ref())
            .expect("the paragraph's text");
        layout
            .overflow
            .as_ref()
            .map(|overflow| (overflow.lines.len(), overflow.marker.advance))
    };
    let (lines, marker) = overflow("#long").expect("cut");
    assert_eq!(lines, 1);
    assert!(marker > 0.0, "the ellipsis has a width");
    assert_eq!(overflow("#short"), None, "a title that fits isn't cut");
}

fn search() -> Element {
    let mut query = use_signal(String::new);
    rsx! {
        input {
            id: "search",
            value: "{query}",
            placeholder: "Search games",
            oninput: move |event: FormEvent| query.set(event.value()),
        }
    }
}

/// The field's placeholder, while it shows.
fn placeholder(doc: &DioxusDocument) -> Option<String> {
    let doc = doc.inner.borrow();
    let field = doc.get_element_by_id("search")?;
    let input = doc.get_node(field)?.element_data()?.text_input_data()?;
    let placeholder = input.placeholder.as_ref()?;
    input
        .shows_placeholder()
        .then(|| placeholder.editor.raw_text().to_owned())
}

#[test]
fn a_placeholder_shows_while_the_field_is_empty_and_not_composing() {
    let mut doc = open(search);
    assert_eq!(placeholder(&doc).as_deref(), Some("Search games"));
    let field = doc.inner.borrow().get_element_by_id("search").unwrap();
    send(
        &mut doc,
        UiEvent::Accessibility(BlitzAccessibilityEvent {
            target: field,
            action: BlitzAccessibilityAction::Focus,
        }),
    );
    // Focus alone keeps it, as Chromium does.
    assert_eq!(placeholder(&doc).as_deref(), Some("Search games"));

    // An IME composing into the empty field: the preedit shows, not the
    // placeholder, though the value is still empty.
    send(&mut doc, UiEvent::Ime(BlitzImeEvent::Enabled));
    send(
        &mut doc,
        UiEvent::Ime(BlitzImeEvent::Preedit("ni".into(), Some((2, 2)))),
    );
    assert_eq!(placeholder(&doc), None);
    send(&mut doc, UiEvent::Ime(BlitzImeEvent::Commit("你".into())));
    assert_eq!(placeholder(&doc), None, "a value");

    // Emptied again: back.
    doc.inner
        .borrow_mut()
        .mutate()
        .set_attribute(field, qual("value"), "");
    settle(&mut doc);
    assert_eq!(placeholder(&doc).as_deref(), Some("Search games"));

    // A new placeholder (another language) replaces the old, as a
    // re-render sets it.
    doc.inner
        .borrow_mut()
        .mutate()
        .set_attribute(field, qual("placeholder"), "Buscar juegos");
    settle(&mut doc);
    assert_eq!(placeholder(&doc).as_deref(), Some("Buscar juegos"));
}

fn qual(name: &str) -> blitz_dom::QualName {
    blitz_dom::QualName::new(None, blitz_dom::ns!(), LocalName::from(name))
}

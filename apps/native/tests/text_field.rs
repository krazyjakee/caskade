//! Text fields in the native UI with an IME (#84): a controlled field hears
//! a composition's events, its keys say they're composing, and its value is
//! the committed text, as eng3's TextField and Search (#93) rely on. Blitz's
//! text fields are the vendored series' (`vendor/blitz`).
//!
//! `cargo test -p caskade-native --test text_field`

use blitz_dom::{Document, DocumentConfig};
use blitz_traits::events::{
    BlitzAccessibilityAction, BlitzAccessibilityEvent, BlitzImeEvent, BlitzKeyEvent, KeyState,
    UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Code, Location, Modifiers, NamedKey};
use dioxus_native::prelude::*;
use std::sync::Mutex;

static HEARD: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn heard(what: String) {
    caskade_core::lock(&HEARD).push(what);
}

#[component]
fn Search() -> Element {
    let mut query = use_signal(String::new);
    rsx! {
        input {
            id: "search",
            r#type: "search",
            value: "{query}",
            oninput: move |event: FormEvent| {
                heard(format!("input {}", event.value()));
                query.set(event.value());
            },
            oncompositionstart: move |_| heard("compositionstart".into()),
            oncompositionupdate: move |event: CompositionEvent| {
                heard(format!("compositionupdate {}", event.data().data()))
            },
            oncompositionend: move |event: CompositionEvent| {
                heard(format!("compositionend {}", event.data().data()))
            },
            onkeydown: move |event: KeyboardEvent| {
                heard(format!("keydown composing={}", event.is_composing()))
            },
        }
    }
}

fn send(doc: &mut DioxusDocument, event: UiEvent) {
    Document::handle_ui_event(doc, event);
    settle(doc);
}

fn settle(doc: &mut DioxusDocument) {
    for _ in 0..3 {
        Document::poll(doc, None);
        doc.inner.borrow_mut().resolve(0.0);
    }
}

#[test]
fn a_composition_reaches_the_fields_listeners() {
    let mut doc = DioxusDocument::new(
        VirtualDom::new(Search),
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    settle(&mut doc);
    let field = doc.inner.borrow().get_element_by_id("search").unwrap();
    send(
        &mut doc,
        UiEvent::Accessibility(BlitzAccessibilityEvent {
            target: field,
            action: BlitzAccessibilityAction::Focus,
        }),
    );
    HEARD.lock().unwrap().clear();

    send(
        &mut doc,
        UiEvent::Ime(BlitzImeEvent::Preedit("ni".into(), Some((2, 2)))),
    );
    // Enter that picks a candidate is a composing key.
    send(
        &mut doc,
        UiEvent::KeyDown(BlitzKeyEvent {
            key: Key::Named(NamedKey::Enter),
            code: Code::Enter,
            modifiers: Modifiers::empty(),
            location: Location::Standard,
            is_auto_repeating: false,
            is_composing: false,
            state: KeyState::Pressed,
            text: None,
        }),
    );
    send(
        &mut doc,
        UiEvent::Ime(BlitzImeEvent::Preedit("你".into(), Some((3, 3)))),
    );
    send(&mut doc, UiEvent::Ime(BlitzImeEvent::Commit("你".into())));
    // Then a pasted query, with a line break a single-line field drops.
    send(&mut doc, UiEvent::Paste("好\n".into()));

    assert_eq!(
        *HEARD.lock().unwrap(),
        [
            "compositionstart",
            "compositionupdate ni",
            "keydown composing=true",
            "compositionupdate 你",
            "input 你",
            "compositionend 你",
            "input 你好",
        ]
    );
    // The controlled value went round through the signal and back: the
    // field holds what it was told, with nothing lost.
    let doc = doc.inner.borrow();
    let value = doc
        .get_node(field)
        .and_then(|node| node.element_data())
        .and_then(|element| element.text_input_data())
        .map(|input| input.editor.text().to_string());
    assert_eq!(value.as_deref(), Some("你好"));
}

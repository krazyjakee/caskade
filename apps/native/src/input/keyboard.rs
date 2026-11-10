//! The on-screen keyboard: how a pad types (the webview's
//! `OnScreenKeyboard.tsx`, over `caskade_ui_input::osk`). It opens on a text
//! field (A on the field, a tap in console mode) and types into it while
//! focus stays there. The d-pad moves between keys, A types, B closes, X
//! deletes, Y is a space, LB/RB switch between letters and symbols, LT/RT
//! move the cursor, Start is Done. Keys also take clicks and taps, which
//! leave focus in the field.

use super::root::{Bind, Input, use_input, use_input_action};
use blitz_dom::{BaseDocument, LocalName, NodeId, QualName, ns};
use caskade_core::{tr, trp};
use caskade_ui_input::Action;
use caskade_ui_input::osk::{Field, KeyKind, Keyboard, Layer, Outcome, Shift, Special};
use dioxus_native::prelude::dioxus_html::PlatformEventData;
use dioxus_native::prelude::*;
use std::any::Any;
use std::rc::Rc;

/// What the field holds now.
fn value_of(doc: &BaseDocument, field: NodeId) -> String {
    let Some(node) = doc.get_node(field) else {
        return String::new();
    };
    node.element_data()
        .and_then(|element| element.text_input_data())
        .map(|input| input.editor.raw_text().to_owned())
        .or_else(|| node.attr(LocalName::from("value")).map(str::to_owned))
        .unwrap_or_default()
}

/// The keyboard's state over its field.
#[derive(Clone, Copy, PartialEq)]
struct Typer {
    field: NodeId,
    board: Signal<Keyboard>,
    text: Signal<Field>,
}

impl Typer {
    /// The field's text as it is now: someone may have typed on a real
    /// keyboard, or pasted, meanwhile.
    fn sync(&self, input: &Input) {
        let now = input
            .read(|doc| value_of(doc, self.field))
            .unwrap_or_default();
        let mut text = self.text;
        if text.peek().value != now {
            text.set(Field::new(now));
        }
    }

    /// A pad action; whether the keyboard took it.
    fn action(&self, input: &Input, action: Action) -> bool {
        self.sync(input);
        let before = self.text.peek().value.clone();
        let mut board = self.board;
        let mut text = self.text;
        let outcome = board.write().action(action, &mut text.write());
        match outcome {
            Some(outcome) => {
                self.finish(input, outcome, &before);
                true
            }
            None => false,
        }
    }

    /// A click or tap on the key at `row`, `col`.
    fn tap(&self, input: &Input, row: usize, col: usize) {
        self.sync(input);
        let before = self.text.peek().value.clone();
        let mut board = self.board;
        let mut text = self.text;
        let outcome = board.write().tap(row, col, &mut text.write());
        self.finish(input, outcome, &before);
    }

    fn finish(&self, input: &Input, outcome: Outcome, before: &str) {
        let value = self.text.peek().value.clone();
        if value != before {
            // The field shows it, and its listeners hear it as typing.
            input.write(|doc| {
                doc.mutate().set_attribute(
                    self.field,
                    QualName::new(None, ns!(), LocalName::from("value")),
                    &value,
                );
            });
            input.fire(self.field, "input", form_data(&value), true);
        }
        match outcome {
            Outcome::Changed => {}
            // The clipboard's text arrives in the focused field: this one.
            Outcome::Paste => {
                input.write(|doc| {
                    if doc.shell_provider.request_clipboard_text().is_err() {
                        tracing::debug!("no clipboard to paste from");
                    }
                });
            }
            Outcome::Close => input.close_keyboard(),
            Outcome::Submit => {
                input.close_keyboard();
                input.fire(self.field, "submit", form_data(&value), true);
            }
        }
    }
}

fn form_data(value: &str) -> Rc<dyn Any> {
    Rc::new(PlatformEventData::new(dioxus_native::synthetic_form_event(
        value,
        Vec::new(),
    )))
}

/// Binds `action` to the keyboard, over the field's own bindings and its
/// popover's.
fn use_key_action(typer: Typer, action: Action, label: Option<String>) {
    let input = use_input();
    use_input_action(
        action,
        Bind {
            label,
            owner: Some(typer.field),
            global: true,
        },
        move || typer.action(&input, action),
    );
}

#[component]
pub fn OnScreenKeyboard(field: NodeId) -> Element {
    let input = use_input();
    let board = use_signal(|| Keyboard::new(&caskade_core::i18n::locale()));
    let text =
        use_signal(|| Field::new(input.read(|doc| value_of(doc, field)).unwrap_or_default()));
    let typer = Typer { field, board, text };
    let delete = trp!("on-screen keyboard", "Delete");
    let space = trp!("on-screen keyboard", "Space");
    let paste = trp!("on-screen keyboard", "Paste");
    let left = tr!("Cursor left");
    let right = tr!("Cursor right");
    let done = trp!("on-screen keyboard", "Done");
    let other = match board.read().layer {
        Layer::Letters => trp!("on-screen keyboard", "Symbols"),
        Layer::Symbols => trp!("on-screen keyboard", "Letters"),
    };
    for action in [
        Action::NavUp,
        Action::NavDown,
        Action::NavLeft,
        Action::NavRight,
    ] {
        use_key_action(typer, action, None);
    }
    use_key_action(
        typer,
        Action::Confirm,
        Some(trp!("on-screen keyboard", "Type")),
    );
    use_key_action(typer, Action::Back, Some(tr!("Close keyboard")));
    use_key_action(typer, Action::Details, Some(delete.clone()));
    use_key_action(typer, Action::Search, Some(space.clone()));
    use_key_action(typer, Action::TabPrev, Some(other.clone()));
    use_key_action(typer, Action::TabNext, Some(other));
    use_key_action(typer, Action::PagePrev, Some(left.clone()));
    use_key_action(typer, Action::PageNext, Some(right.clone()));
    use_key_action(typer, Action::Menu, Some(done.clone()));

    let secret = input
        .read(|doc| {
            doc.get_node(field)
                .and_then(|node| node.attr(LocalName::from("type")))
                == Some("password")
        })
        .unwrap_or(false);
    let keyboard = board.read();
    let shown = text.read().value.clone();
    let preview = if secret {
        "•".repeat(shown.chars().count())
    } else {
        shown
    };
    let layer = match keyboard.layer {
        Layer::Letters => "letters",
        Layer::Symbols => "symbols",
    };
    let shift = match keyboard.shift {
        Shift::Off => "off",
        Shift::Once => "once",
        Shift::Lock => "lock",
    };
    let rows = keyboard.rows();
    let (at_row, at_col) = keyboard.at;
    let label_of = |kind: KeyKind| -> String {
        match kind {
            KeyKind::Char(_) => String::new(),
            KeyKind::Special(Special::Shift) => "⇧".into(),
            KeyKind::Special(Special::Layer) => match keyboard.layer {
                Layer::Letters => trp!("on-screen keyboard", "?123"),
                Layer::Symbols => trp!("on-screen keyboard", "abc"),
            },
            KeyKind::Special(Special::Backspace) => delete.clone(),
            KeyKind::Special(Special::Space) => space.clone(),
            KeyKind::Special(Special::Paste) => paste.clone(),
            KeyKind::Special(Special::Left) => left.clone(),
            KeyKind::Special(Special::Right) => right.clone(),
            KeyKind::Special(Special::Done) => done.clone(),
        }
    };
    rsx! {
        fieldset {
            "data-cs": "osk",
            "data-layer": layer,
            "data-shift": shift,
            aria_label: tr!("On-screen keyboard"),
            output { "data-part": "preview", "{preview}" }
            for (r, row) in rows.iter().enumerate() {
                div { key: "{r}", "data-part": "row",
                    for (c, key) in row.iter().enumerate() {
                        button {
                            key: "{c}",
                            r#type: "button",
                            tabindex: "-1",
                            "data-part": "key",
                            "data-key": key.name(),
                            "data-focused": if r == at_row && c == at_col { "true" },
                            "data-selected": if matches!(key.kind, KeyKind::Special(Special::Shift)) && keyboard.shift != Shift::Off { "true" },
                            style: "flex: {key.width};",
                            onclick: {
                                let input = input.clone();
                                move |event: MouseEvent| {
                                    // The field keeps focus: neither Blitz's
                                    // click (which clears it) nor the root's
                                    // (which moves it) runs.
                                    event.prevent_default();
                                    event.stop_propagation();
                                    typer.tap(&input, r, c);
                                }
                            },
                            {
                                keyboard
                                    .label(key)
                                    .unwrap_or_else(|| label_of(key.kind))
                            }
                        }
                    }
                }
            }
        }
    }
}

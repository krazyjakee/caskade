//! A text field's edit commands (#84) for its context menu and for a pad
//! (#93): select all, copy, cut and paste run in Blitz as the field's own
//! shortcuts do, and the field's listeners hear the `input` a cut makes.

use super::Input;
use blitz_dom::{NodeId, TextFieldCommand};
use blitz_traits::events::DomEventData;
use dioxus_native::prelude::*;
use std::rc::Rc;

impl Input {
    /// Runs `command` on the text field `field`. Copy and select all change
    /// nothing the page hears; a cut fires `input`. A paste focuses the
    /// field and asks for the clipboard, whose text arrives there as typing
    /// does (read-only fields don't take it).
    // The text field's menu (#93) calls it.
    pub fn field_command(&self, field: NodeId, command: TextFieldCommand) {
        let mut inputs = Vec::new();
        self.write(|doc| {
            doc.text_field_command(field, command, |event| {
                if let DomEventData::Input(data) = event.data {
                    inputs.push((event.target, data.value));
                }
            })
        });
        for (target, value) in inputs {
            let data = Rc::new(PlatformEventData::new(dioxus_native::synthetic_form_event(
                &value,
                Vec::new(),
            )));
            self.fire(target, "input", data, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{InputRoot, use_input};
    use blitz_dom::DocumentConfig;
    use blitz_traits::shell::{ColorScheme, Viewport};
    use dioxus_native::DioxusDocument;
    use std::cell::RefCell;

    thread_local! {
        static INPUT: RefCell<Option<Input>> = const { RefCell::new(None) };
        static HEARD: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    #[component]
    fn Field() -> Element {
        let input = use_input();
        INPUT.with(|slot| *slot.borrow_mut() = Some(input));
        let mut text = use_signal(|| "hello".to_string());
        rsx! {
            input {
                id: "field",
                value: "{text}",
                oninput: move |event: FormEvent| {
                    HEARD.with(|heard| heard.borrow_mut().push(event.value()));
                    text.set(event.value());
                },
            }
        }
    }

    fn app() -> Element {
        rsx! {
            InputRoot { Field {} }
        }
    }

    fn settle(doc: &mut DioxusDocument) {
        for _ in 0..5 {
            blitz_dom::Document::poll(doc, None);
            doc.inner.borrow_mut().resolve(0.0);
        }
    }

    #[test]
    fn a_cut_from_the_menu_is_heard_as_typing() {
        let mut doc = DioxusDocument::new(
            VirtualDom::new(app),
            DocumentConfig {
                viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Dark)),
                ..Default::default()
            },
        );
        doc.initial_build();
        settle(&mut doc);
        let field = doc.inner.borrow().get_element_by_id("field").unwrap();
        let input = INPUT.with(|slot| slot.borrow().clone()).unwrap();
        // Running on the document's own runtime, as a menu's handler does.
        doc.vdom.in_runtime(|| {
            input.field_command(field, TextFieldCommand::SelectAll);
            input.field_command(field, TextFieldCommand::Copy);
            input.field_command(field, TextFieldCommand::Cut);
        });
        settle(&mut doc);
        assert_eq!(HEARD.with(|heard| heard.borrow().clone()), [""]);
        let doc = doc.inner.borrow();
        let value = doc
            .get_node(field)
            .and_then(|node| node.element_data())
            .and_then(|element| element.text_input_data())
            .map(|input| input.editor.text().to_string());
        assert_eq!(value.as_deref(), Some(""));
    }
}

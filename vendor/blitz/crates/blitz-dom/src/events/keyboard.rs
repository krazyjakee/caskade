use crate::events::focus::generate_focus_events;
use crate::{BaseDocument, node::GeneratedTextInputEvent, util::ACTION_MOD};
use blitz_traits::node_id::NodeId;
use blitz_traits::{
    SmolStr,
    events::{BlitzCompositionEvent, BlitzInputEvent, BlitzKeyEvent, DomEvent, DomEventData},
};
use keyboard_types::{Key, Modifiers, NamedKey};
use markup5ever::local_name;

pub(super) enum KeyboardOrTextInputEvent {
    KeyPress(BlitzKeyEvent),
    AppleStandardKeyBinding(SmolStr),
}

pub(crate) fn handle_key_or_input_event<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: NodeId,
    event: KeyboardOrTextInputEvent,
    mut dispatch_event: F,
) {
    if let KeyboardOrTextInputEvent::KeyPress(event) = &event {
        if event.key == Key::Named(NamedKey::Tab) {
            let backwards = event.modifiers.contains(Modifiers::SHIFT);
            generate_focus_events(
                doc,
                &mut |doc| {
                    if backwards {
                        doc.focus_prev_node();
                    } else {
                        doc.focus_next_node();
                    }
                },
                &mut dispatch_event,
            );
            return;
        }

        // Handle copy (Ctrl+C/Cmd+C) for text selection when no text input is focused
        if event.state.is_pressed() {
            let action_mod = event.modifiers.contains(ACTION_MOD);
            if action_mod {
                if let Key::Character(c) = &event.key {
                    if c.to_lowercase() == "c" {
                        // Check if we have a text selection (and no focused text input)
                        let has_focused_text_input = doc.focus_node_id.is_some_and(|id| {
                            doc.get_node(id)
                                .and_then(|n| n.element_data())
                                .is_some_and(|e| e.text_input_data().is_some())
                        });

                        if !has_focused_text_input {
                            if let Some(text) = doc.get_selected_text() {
                                let _ = doc.shell_provider.set_clipboard_text(text);
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(node_id) = doc.focus_node_id {
        if target != node_id {
            return;
        }

        let node = &mut doc.nodes[node_id];
        let Some(element_data) = node.element_data_mut() else {
            return;
        };

        let flags = element_data.text_field_flags();
        if let Some(input_data) = element_data.text_input_data_mut() {
            let generated_event = match event {
                KeyboardOrTextInputEvent::KeyPress(blitz_key_event) => input_data
                    .apply_keypress_event(
                        &mut doc.font_ctx.lock().unwrap(),
                        &mut doc.layout_ctx,
                        &*doc.shell_provider,
                        blitz_key_event,
                        flags,
                    ),
                KeyboardOrTextInputEvent::AppleStandardKeyBinding(command) => input_data
                    .apply_apple_standard_keybinding(
                        &mut doc.font_ctx.lock().unwrap(),
                        &mut doc.layout_ctx,
                        &*doc.shell_provider,
                        &command,
                        flags,
                    ),
            };

            if let Some(generated_event) = generated_event {
                doc.apply_generated_text_input_event(node_id, generated_event, dispatch_event);
            }
        }
    }
}

/// A text field's edit commands, as a context menu offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFieldCommand {
    SelectAll,
    Copy,
    Cut,
    Paste,
}

impl BaseDocument {
    /// Runs `command` on the text field `node_id`, as its keyboard shortcut
    /// would: `dispatch_event` gets the DOM events it generates (`input`
    /// after a cut, focus events when a paste focuses the field). A paste
    /// asks the shell for the clipboard; its text arrives as a paste in the
    /// focused field, so the field is focused first. Read-only fields only
    /// select and copy, and passwords don't copy. Returns whether the node
    /// is a text field.
    pub fn text_field_command(
        &mut self,
        node_id: NodeId,
        command: TextFieldCommand,
        mut dispatch_event: impl FnMut(DomEvent),
    ) -> bool {
        let Some(element) = self.nodes.get(node_id).and_then(|node| node.element_data()) else {
            return false;
        };
        if element.text_input_data().is_none() {
            return false;
        }
        let flags = element.text_field_flags();
        if command == TextFieldCommand::Paste {
            if !flags.read_only {
                super::focus::generate_focus_events(
                    self,
                    &mut |doc| {
                        doc.set_focus_to(node_id);
                    },
                    &mut dispatch_event,
                );
                let _ = self.shell_provider.request_clipboard_text();
            }
            return true;
        }
        let shell_provider = self.shell_provider.clone();
        let font_ctx = self.font_ctx.clone();
        let mut font_ctx = font_ctx.lock().unwrap();
        let Some(input) = self.nodes[node_id]
            .element_data_mut()
            .and_then(|element| element.text_input_data_mut())
        else {
            return false;
        };
        let mut driver = input.editor.driver(&mut font_ctx, &mut self.layout_ctx);
        let generated = match command {
            TextFieldCommand::SelectAll => {
                driver.select_all();
                Some(GeneratedTextInputEvent::Select)
            }
            TextFieldCommand::Copy => {
                if let Some(text) = driver.editor.selected_text().filter(|_| !flags.password) {
                    let _ = shell_provider.set_clipboard_text(text.to_owned());
                }
                None
            }
            TextFieldCommand::Cut => match driver.editor.selected_text() {
                Some(text) if !flags.password && !flags.read_only => {
                    let _ = shell_provider.set_clipboard_text(text.to_owned());
                    driver.delete_selection();
                    Some(GeneratedTextInputEvent::Input)
                }
                _ => None,
            },
            TextFieldCommand::Paste => unreachable!(),
        };
        drop(font_ctx);
        if let Some(generated) = generated {
            self.apply_generated_text_input_event(node_id, generated, dispatch_event);
        }
        true
    }

    pub(crate) fn apply_generated_text_input_event<F: FnMut(DomEvent)>(
        &mut self,
        node_id: NodeId,
        event: GeneratedTextInputEvent,
        mut dispatch_event: F,
    ) {
        let node = &mut self.nodes[node_id];
        let element_data = node
            .element_data_mut()
            .expect("apply_generated_text_input_event called on a node that is not an element");
        let input_data = element_data
            .text_input_data_mut()
            .expect("apply_generated_text_input_event called on a node that is not a text input");

        match event {
            GeneratedTextInputEvent::Input => {
                // The value without an IME's preedit, which isn't text yet.
                let value = input_data.editor.text().to_string();
                // The value is in the accessibility tree.
                self.has_changes = true;
                dispatch_event(DomEvent::new(
                    node_id,
                    DomEventData::Input(BlitzInputEvent { value }),
                ));
                self.shell_provider.request_redraw();
            }
            GeneratedTextInputEvent::Select | GeneratedTextInputEvent::PreEditChange => {
                self.shell_provider.request_redraw();
            }
            GeneratedTextInputEvent::CompositionStart => {
                dispatch_event(DomEvent::new(
                    node_id,
                    DomEventData::CompositionStart(BlitzCompositionEvent {
                        data: String::new(),
                    }),
                ));
            }
            GeneratedTextInputEvent::CompositionUpdate(data) => {
                dispatch_event(DomEvent::new(
                    node_id,
                    DomEventData::CompositionUpdate(BlitzCompositionEvent { data }),
                ));
            }
            GeneratedTextInputEvent::CompositionEnd(data) => {
                dispatch_event(DomEvent::new(
                    node_id,
                    DomEventData::CompositionEnd(BlitzCompositionEvent { data }),
                ));
                self.shell_provider.request_redraw();
            }
            GeneratedTextInputEvent::Submit => {
                // TODO: Generate submit event that can be handled by script
                implicit_form_submission(self, node_id);
            }
        }
    }
}

/// https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#field-that-blocks-implicit-submission
fn implicit_form_submission(doc: &BaseDocument, text_target: NodeId) {
    let Some(form_owner_id) = doc.controls_to_form.get(&text_target) else {
        return;
    };
    if doc
        .controls_to_form
        .iter()
        .filter(|(_control_id, form_id)| *form_id == form_owner_id)
        .filter_map(|(control_id, _)| doc.nodes[*control_id].element_data())
        .filter(|element_data| {
            element_data.attr(local_name!("type")).is_some_and(|t| {
                matches!(
                    t,
                    "text"
                        | "search"
                        | "email"
                        | "url"
                        | "tel"
                        | "password"
                        | "date"
                        | "month"
                        | "week"
                        | "time"
                        | "datetime-local"
                        | "number"
                )
            })
        })
        .count()
        > 1
    {
        return;
    }

    doc.submit_form(*form_owner_id, *form_owner_id);
}

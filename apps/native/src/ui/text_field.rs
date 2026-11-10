//! Text input, as `@caskade/ui`'s `TextField`: a label, a box with an
//! optional icon around a Blitz-native `<input>` (caret, selection, IME and
//! clipboard are Blitz's, #84), an optional control beside it, help text
//! and the validation message. Its edit menu (Cut, Copy, Paste, Select all)
//! opens from the context-menu key, Shift+F10 or a pad's Start, so a pad
//! user has the clipboard too. [`SearchField`] is one with a magnifier and
//! a clear button.

use blitz_dom::TextFieldCommand;
use caskade_ui_i18n::t;
use caskade_ui_input::Action;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::icons::{Icon, SEARCH, Svg, X};
use super::ids::use_id;
use super::interact::{flag, use_forced};
use super::menu::{Menu, MenuEntry, MenuItem};
use super::popover::Anchor;
use crate::input::{Bind, use_input, use_input_action};

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("label", "The visible label, when given."),
        ("control", "The box around the icon and input."),
        ("icon", "The leading icon."),
        (
            "input",
            "The `<input>`; carries `data-focused` and `data-hovered`.",
        ),
        (
            "action",
            "The control beside the input, e.g. a Browse\u{2026} button.",
        ),
        ("description", "The help text."),
        ("error", "The validation message."),
    ],
    interaction: &[
        "data-disabled",
        "data-invalid",
        "data-readonly",
        "data-required",
    ],
    slots: &["action"],
    ..Entry::new(
        "text-field",
        "TextField, SearchField (apps/native/src/ui/text_field.rs)",
        Source::Ui,
        "A single-line text input.",
    )
};

#[component]
pub fn TextField(
    value: String,
    on_input: EventHandler<String>,
    #[props(default)] label: Option<String>,
    #[props(default)] description: Option<String>,
    /// The validation message; the field is invalid while there is one.
    #[props(default)]
    error: Option<String>,
    #[props(default)] placeholder: Option<String>,
    #[props(default)] icon: Option<Icon>,
    /// Monospace, for paths and identifiers.
    #[props(default)]
    mono: bool,
    /// `search`, `password`, `email`…; `text` by default.
    #[props(default = "text")]
    kind: &'static str,
    #[props(default)] disabled: bool,
    #[props(default)] readonly: bool,
    #[props(default)] required: bool,
    /// Enter, unless it confirms an IME composition.
    #[props(default)]
    on_submit: Option<EventHandler<String>>,
    /// The `<input>`, once mounted.
    #[props(default)]
    on_mounted: Option<EventHandler<Event<MountedData>>>,
    /// Keys before the field's own (a combobox's arrows).
    #[props(default)]
    on_key_down: Option<EventHandler<KeyboardEvent>>,
    /// A control beside the input, e.g. a Browse… button.
    #[props(default)]
    action: Option<Element>,
    /// Shown at the end of the box, e.g. a clear button.
    #[props(default)]
    trailing: Option<Element>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let input = use_input();
    let label_id = use_id("field-label");
    let help_id = use_id("field-help");
    let mut field = use_signal(|| None::<NodeHandle>);
    let mut menu = use_signal(|| None::<Anchor>);
    let forced = use_forced();
    let mut hovered = use_signal(|| forced.hovered);
    let mut focused = use_signal(|| forced.focused || forced.focus_visible);
    let invalid = error.is_some();
    let mut open_menu = move || {
        if let Some(anchor) = field.peek().as_ref().and_then(Anchor::of) {
            menu.set(Some(anchor));
        }
    };
    let node = field.peek().as_ref().map(NodeHandle::node_id);
    use_input_action(
        Action::Menu,
        Bind {
            owner: node,
            label: Some(t!("Edit")),
            ..Bind::default()
        },
        move || {
            open_menu();
            true
        },
    );
    let mut items = Vec::new();
    if !readonly {
        items.push(MenuEntry::Item(MenuItem::new("cut", t!("Cut"))));
    }
    items.push(MenuEntry::Item(MenuItem::new("copy", t!("Copy"))));
    if !readonly {
        items.push(MenuEntry::Item(MenuItem::new("paste", t!("Paste"))));
    }
    items.push(MenuEntry::Separator);
    items.push(MenuEntry::Item(MenuItem::new(
        "select-all",
        t!("Select all"),
    )));
    let run = move |id: String| {
        let command = match id.as_str() {
            "cut" => TextFieldCommand::Cut,
            "copy" => TextFieldCommand::Copy,
            "paste" => TextFieldCommand::Paste,
            _ => TextFieldCommand::SelectAll,
        };
        if let Some(node) = field.peek().as_ref().map(NodeHandle::node_id) {
            input.field_command(node, command);
        }
    };
    let described = description.is_some() || error.is_some();
    rsx! {
        div {
            "data-cs": "text-field",
            "data-disabled": flag(disabled),
            "data-invalid": flag(invalid),
            "data-readonly": flag(readonly),
            "data-required": flag(required),
            class: "group flex flex-col gap-1.5 data-disabled:opacity-40 {class}",
            if let Some(label) = &label {
                label { id: "{label_id}", "data-part": "label", class: "text-sm font-medium", "{label}" }
            }
            div { class: "flex gap-2",
                div {
                    "data-part": "control",
                    // Its input's focus, for its border: Blitz has no
                    // `:has()`, and `:focus-within` doesn't see the
                    // gallery's forced focus.
                    "data-focused": flag(focused()),
                    class: "flex h-9 flex-1 items-center gap-2 rounded-control border border-border-strong bg-surface-base/60 px-3 transition-[border-color,box-shadow] duration-(--cs-motion-duration-fast) focus-within:border-accent-alt/60 focus-within:shadow-[0_0_16px_-6px] focus-within:shadow-accent-alt console:focus-within:focus-ring data-focused:border-accent-alt/60 data-focused:shadow-[0_0_16px_-6px] data-focused:shadow-accent-alt console:data-focused:focus-ring group-data-invalid:border-danger/70",
                    if let Some(icon) = icon {
                        Svg { icon, part: "icon", class: "size-4 shrink-0 text-fg-subtle" }
                    }
                    input {
                        "data-part": "input",
                        r#type: kind,
                        value: "{value}",
                        placeholder: placeholder.clone(),
                        disabled,
                        readonly,
                        required,
                        "aria-labelledby": label.as_ref().map(|_| label_id.clone()),
                        "aria-describedby": described.then(|| help_id.clone()),
                        "aria-invalid": flag(invalid),
                        "data-focused": flag(focused()),
                        "data-hovered": flag(hovered()),
                        "data-focus-visible": flag(forced.focus_visible),
                        class: if mono { "w-full min-w-0 bg-transparent font-mono text-xs placeholder:text-fg-subtle focus:outline-none" } else { "w-full min-w-0 bg-transparent text-sm placeholder:text-fg-subtle focus:outline-none" },
                        onmounted: move |event: Event<MountedData>| {
                            field.set(event.data().downcast::<NodeHandle>().cloned());
                            if let Some(on_mounted) = on_mounted {
                                on_mounted.call(event);
                            }
                        },
                        oninput: move |event| on_input.call(event.value()),
                        onfocus: move |_| focused.set(true),
                        onblur: move |_| focused.set(false),
                        onmouseenter: move |_| hovered.set(true),
                        onmouseleave: move |_| hovered.set(false),
                        onkeydown: move |event: KeyboardEvent| {
                            if let Some(on_key_down) = on_key_down {
                                on_key_down.call(event.clone());
                                if !event.propagates() {
                                    return;
                                }
                            }
                            let shift_f10 = event.key() == Key::Named(NamedKey::F10) && event.modifiers().shift();
                            if event.key() == Key::Named(NamedKey::ContextMenu) || shift_f10 {
                                event.prevent_default();
                                event.stop_propagation();
                                open_menu();
                            } else if event.key() == Key::Named(NamedKey::Enter)
                                && !event.is_composing()
                                && let Some(on_submit) = on_submit
                            {
                                on_submit.call(value.clone());
                            }
                        },
                        ..attributes,
                    }
                    if let Some(trailing) = trailing {
                        {trailing}
                    }
                }
                if let Some(action) = action {
                    div { "data-part": "action", class: "flex shrink-0", {action} }
                }
            }
            if described {
                div { id: "{help_id}", class: "flex flex-col gap-1",
                    if let Some(description) = &description {
                        p { "data-part": "description", class: "text-xs text-fg-muted", "{description}" }
                    }
                    if let Some(error) = &error {
                        p { "data-part": "error", class: "text-xs text-danger", "{error}" }
                    }
                }
            }
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items: items.clone(),
                    label: t!("Edit"),
                    return_to: node,
                    on_action: run,
                    on_close: move |_| menu.set(None),
                }
            }
        }
    }
}

/// A search box: a text field with a magnifier and, when there's text, a
/// clear button. Escape clears it too.
#[component]
pub fn SearchField(
    value: String,
    on_input: EventHandler<String>,
    /// Its name, for screen readers ("Search your library").
    label: String,
    #[props(default)] placeholder: Option<String>,
    #[props(default)] on_submit: Option<EventHandler<String>>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let has_text = !value.is_empty();
    // Its name joins the caller's attributes: a component spread can't
    // sit beside a literal attribute.
    let mut attributes = attributes;
    attributes.push(Attribute::new("aria-label", label, None, false));
    rsx! {
        TextField {
            value,
            on_input,
            kind: "search",
            icon: SEARCH,
            placeholder,
            on_submit,
            class,
            on_key_down: move |event: KeyboardEvent| {
                if event.key() == Key::Named(NamedKey::Escape) && has_text {
                    event.prevent_default();
                    event.stop_propagation();
                    on_input.call(String::new());
                }
            },
            trailing: has_text.then(|| rsx! {
                button {
                    r#type: "button",
                    "aria-label": t!("Clear search"),
                    class: "grid size-5 shrink-0 place-items-center rounded-full text-fg-subtle hover:text-fg data-focus-visible:focus-ring",
                    onclick: move |_| on_input.call(String::new()),
                    Svg { icon: X, class: "size-3.5" }
                }
            }),
            attributes,
        }
    }
}

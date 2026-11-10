//! A combobox, as react-aria's `ComboBox`: a text field that filters a list
//! of options as you type (`role="combobox"`, `aria-autocomplete="list"`).
//! Focus stays in the field; ArrowDown/ArrowUp (or a pad's down and up)
//! move the active option (`aria-activedescendant`), Enter or A chooses it,
//! Escape or Back closes the list. No catalogued root (the webview's search
//! box, `search`, is the app shell's, #96).

use blitz_dom::NodeId;
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::icons::{CHECK, Svg};
use super::ids::use_id;
use super::popover::{Anchor, Popover};
use super::select::SelectOption;
use super::text_field::TextField;
use crate::input::{Bind, use_input_action};

/// The active option of a list a text field drives without leaving it (a
/// combobox, the app's search; the WAI-ARIA APG's combobox): ArrowDown and
/// ArrowUp, or a pad's down and up on the field, open the list or move the
/// active option (wrapping); Enter or a pad's A choose it while the list is
/// open; Escape closes it. Give the field [`ActiveOption::key_down`] and
/// `aria-activedescendant` from [`ActiveOption::active_id`].
#[derive(Clone, Copy)]
pub struct ActiveOption {
    active: Signal<usize>,
    /// The options shown now and whether the list is open, as of the last
    /// render (the pad's bindings outlive a render).
    shown: CopyValue<(usize, bool)>,
    on_open: Callback<()>,
    on_choose: Callback<usize>,
    on_close: Callback<()>,
}

/// The active option among `count` (the options shown now), for the list
/// that's `open` under `field` (the text field's node, which the pad's
/// keys belong to). `on_open` opens the list, `on_choose` gets the chosen
/// option's index, `on_close` closes it.
pub fn use_active_option(
    count: usize,
    open: bool,
    field: Option<NodeId>,
    mut on_open: impl FnMut() + 'static,
    on_choose: impl FnMut(usize) + 'static,
    mut on_close: impl FnMut() + 'static,
) -> ActiveOption {
    let active = use_signal(|| 0usize);
    let mut shown = use_hook(|| CopyValue::new((count, open)));
    shown.set((count, open));
    let options = ActiveOption {
        active,
        shown,
        on_open: use_callback(move |()| on_open()),
        on_choose: use_callback(on_choose),
        on_close: use_callback(move |()| on_close()),
    };
    for (action, forward) in [(Action::NavDown, true), (Action::NavUp, false)] {
        use_input_action(
            action,
            Bind {
                owner: field,
                // The list is an overlay; focus stays in the field.
                global: true,
                ..Bind::default()
            },
            move || {
                options.step(forward);
                true
            },
        );
    }
    use_input_action(
        Action::Confirm,
        Bind {
            owner: field,
            global: true,
            ..Bind::default()
        },
        move || options.choose(),
    );
    options
}

impl ActiveOption {
    /// The active option's index, while there is one.
    pub fn index(&self) -> Option<usize> {
        let (count, open) = (self.shown)();
        (open && count > 0).then(|| (self.active)().min(count - 1))
    }

    /// `aria-activedescendant`: the active option's id, `<list_id>-<index>`.
    pub fn active_id(&self, list_id: &str) -> Option<String> {
        self.index().map(|index| format!("{list_id}-{index}"))
    }

    /// The options changed (typing): the first is active.
    pub fn reset(mut self) {
        self.active.set(0);
    }

    /// Opens the list on its first option, or moves the active one.
    pub fn step(mut self, forward: bool) {
        let (count, open) = (self.shown)();
        if !open {
            self.active.set(0);
            self.on_open.call(());
        } else if count > 0 {
            let at = (*self.active.peek()).min(count - 1);
            self.active.set(if forward {
                (at + 1) % count
            } else {
                (at + count - 1) % count
            });
        }
    }

    /// Chooses the active option, if the list is open; whether it did.
    pub fn choose(self) -> bool {
        match self.index() {
            Some(index) => {
                self.on_choose.call(index);
                true
            }
            None => false,
        }
    }

    /// The field's keys; stops the ones it took.
    pub fn key_down(self, event: &KeyboardEvent) {
        let handled = match event.key() {
            Key::Named(NamedKey::ArrowDown) => {
                self.step(true);
                true
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.step(false);
                true
            }
            Key::Named(NamedKey::Enter) if (self.shown)().1 && !event.is_composing() => {
                self.choose()
            }
            Key::Named(NamedKey::Escape) if (self.shown)().1 => {
                self.on_close.call(());
                true
            }
            _ => false,
        };
        if handled {
            event.prevent_default();
            event.stop_propagation();
        }
    }
}

#[component]
pub fn Combobox(
    options: Vec<SelectOption>,
    /// The chosen option's value.
    value: Option<String>,
    on_change: EventHandler<String>,
    label: String,
    #[props(default)] placeholder: Option<String>,
    #[props(default)] class: String,
) -> Element {
    let list_id = use_id("combobox-list");
    let chosen_label = options
        .iter()
        .find(|option| Some(&option.value) == value.as_ref())
        .map(|option| option.label.clone())
        .unwrap_or_default();
    let mut text = use_signal(|| chosen_label.clone());
    let mut open = use_signal(|| None::<Anchor>);
    let mut field = use_signal(|| None::<NodeHandle>);
    let query = text().to_lowercase();
    let matches: Vec<SelectOption> = options
        .iter()
        .filter(|option| !option.disabled && option.label.to_lowercase().contains(&query))
        .cloned()
        .collect();
    let show = move || {
        if open.peek().is_none()
            && let Some(anchor) = field.peek().as_ref().and_then(Anchor::of)
        {
            open.set(Some(anchor));
        }
    };
    let chosen: Vec<(String, String)> = matches
        .iter()
        .map(|option| (option.value.clone(), option.label.clone()))
        .collect();
    let active = use_active_option(
        matches.len(),
        open().is_some(),
        field.peek().as_ref().map(NodeHandle::node_id),
        show,
        move |index| {
            if let Some((value, label)) = chosen.get(index) {
                text.set(label.clone());
                on_change.call(value.clone());
            }
            open.set(None);
        },
        move || open.set(None),
    );
    let active_index = active.index();
    rsx! {
        div { class: "relative {class}",
            TextField {
                value: text(),
                label: label.clone(),
                placeholder,
                role: "combobox",
                "aria-autocomplete": "list",
                "aria-expanded": if open().is_some() { "true" } else { "false" },
                "aria-controls": "{list_id}",
                "aria-activedescendant": active.active_id(&list_id),
                on_mounted: move |event: Event<MountedData>| field.set(event.data().downcast::<NodeHandle>().cloned()),
                on_input: move |typed: String| {
                    text.set(typed);
                    active.reset();
                    if open.peek().is_none() {
                        active.step(true);
                    }
                },
                on_key_down: move |event: KeyboardEvent| active.key_down(&event),
            }
            if let Some(anchor) = open() {
                Popover {
                    anchor,
                    on_close: move |_| open.set(None),
                    data_cs: "select-popover",
                    role: "listbox",
                    keep_focus: true,
                    match_width: true,
                    id: "{list_id}",
                    "aria-label": "{label}",
                    class: "z-40 max-h-[60vh] overflow-y-auto rounded-control border border-border-strong bg-surface-overlay p-1 shadow-xl shadow-black/40 outline-none",
                    for (index, option) in matches.iter().enumerate() {
                        div {
                            key: "{option.value}",
                            id: "{list_id}-{index}",
                            role: "option",
                            "aria-selected": if Some(&option.value) == value.as_ref() { "true" } else { "false" },
                            "data-part": "option",
                            "data-focused": (Some(index) == active_index).then_some("true"),
                            "data-selected": (Some(&option.value) == value.as_ref()).then_some("true"),
                            class: "flex cursor-default items-center gap-2 rounded-[6px] px-2.5 py-1.5 text-sm text-fg-muted data-focused:bg-surface-hover data-focused:text-fg data-selected:text-fg",
                            onclick: move |_| { active.on_choose.call(index); },
                            span { class: "grow truncate", "{option.label}" }
                            if Some(&option.value) == value.as_ref() {
                                Svg { icon: CHECK, part: "check", class: "size-4 shrink-0 text-accent-alt" }
                            }
                        }
                    }
                }
            }
        }
    }
}

//! The keyboard map: keys produce the same actions pads send. Ported from
//! the webview's `input/actions.ts`.

use crate::Action;
use crate::tree::{FocusTree, closest};

/// The modifier keys held with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The action a key gives, by its DOM `key` name.
pub fn mapped(key: &str) -> Option<Action> {
    Some(match key {
        "ArrowUp" => Action::NavUp,
        "ArrowDown" => Action::NavDown,
        "ArrowLeft" => Action::NavLeft,
        "ArrowRight" => Action::NavRight,
        "Enter" => Action::Confirm,
        "Escape" | "Backspace" => Action::Back,
        "[" => Action::TabPrev,
        "]" => Action::TabNext,
        "PageUp" => Action::PagePrev,
        "PageDown" => Action::PageNext,
        "/" | "F3" => Action::Search,
        "i" => Action::Details,
        "ContextMenu" | "m" => Action::Menu,
        "v" => Action::View,
        _ => return None,
    })
}

/// Input types that don't take typing.
const NOT_TYPED: [&str; 8] = [
    "checkbox", "radio", "button", "submit", "reset", "range", "color", "file",
];

/// Fields where keys keep their typing meaning.
pub fn is_text_entry<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    match tree.tag(node) {
        "textarea" => true,
        "input" => !NOT_TYPED.contains(&tree.attr(node, "type").unwrap_or("text")),
        _ => is_content_editable(tree, node),
    }
}

/// Whether `node` is editable text: `contenteditable` holds for what's
/// inside, unless switched off there.
fn is_content_editable<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    closest(tree, node, |at| tree.attr(at, "contenteditable").is_some())
        .and_then(|at| tree.attr(at, "contenteditable"))
        .is_some_and(|value| value == "true" || value.is_empty())
}

/// A slider: it takes left and right itself.
pub fn is_slider<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    tree.attr(node, "role") == Some("slider")
        || (tree.tag(node) == "input" && tree.attr(node, "type") == Some("range"))
}

/// The keyboard's action for `key` pressed on `target`, or `None` to leave
/// the key alone: modifiers, and keys that edit or move the caret in a text
/// field. Up and down still leave a field, and Escape still goes back.
pub fn key_action<T: FocusTree + ?Sized>(
    tree: &T,
    target: Option<T::Node>,
    key: &str,
    modifiers: Modifiers,
) -> Option<Action> {
    if modifiers.ctrl || modifiers.meta || modifiers.alt {
        return None;
    }
    let action = mapped(key)?;
    let Some(target) = target else {
        return Some(action);
    };
    if is_text_entry(tree, target) {
        let kept = matches!(action, Action::NavUp | Action::NavDown)
            || (action == Action::Back && key == "Escape");
        return kept.then_some(action);
    }
    if is_slider(tree, target) && matches!(action, Action::NavLeft | Action::NavRight) {
        return None;
    }
    Some(action)
}

/// Whether `key` pressed on `target` starts a search, and the text it
/// starts with: a printable character typed outside a text field, a menu, a
/// popover or a dialog; `/` starts an empty one. Keys the keyboard map turns
/// into actions are handled before this is asked (the webview's
/// `startsTyping`).
pub fn starts_typing<T: FocusTree + ?Sized>(
    tree: &T,
    target: Option<T::Node>,
    key: &str,
    modifiers: Modifiers,
) -> Option<String> {
    if modifiers.ctrl || modifiers.meta || modifiers.alt {
        return None;
    }
    if key.chars().count() != 1 || key.trim().is_empty() {
        return None;
    }
    if let Some(target) = target {
        if is_text_entry(tree, target) {
            return None;
        }
        let inside = closest(tree, target, |at| {
            matches!(
                tree.attr(at, "role"),
                Some("dialog" | "alertdialog" | "menu" | "listbox")
            ) || tree.attr(at, "data-placement").is_some()
        });
        if inside.is_some() {
            return None;
        }
    }
    Some(if key == "/" {
        String::new()
    } else {
        key.to_owned()
    })
}

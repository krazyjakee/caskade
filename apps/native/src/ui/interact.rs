//! Pointer and key state, as react-aria gives the webview's components and
//! themes select it: `data-hovered` while a pointer is over the control,
//! `data-pressed` while it's held (a pointer, or Space), and activation by
//! click, Enter and Space as the WAI-ARIA APG has it. A pad's A reaches a
//! control as a click (the input layer's), so `onclick` covers it.

use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

/// A control's interaction state.
#[derive(Clone, Copy, PartialEq)]
pub struct Interaction {
    hovered: Signal<bool>,
    pressed: Signal<bool>,
    focused: Signal<bool>,
    /// The focus ring, as the gallery shows it (the input layer sets the
    /// attribute itself otherwise).
    ring: bool,
}

/// States the gallery (#93) shows the controls under it in, without a
/// pointer, keys or a pad: provided as a context, read when a control
/// mounts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Forced {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    /// `data-focus-visible`, which the input layer sets when keys or a pad
    /// move focus.
    pub focus_visible: bool,
}

/// The states forced on the controls here: none outside the gallery.
pub fn use_forced() -> Forced {
    use_hook(|| try_consume_context::<Forced>().unwrap_or_default())
}

/// The attribute value for a flag: present when on, absent when off, as
/// react-aria sets its `data-*` states.
pub fn flag(on: bool) -> Option<&'static str> {
    on.then_some("true")
}

pub fn use_interaction() -> Interaction {
    use_interaction_where(true)
}

/// An item of a composite control (a radio, a tab, a menu item): forced
/// states reach only the one `here` (the one focus would land on).
pub fn use_interaction_where(here: bool) -> Interaction {
    let forced = if here {
        use_forced()
    } else {
        use_hook(Forced::default)
    };
    Interaction {
        hovered: use_signal(|| forced.hovered),
        pressed: use_signal(|| forced.pressed),
        focused: use_signal(|| forced.focused || forced.focus_visible),
        ring: forced.focus_visible,
    }
}

/// How a key acts on a control.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Keys {
    /// Buttons and links: Enter on key down, Space on key up.
    EnterAndSpace,
    /// Checkboxes and switches: Space only, as native inputs.
    Space,
    /// Links: Enter only.
    Enter,
}

impl Interaction {
    pub fn hovered(&self) -> Option<&'static str> {
        flag((self.hovered)())
    }

    pub fn pressed(&self) -> Option<&'static str> {
        flag((self.pressed)())
    }

    /// `data-focused`: focus is on the control (by any means; the input
    /// layer adds `data-focus-visible` when keys or a pad put it there).
    pub fn focused(&self) -> Option<&'static str> {
        flag((self.focused)())
    }

    /// `data-focus-visible` when the gallery forces it; the input layer
    /// sets it otherwise, which this leaves alone.
    pub fn ring(&self) -> Option<&'static str> {
        flag(self.ring)
    }

    pub fn focus(mut self) {
        self.focused.set(true);
    }

    pub fn blur(mut self) {
        self.focused.set(false);
        self.pressed.set(false);
    }

    pub fn enter(mut self) {
        self.hovered.set(true);
    }

    pub fn leave(mut self) {
        self.hovered.set(false);
        self.pressed.set(false);
    }

    pub fn down(mut self) {
        self.pressed.set(true);
    }

    pub fn up(mut self) {
        self.pressed.set(false);
    }

    /// A key went down on the control: whether it was the control's (the
    /// event then goes no further, so the input layer's keyboard map
    /// doesn't act on it too). Enter activates at once.
    pub fn key_down(mut self, event: &KeyboardEvent, keys: Keys, activate: impl FnOnce()) -> bool {
        match event.key() {
            Key::Named(NamedKey::Enter) if keys != Keys::Space => {
                stop(event);
                activate();
                true
            }
            Key::Character(space) if space == " " && keys != Keys::Enter => {
                stop(event);
                if !event.is_auto_repeating() {
                    self.pressed.set(true);
                }
                true
            }
            _ => false,
        }
    }

    /// A key came up: Space activates, if it went down here.
    pub fn key_up(mut self, event: &KeyboardEvent, activate: impl FnOnce()) {
        if matches!(event.key(), Key::Character(space) if space == " ") {
            stop(event);
            if *self.pressed.peek() {
                self.pressed.set(false);
                activate();
            }
        }
    }
}

fn stop(event: &KeyboardEvent) {
    event.prevent_default();
    event.stop_propagation();
}

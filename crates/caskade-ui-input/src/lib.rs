//! The native UI's input rules (#92), free of any UI toolkit, ported from the
//! webview's `apps/desktop/src/input`:
//!
//! - [`tree`]: what the rules read of the UI, a tree of elements with
//!   attributes and layout boxes ([`FocusTree`]). The native app reads Blitz's
//!   document through it; tests build one by hand.
//! - [`spatial`]: the next focusable in a direction, by geometry, within
//!   `data-focus-section`s that remember where focus was, and inside the
//!   topmost dialog or popover.
//! - [`keys`]: the keyboard's actions, which leave a field's typing keys
//!   alone.
//! - [`bindings`]: who handles an action: the innermost binding around focus.
//! - [`dispatch`]: what an action does when no binding takes it.
//! - [`osk`]: the on-screen keyboard's layouts and state: how a pad types.
//!
//! Pads' actions come from `caskade-input` ([`Action`]); the keyboard map
//! produces the same ones.

pub mod bindings;
pub mod dispatch;
pub mod keys;
pub mod osk;
pub mod spatial;
pub mod tree;

#[cfg(test)]
mod tests;

pub use caskade_input::{Action, GlyphFamily};
pub use tree::{FocusTree, Rect};

/// What sent an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSource {
    Pad,
    Keyboard,
    /// A tap on a touch screen (console mode).
    Touch,
}

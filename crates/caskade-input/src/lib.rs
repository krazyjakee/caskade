//! Console mode's input path (TECH-PLAN §6): gamepads are read natively with
//! `gilrs`, so input works in every webview and while the window is
//! unfocused, and raw pad events become a small set of [`Action`]s. The
//! frontend consumes the actions, and its keyboard map produces the same
//! ones.
//!
//! [`Normalizer`] is pure (it takes the time as an argument), so tests feed
//! it synthetic event streams. [`InputService`] runs it on its own thread
//! over the `gilrs` backend and injected events.

mod action;
mod normalize;
mod service;

pub use action::{Action, GlyphFamily, InputAction, PadInfo, Source};
pub use normalize::{Button, Normalizer, PadId, Raw, Stick, Timing};
pub use service::{InputEvent, InputService};

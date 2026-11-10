//! The component library (#93): `@caskade/ui` and react-aria's behaviour in
//! RSX, on the input layer (`crate::input`) and `caskade_ui_i18n`.
//!
//! Every component renders the webview's anatomy (`data-cs`, `data-part`,
//! the state and interaction attributes themes select), declares its
//! [`Entry`] beside its code ([`ANATOMY`], checked against the webview's
//! catalogue in `crate::anatomy`), and behaves as the WAI-ARIA APG has it:
//! Enter and Space as native controls take them, a pad's A as a click (the
//! input layer's), Back and Escape through `use_input_action`.

// The gallery (#93) and the views (#96 onwards) use them.
#![allow(dead_code)]

mod button;
mod checkbox;
mod icons;
mod ids;
mod interact;
mod link;
mod switch;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use button::{Button, ButtonSize, ButtonVariant};
#[allow(unused_imports)]
pub use checkbox::Checkbox;
#[allow(unused_imports)]
pub use icons::{Icon, Svg};
#[allow(unused_imports)]
pub use link::Link;
#[allow(unused_imports)]
pub use switch::{Switch, SwitchSize};

use caskade_ui_theme::anatomy::Entry;

/// The components' anatomy entries.
pub const ANATOMY: &[Entry] = &[button::ANATOMY, checkbox::ANATOMY, switch::ANATOMY];

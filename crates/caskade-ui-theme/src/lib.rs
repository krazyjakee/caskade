//! The native UI's look (#90), free of any UI toolkit, as the webview has
//! it: Appearance settings as the root element's attributes and tokens
//! ([`appearance`]), a user accent kept readable on the theme
//! ([`accent`]), the user CSS check ([`user_css`]) and the anatomy
//! catalogue's entries and generators ([`anatomy`]). The native app
//! applies them to Blitz's document; themes themselves come compiled from
//! `caskade_core::themes::ThemeService`, as they reach the webview.

pub mod accent;
pub mod anatomy;
pub mod appearance;
pub mod user_css;

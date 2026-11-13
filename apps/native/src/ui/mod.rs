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

mod age_lock;
mod avatar;
mod button;
mod checkbox;
mod combobox;
mod dialog;
mod error_notice;
mod failure;
mod game_art;
mod game_card;
mod game_row;
mod hero;
pub(crate) mod icons;
mod ids;
pub(crate) mod interact;
mod link;
mod menu;
pub(crate) mod popover;
mod progress;
mod radio;
mod segmented;
mod select;
mod settings;
mod shelf;
mod slider;
mod status_badge;
mod switch;
mod tabs;
#[cfg(test)]
mod tests;
mod text_field;
mod toast;
mod tooltip;

pub use age_lock::AgeLockLabel;
pub use avatar::{Avatar, AvatarPicker, AvatarSize, DEFAULT_AVATAR, avatars};
pub use button::{Button, ButtonSize, ButtonVariant};
pub use checkbox::Checkbox;
pub use combobox::use_active_option;
pub use dialog::{ConfirmDialog, Dialog};
pub use error_notice::{ErrorNotice, NoticeVariant};
pub use failure::FailureNotice;
pub use game_art::GameArt;
pub use game_card::{CardGame, GameCard, platform_line};
pub use game_row::{GameRow, TABLE_COLUMNS};
pub use hero::Hero;
pub use icons::{Icon, Svg};
pub use link::Link;
pub use menu::{Menu, MenuButton, MenuEntry, MenuItem};
pub use popover::{Anchor, Popover};
pub use progress::Progress;
pub use radio::{RadioGroup, RadioOption};
pub use segmented::{Segment, SegmentedControl};
pub use select::{Select, SelectOption};
pub use settings::{DotTone, Found, Pill, PillTone, SettingRow, SettingsSection, StatusDot, fold};
pub use shelf::Shelf;
pub use slider::Slider;
pub use status_badge::{GameStatus, StatusBadge};
pub use switch::{Switch, SwitchSize};
pub use tabs::{Tab, Tabs};
pub use text_field::{SearchField, TextField};
pub use toast::{ToastRegion, Toasts, use_toasts};
// Only the gallery shows these.
#[cfg_attr(not(any(debug_assertions, feature = "gallery")), allow(unused_imports))]
pub(crate) use {
    avatar::AvatarInfo, combobox::Combobox, game_card::CardVariant, hero::HeroGame,
    interact::Forced, select::List as SelectList, tooltip::Tooltip,
};

use caskade_ui_theme::anatomy::Entry;

/// The components' anatomy entries.
pub const ANATOMY: &[Entry] = &[
    avatar::ANATOMY,
    avatar::PICKER_ANATOMY,
    button::ANATOMY,
    checkbox::ANATOMY,
    error_notice::ANATOMY,
    game_art::ANATOMY,
    game_card::ANATOMY,
    game_row::ANATOMY,
    hero::ANATOMY,
    menu::ANATOMY,
    select::ANATOMY,
    select::POPOVER_ANATOMY,
    settings::SECTION_ANATOMY,
    settings::ROW_ANATOMY,
    settings::DOT_ANATOMY,
    settings::PILL_ANATOMY,
    segmented::ANATOMY,
    shelf::ANATOMY,
    slider::ANATOMY,
    status_badge::ANATOMY,
    switch::ANATOMY,
    text_field::ANATOMY,
];

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
mod interact;
mod link;
mod menu;
mod popover;
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

#[allow(unused_imports)]
pub use avatar::{
    Avatar, AvatarInfo, AvatarPicker, AvatarSize, DEFAULT_AVATAR, avatar_info, avatars, initials,
};
#[allow(unused_imports)]
pub use button::{Button, ButtonSize, ButtonVariant};
#[allow(unused_imports)]
pub use checkbox::Checkbox;
#[allow(unused_imports)]
pub use combobox::{ActiveOption, Combobox, use_active_option};
#[allow(unused_imports)]
pub use dialog::{ConfirmDialog, Dialog};
#[allow(unused_imports)]
pub use error_notice::{ErrorNotice, NoticeVariant};
#[allow(unused_imports)]
pub use failure::FailureNotice;
#[allow(unused_imports)]
pub use game_art::{GameArt, art_background};
#[allow(unused_imports)]
pub use game_card::{CardGame, CardVariant, GameCard, platform_line};
#[allow(unused_imports)]
pub use game_row::GameRow;
#[allow(unused_imports)]
pub use hero::{Hero, HeroGame};
#[allow(unused_imports)]
pub use icons::{Icon, Svg};
#[allow(unused_imports)]
pub use interact::Forced;
#[allow(unused_imports)]
pub use link::Link;
#[allow(unused_imports)]
pub use menu::{Menu, MenuButton, MenuEntry, MenuItem};
#[allow(unused_imports)]
pub use popover::{Anchor, Popover};
#[allow(unused_imports)]
pub use progress::Progress;
#[allow(unused_imports)]
pub use radio::{RadioGroup, RadioOption};
#[allow(unused_imports)]
pub use segmented::{Segment, SegmentedControl};
#[allow(unused_imports)]
pub(crate) use select::List as SelectList;
#[allow(unused_imports)]
pub use select::{Select, SelectOption};
#[allow(unused_imports)]
pub use settings::{DotTone, Pill, PillTone, SettingRow, SettingsSection, StatusDot};
#[allow(unused_imports)]
pub use shelf::Shelf;
#[allow(unused_imports)]
pub use slider::Slider;
#[allow(unused_imports)]
pub use status_badge::{GameStatus, StatusBadge};
#[allow(unused_imports)]
pub use switch::{Switch, SwitchSize};
#[allow(unused_imports)]
pub use tabs::{Tab, Tabs};
#[allow(unused_imports)]
pub use text_field::{SearchField, TextField};
#[allow(unused_imports)]
pub use toast::{ToastRegion, Toasts, use_toasts};
#[allow(unused_imports)]
pub use tooltip::Tooltip;

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

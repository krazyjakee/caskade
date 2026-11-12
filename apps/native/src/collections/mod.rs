//! Collections (#102, the webview's `views/CollectionsView.tsx` and
//! `views/CollectionView.tsx`): every collection as a card under All, Smart
//! and Manual tabs, with its menu at a right click, the menu key or a pad's
//! Start; and one collection's page, with its rules, Play Random, Add Games
//! or Edit Rules, its menu, and its games in the library's virtual grid.
//! Making and editing one is the library's `CollectionDialog`; a game's
//! menu puts it in one (`game_menu::collections`).

mod actions;
mod add_games;
mod anatomy;
mod card;
mod delete;
mod list;
mod page;
mod rules;
#[cfg(test)]
mod tests;

pub use anatomy::ANATOMY;
pub use list::CollectionsView;
pub use page::CollectionView;

use crate::ui::Icon;
use crate::ui::interact::{Keys, flag, use_interaction};
use dioxus_native::prelude::*;

/// Bumped whenever this UI changes a collection. The core sends no event
/// for collections (the webview refetches every collection query after its
/// own changes), so the collection queries here read it and load again.
static REVISION: GlobalSignal<u64> = Signal::global(|| 0);

/// A collection changed here (made, edited, pinned, deleted, its games):
/// the collection views load again. `game_menu::collections`' picker
/// changes collections too.
pub(crate) fn changed() {
    *REVISION.write() += 1;
}

/// [`REVISION`], read where a query or render should follow it.
fn revision() -> u64 {
    *REVISION.read()
}

/// Lucide's `pin`: pinned collections.
const PIN: Icon = Icon(&[
    "M12 17v5",
    "M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z",
]);

/// Lucide's `pin-off`: Unpin.
const PIN_OFF: Icon = Icon(&[
    "M12 17v5",
    "M15 9.34V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H7.89",
    "m2 2 20 20",
    "M9 9v1.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h11",
]);

/// Lucide's `shuffle`: Play Random.
const SHUFFLE: Icon = Icon(&[
    "m18 14 4 4-4 4",
    "m18 2 4 4-4 4",
    "M2 18h1.973a4 4 0 0 0 3.3-1.7l5.454-8.6a4 4 0 0 1 3.3-1.7H22",
    "M2 6h1.972a4 4 0 0 1 3.6 2.2",
    "M22 18h-6.041a4 4 0 0 1-3.3-1.8l-.359-.45",
]);

/// A button styled wholly by the view, as the webview's plain buttons and
/// react-aria `Button`s are (New Collection, Play Random, More): no
/// `data-cs`, the interaction attributes react-aria gives, and Enter,
/// Space, a click or a pad's A to press it.
#[component]
fn PlainButton(
    #[props(default)] disabled: bool,
    #[props(default)] on_press: Option<EventHandler<()>>,
    /// The element, for a menu's anchor.
    #[props(default)]
    on_mounted: Option<EventHandler<Event<MountedData>>>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let interaction = use_interaction();
    let press = move || {
        if !disabled && let Some(on_press) = on_press {
            on_press.call(());
        }
    };
    rsx! {
        button {
            r#type: "button",
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            "data-disabled": flag(disabled),
            "aria-disabled": flag(disabled),
            class: "{class}",
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onclick: move |_| press(),
            onmounted: move |event| {
                if let Some(on_mounted) = on_mounted {
                    on_mounted.call(event);
                }
            },
            onkeydown: move |event: KeyboardEvent| {
                interaction.key_down(&event, Keys::EnterAndSpace, press);
            },
            onkeyup: move |event| interaction.key_up(&event, press),
            ..attributes,
            {children}
        }
    }
}

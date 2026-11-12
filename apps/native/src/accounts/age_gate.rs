//! Age limits on play (#209), as the webview's `views/game/AgeGate.tsx`: a
//! game above the signed-in account's age limit starts only after an
//! admin's PIN. The core decides and checks every launch; Play and a
//! release's tool ask it first, and a launch it still refuses asks again.
//! An approval covers one start or stays for this account and game; the
//! core keeps it, and the PIN never stays here.

use caskade_core::igdb::IgdbAgeRating;
use caskade_ui_data::accounts::AgeRestriction;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::use_accounts;
use crate::data::change;
use crate::flows::pin_pad::{Check, PinPad};
use crate::game::{rating_name, shown_rating, use_preferred_board};
use crate::ui::icons::LOCK;
use crate::ui::{Dialog, RadioGroup, RadioOption, Svg};

/// "An admin PIN is needed to play Doom, rated PEGI 18": the preferred
/// board's rating.
pub fn age_gate_title(restriction: &AgeRestriction, board: &str) -> String {
    let game = restriction.title.clone();
    if restriction.min_age.is_none() {
        return t!(
            "An admin PIN is needed to play {game}, which is unrated",
            game = game
        );
    }
    match shown_rating(&restriction.ratings, board) {
        Some(shown) => t!(
            note = "{rating} is an age rating, e.g. PEGI 18",
            "An admin PIN is needed to play {game}, rated {rating}",
            game = game,
            rating = rating_name(shown)
        ),
        None => t!("An admin PIN is needed to play {game}", game = game),
    }
}

/// The admin's PIN for `restriction`, with how long the approval lasts;
/// `on_approved` once a PIN matched, to launch.
#[component]
pub fn AgeGateDialog(
    restriction: AgeRestriction,
    on_close: EventHandler<()>,
    on_approved: EventHandler<()>,
) -> Element {
    let accounts = use_accounts();
    let board = use_preferred_board();
    let mut always = use_signal(|| false);
    let title = age_gate_title(&restriction, &board);
    let check = {
        let (game_id, account_id) = (restriction.game_id.clone(), restriction.account_id.clone());
        Check::new(move |pin: String| {
            let accounts = accounts.clone();
            let (game_id, account_id) = (game_id.clone(), account_id.clone());
            let permanent = *always.peek();
            async move {
                change(async move {
                    accounts
                        .approve_play(&game_id, &account_id, pin, permanent)
                        .await
                })
                .await
                .map_err(|report| report.summary)
            }
        })
    };
    let ratings: Vec<IgdbAgeRating> = restriction.ratings.clone();
    let shown = shown_rating(&ratings, &board).cloned();
    let all = ratings
        .iter()
        .map(rating_name)
        .collect::<Vec<_>>()
        .join(", ");
    rsx! {
        Dialog {
            title: title.clone(),
            on_close: move |_| on_close.call(()),
            class: "max-h-[85vh] max-w-sm overflow-y-auto",
            div { class: "flex flex-col items-center gap-4",
                Svg { icon: LOCK, class: "size-6 text-fg-muted" }
                if let Some(shown) = shown {
                    span { title: "{all}", class: "font-mono text-sm", {rating_name(&shown)} }
                }
                p { class: "text-center text-sm text-fg-muted",
                    {t!(
                        "Approve {game} for {account}.",
                        game = restriction.title.clone(),
                        account = restriction.account_name.clone()
                    )}
                }
                RadioGroup {
                    id: "age-gate-scope",
                    label: t!("Approval duration"),
                    class: "w-full",
                    options: vec![
                        RadioOption::new("once", t!("Unlock this time")),
                        RadioOption::new("always", t!("Always allow this game for this account")),
                    ],
                    value: Some(if always() { "always" } else { "once" }.to_owned()),
                    on_change: move |value: String| always.set(value == "always"),
                }
                PinPad {
                    title,
                    show_title: false,
                    check,
                    on_matched: move |_| on_approved.call(()),
                    on_cancel: move |_| on_close.call(()),
                }
            }
        }
    }
}

/// "Admin PIN needed" beside a game's rating when the signed-in
/// account's age limit holds it back, with why in its tooltip; nothing
/// otherwise. Asked again after another account signs in.
#[component]
pub fn AgeLock(
    game_id: String,
    /// After the rating in a line, with " · " before it.
    #[props(default)]
    separated: bool,
) -> Element {
    let restriction = {
        let game_id = game_id.clone();
        crate::data::use_settings_query(
            caskade_ui_data::Key::Game(game_id.clone()),
            move |settings| {
                let game_id = game_id.clone();
                async move { settings.accounts().age_restriction(&game_id).await }
            },
        )
    };
    let board = use_preferred_board();
    let Some(Ok(Some(restriction))) = restriction.read().clone() else {
        return rsx! {};
    };
    rsx! {
        if separated {
            " · "
        }
        crate::ui::AgeLockLabel { title: age_gate_title(&restriction, &board) }
    }
}

//! A game page's pictures and videos (#100, the webview's
//! `views/game/Screenshots.tsx`, `Videos.tsx` and
//! `components/AgeRatingLabel.tsx`): its screenshots in a grid, one at a
//! time filling the window with a filmstrip; IGDB's videos, each playing
//! in the video window; and its age rating, on the board an admin prefers.

use super::data::{use_answer, use_page_data};
use crate::data::use_settings_query;
use crate::input::{Bind, use_input_action};
use crate::ui::{Button, ButtonVariant, Dialog, Svg, icons};
use caskade_core::igdb::{IgdbAgeRating, IgdbVideo};
use caskade_core::model::{Artwork, ArtworkKind};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::t;
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

/// Empty slots the grid keeps while a game has fewer screenshots.
const RESERVED: usize = 4;

/// One screenshot: its 320 px thumbnail and its original.
#[derive(Debug, Clone, PartialEq)]
pub struct Shot {
    pub thumb: String,
    pub full: String,
}

/// A game's screenshots, in their order.
pub fn shots(artwork: &[Artwork]) -> Vec<Shot> {
    artwork
        .iter()
        .filter(|art| art.kind == ArtworkKind::Screenshot)
        .filter_map(|art| {
            let thumb = crate::library::art_url_card(Some(art))?;
            let full = crate::library::art_url_original(art).unwrap_or_else(|| thumb.clone());
            Some(Shot { thumb, full })
        })
        .collect()
}

#[component]
pub fn Screenshots(title: String, shots: Vec<Shot>) -> Element {
    let mut open = use_signal(|| None::<usize>);
    let none = shots.is_empty();
    rsx! {
        div { class: "relative mt-4",
            ul {
                "data-part": "screenshots",
                "aria-label": t!("Screenshots"),
                class: "grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4",
                for (index, shot) in shots.iter().enumerate() {
                    li { key: "{shot.full}",
                        button {
                            r#type: "button",
                            "data-part": "screenshot",
                            "data-spatial": "focusable",
                            class: "relative block w-full overflow-hidden rounded-control border border-border outline-none data-focus-visible:focus-ring-over hover:border-border-strong",
                            onclick: move |_| open.set(Some(index)),
                            img {
                                src: "{shot.thumb}",
                                alt: t!(note = "{title} is the game's title", "{title}, screenshot {number}", title = title.clone(), number = (index + 1).to_string()),
                                class: "aspect-video w-full object-cover",
                            }
                        }
                    }
                }
                for slot in 0..RESERVED.saturating_sub(shots.len()) {
                    li {
                        key: "slot-{slot}",
                        "aria-hidden": "true",
                        "data-part": "screenshot-slot",
                        "data-status": "empty",
                        class: "aspect-video w-full rounded-control border border-border bg-surface-raised",
                    }
                }
            }
            if none {
                p { "data-part": "screenshots-empty", class: "pointer-events-none absolute inset-0 grid place-items-center text-sm text-fg-muted",
                    {t!("No screenshots")}
                }
            }
            if let Some(start) = open() {
                ScreenshotViewer { title: title.clone(), shots: shots.clone(), start, on_close: move |_| open.set(None) }
            }
        }
    }
}

/// The screenshots one at a time, filling the window, with a filmstrip.
/// The arrow keys, Home and End, a pad's left and right or LB and RB step
/// through them (round from the last to the first); B or Escape closes it.
#[component]
pub fn ScreenshotViewer(
    title: String,
    shots: Vec<Shot>,
    start: usize,
    on_close: EventHandler<()>,
) -> Element {
    let count = shots.len();
    let mut at = use_signal(|| start.min(count.saturating_sub(1)));
    let many = count > 1;
    let mut go = move |by: isize| {
        if count == 0 {
            return;
        }
        let next = (*at.peek() as isize + by).rem_euclid(count as isize) as usize;
        at.set(next);
    };
    for (action, by) in [(Action::TabPrev, -1isize), (Action::TabNext, 1)] {
        use_input_action(
            action,
            Bind {
                label: many.then(|| {
                    if by < 0 {
                        t!(note = "Goes to the previous screenshot", "Previous")
                    } else {
                        t!(note = "Goes to the next screenshot", "Next")
                    }
                }),
                global: true,
                ..Bind::default()
            },
            move || {
                if !many {
                    return false;
                }
                go(by);
                true
            },
        );
    }
    let Some(shot) = shots.get(at()).cloned() else {
        return rsx! {};
    };
    let number = at() + 1;
    rsx! {
        Dialog {
            title: t!(note = "{title} is the game's title", "{title}, screenshots", title = title.clone()),
            "data-cs": "screenshot-viewer",
            class: "max-w-none! h-[96vh] w-[96vw]",
            on_close: move |_| on_close.call(()),
            div {
                class: "flex size-full flex-col",
                onkeydown: move |event: KeyboardEvent| {
                    if !many {
                        return;
                    }
                    match event.key() {
                        Key::Named(NamedKey::ArrowLeft) => go(-1),
                        Key::Named(NamedKey::ArrowRight) => go(1),
                        Key::Named(NamedKey::Home) => at.set(0),
                        Key::Named(NamedKey::End) => at.set(count - 1),
                        _ => return,
                    }
                    event.prevent_default();
                    event.stop_propagation();
                },
                p { "data-part": "counter", class: "text-sm text-fg-muted", role: "status",
                    {t!("{number} of {count}", number = number.to_string(), count = count.to_string())}
                }
                div { "data-part": "stage", class: "relative flex min-h-0 flex-1 items-center justify-center gap-2",
                    if many {
                        Button {
                            "data-part": "prev",
                            variant: ButtonVariant::Ghost,
                            "aria-label": t!(note = "Goes to the previous screenshot", "Previous"),
                            on_press: move |_| go(-1),
                            Svg { icon: icons::ARROW_LEFT, class: "size-5" }
                        }
                    }
                    img {
                        "data-part": "image",
                        src: "{shot.full}",
                        alt: t!(note = "{title} is the game's title", "{title}, screenshot {number}", title = title.clone(), number = number.to_string()),
                        class: "max-h-full max-w-full object-contain",
                    }
                    if many {
                        Button {
                            "data-part": "next",
                            variant: ButtonVariant::Ghost,
                            "aria-label": t!(note = "Goes to the next screenshot", "Next"),
                            on_press: move |_| go(1),
                            Svg { icon: icons::ARROW_RIGHT, class: "size-5" }
                        }
                    }
                }
                if many {
                    ol { "data-part": "strip", class: "flex shrink-0 gap-2 overflow-x-auto py-2",
                        for (index, each) in shots.iter().enumerate() {
                            li { key: "{each.full}",
                                button {
                                    r#type: "button",
                                    "data-part": "thumb",
                                    "data-selected": (index == at()).then_some("true"),
                                    "aria-current": if index == at() { "true" } else { "false" },
                                    class: "block w-28 overflow-hidden rounded-control border border-border aria-[current=true]:border-accent-alt",
                                    onclick: move |_| at.set(index),
                                    img { src: "{each.thumb}", alt: "", class: "aspect-video w-full object-cover" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A game's videos (IGDB's trailers) in a row; each plays in the video
/// window. Nothing when the game has none.
#[component]
pub fn Videos(title: String, videos: Vec<IgdbVideo>) -> Element {
    let data = use_page_data();
    let mut error = use_signal(|| None::<String>);
    if videos.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { "data-cs": "videos", "aria-labelledby": "videos-heading", class: "mt-8",
            h2 { id: "videos-heading", class: "mb-3 font-display text-xl font-semibold", {t!("Videos")} }
            ul { "data-part": "items", class: "flex gap-3 overflow-x-auto pb-2",
                for (index, video) in videos.into_iter().enumerate() {
                    {
                        let name = video.name.clone().unwrap_or_else(|| {
                            t!(note = "A game's video IGDB gives no name", "Video {number}", number = (index + 1).to_string())
                        });
                        let full = format!("{title} · {name}");
                        let data = data.clone();
                        let id = video.video_id.clone();
                        rsx! {
                            li { key: "{video.video_id}", class: "w-64 shrink-0 sm:w-72",
                                button {
                                    r#type: "button",
                                    "data-part": "video",
                                    "data-spatial": "focusable",
                                    "aria-label": t!(note = "{name} is a game's video: Trailer, Gameplay video, …", "Play {name}", name = name.clone()),
                                    class: "group block w-full text-left outline-none",
                                    onclick: move |_| {
                                        if let Err(message) = (data.play_video)(id.clone(), full.clone()) {
                                            error.set(Some(message));
                                        }
                                    },
                                    span { "data-part": "thumbnail", class: "relative grid aspect-video w-full place-items-center overflow-hidden rounded-control border border-border bg-surface-raised group-data-focus-visible:focus-ring-over",
                                        span { "data-part": "play", "aria-hidden": "true", class: "grid size-12 place-items-center rounded-full bg-surface-base/75 text-fg",
                                            Svg { icon: icons::PLAY, class: "size-5" }
                                        }
                                    }
                                    span { "data-part": "name", class: "mt-1.5 block truncate text-sm text-fg-muted", "{name}" }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(error) = error() {
                p { role: "alert", class: "mt-2 text-sm text-danger", "{error}" }
            }
        }
    }
}

/// The board shown when none is preferred, and every known one in order.
pub(crate) const DEFAULT_BOARD: &str = "pegi";
pub(crate) const BOARDS: &[(&str, &str)] = &[
    ("pegi", "PEGI"),
    ("esrb", "ESRB"),
    ("usk", "USK"),
    ("cero", "CERO"),
    ("acb", "ACB"),
    ("grac", "GRAC"),
    ("classind", "ClassInd"),
];

pub(crate) fn board_name(board: &str) -> String {
    BOARDS
        .iter()
        .find(|(id, _)| *id == board)
        .map(|(_, name)| (*name).to_owned())
        .unwrap_or_else(|| board.to_uppercase())
}

pub(crate) fn rating_name(rating: &IgdbAgeRating) -> String {
    format!("{} {}", board_name(&rating.board), rating.rating)
}

/// The rating to show: `board`'s, else PEGI's, else the first known
/// board's, else the first.
pub fn shown_rating<'a>(ratings: &'a [IgdbAgeRating], board: &str) -> Option<&'a IgdbAgeRating> {
    std::iter::once(board)
        .chain(std::iter::once(DEFAULT_BOARD))
        .chain(BOARDS.iter().map(|(id, _)| *id))
        .find_map(|wanted| ratings.iter().find(|rating| rating.board == wanted))
        .or(ratings.first())
}

/// The rating board an admin prefers (Settings → Accounts), else PEGI.
pub(crate) fn use_preferred_board() -> String {
    let preferred = use_settings_query(
        DataKey::Settings("core.accounts".to_owned()),
        |settings| async move { settings.get("core.accounts").await },
    );
    preferred
        .read()
        .as_ref()
        .and_then(|snapshot| snapshot.as_ref().ok())
        .and_then(|snapshot| {
            snapshot
                .value
                .get("ratingBoard")
                .and_then(|board| board.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| DEFAULT_BOARD.to_owned())
}

/// A game's age rating, on the board an admin prefers (Settings →
/// Accounts), every board's in its tooltip. Nothing for an unrated game.
#[component]
pub fn AgeRating(game_id: String) -> Element {
    let data = use_page_data();
    let ratings = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.age_ratings)(game_id.clone())
        })
    };
    let board = use_preferred_board();
    let list: Vec<IgdbAgeRating> = ratings
        .read()
        .as_ref()
        .and_then(|ratings| ratings.as_ref().ok().cloned())
        .unwrap_or_default();
    let Some(shown) = shown_rating(&list, &board).cloned() else {
        return rsx! {};
    };
    let all = list.iter().map(rating_name).collect::<Vec<_>>().join(", ");
    rsx! {
        " · "
        span { title: "{all}", class: "font-mono", {rating_name(&shown)} }
    }
}

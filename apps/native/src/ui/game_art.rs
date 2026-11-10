//! A game's cover, or its generated placeholder, as `@caskade/ui`'s
//! `GameArt`. Decorative: the card or row around it names the game.

use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

pub const ANATOMY: Entry = Entry {
    parts: &[("image", "The artwork image.")],
    states: &[("data-source", &["image", "placeholder"])],
    ..Entry::new(
        "game-art",
        "GameArt (apps/native/src/ui/game_art.rs)",
        Source::Ui,
        "A game's cover, or a blank placeholder when it has none.",
    )
};

/// The art behind a game without a cover: the webview's `artBackground`,
/// two hues when it has them.
pub fn art_background(hues: Option<(u16, u16)>) -> String {
    match hues {
        None => {
            "linear-gradient(145deg, var(--cs-color-surface-raised), var(--cs-color-surface-base))"
                .to_owned()
        }
        Some((a, b)) => format!(
            "radial-gradient(120% 90% at 70% 10%, hsl({b} 80% 55% / 0.55), transparent 60%), linear-gradient(160deg, hsl({a} 55% 28%), hsl({b} 60% 12%))"
        ),
    }
}

#[component]
pub fn GameArt(
    /// The cover's URL (`caskade-asset://`), if it has one.
    #[props(default)]
    image: Option<String>,
    #[props(default)] hues: Option<(u16, u16)>,
    /// The `data-part` it plays in a parent component, e.g. `art` in a card.
    #[props(default)]
    part: Option<&'static str>,
    #[props(default)] class: String,
) -> Element {
    let source = if image.is_some() {
        "image"
    } else {
        "placeholder"
    };
    rsx! {
        div {
            "data-cs": "game-art",
            "data-part": part,
            "data-source": source,
            "aria-hidden": "true",
            class: "relative overflow-hidden {class}",
            style: "background: {art_background(hues)};",
            if let Some(image) = image {
                img { "data-part": "image", src: "{image}", alt: "", class: "absolute inset-0 size-full object-cover" }
            }
        }
    }
}

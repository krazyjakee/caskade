//! The editor's Artwork tab (the webview's `gameEditor/ArtworkTab.tsx`):
//! the game's cover, background and icon. Each can show one of the images
//! automatic sources supplied, an image the user imports, or none, and
//! goes back to automatic with Reset. Changes apply at once; refreshes
//! never undo them.

use caskade_app::commands::game_edit::ArtworkChange;
use caskade_core::model::{Artwork, ArtworkChoice, ArtworkKind, ArtworkSlot, USER_ARTWORK_SOURCE};
use caskade_ui_data::Key;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::data::EditData;
use super::{IMAGE_PLUS, flag};
use crate::flows::paths::{PathRequest, PickKind, use_path_picker};
use crate::game::data::use_answer;
use crate::ui::icons::{EYE_OFF, ROTATE_CCW};
use crate::ui::{Button, ButtonSize, ButtonVariant};

const EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

fn kind_name(kind: ArtworkKind) -> String {
    match kind {
        ArtworkKind::Hero => t!("Background"),
        ArtworkKind::Icon => t!("Icon"),
        _ => t!("Cover"),
    }
}

fn kind_id(kind: ArtworkKind) -> &'static str {
    match kind {
        ArtworkKind::Hero => "hero",
        ArtworkKind::Icon => "icon",
        _ => "cover",
    }
}

fn choice_id(choice: ArtworkChoice) -> &'static str {
    match choice {
        ArtworkChoice::Automatic => "automatic",
        ArtworkChoice::Chosen => "chosen",
        ArtworkChoice::Removed => "removed",
    }
}

#[component]
pub fn ArtworkTab(game_id: String, data: EditData) -> Element {
    let slots = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(Key::Game(game_id.clone()), move || {
            (data.artwork)(game_id.clone())
        })
    };
    let slots = slots.read().clone();
    match slots {
        None => rsx! {
            p { class: "text-sm text-fg-muted", {t!("Loading…")} }
        },
        Some(Err(report)) => rsx! {
            p { class: "text-sm text-danger", "{report.summary}" }
        },
        Some(Ok(None)) => rsx! {
            p { class: "text-sm text-fg-muted", {t!("This game is no longer in the library.")} }
        },
        Some(Ok(Some(slots))) => rsx! {
            div { "data-part": "artwork", class: "flex flex-col gap-6",
                for slot in slots {
                    Slot { key: "{kind_id(slot.kind)}", game_id: game_id.clone(), data: data.clone(), slot }
                }
            }
        },
    }
}

#[component]
fn Slot(game_id: String, data: EditData, slot: ArtworkSlot) -> Element {
    let picker = use_path_picker();
    let mut error = use_signal(|| None::<String>);
    let mut pending = use_signal(|| false);
    let name = kind_name(slot.kind);
    let kind = slot.kind;
    let apply = use_callback(move |artwork: ArtworkChange| {
        let (data, game_id) = (data.clone(), game_id.clone());
        error.set(None);
        pending.set(true);
        spawn(async move {
            let result = (data.set_artwork)(game_id, kind, artwork).await;
            pending.set(false);
            if let Err(report) = result {
                error.set(Some(report.summary));
            }
        });
    });
    let choose_file = {
        let name = name.clone();
        move |_| {
            let mut request = PathRequest::new(
                PickKind::File,
                t!("An image for the {name}", name = name.clone()),
            );
            request.extensions = EXTENSIONS.iter().map(|ext| (*ext).to_owned()).collect();
            error.set(None);
            spawn(async move {
                if let Some(path) = picker.pick(request).await {
                    apply(ArtworkChange::Import { path: path.into() });
                }
            });
        }
    };
    let status = match slot.choice {
        ArtworkChoice::Automatic => t!("Automatic"),
        ArtworkChoice::Chosen => t!("Chosen by you"),
        ArtworkChoice::Removed => t!("None, by your choice"),
    };
    let busy = pending();
    let shown_path = slot.shown.as_ref().map(|art| art.cached_path.clone());
    rsx! {
        section {
            "data-part": "slot",
            "data-kind": kind_id(kind),
            "data-choice": choice_id(slot.choice),
            "aria-label": "{name}",
            class: "flex flex-col gap-3 rounded-panel border border-border p-4",
            div { class: "flex flex-wrap items-center gap-2",
                h3 { class: "font-semibold", "{name}" }
                span { "data-part": "choice", class: "text-xs text-fg-muted", "{status}" }
            }
            div { class: "flex flex-wrap items-start gap-4",
                figure {
                    "data-part": "preview",
                    "aria-label": t!("{name} shown now", name = name.clone()),
                    class: "grid shrink-0 place-items-center overflow-hidden rounded-control border border-border bg-surface-base/60 {shape(kind)}",
                    Thumb { image: slot.shown.clone(), kind, fill: true }
                }
                div { class: "flex min-w-0 flex-1 flex-col gap-3",
                    if !slot.automatic.is_empty() {
                        div { class: "flex flex-col gap-1.5",
                            span { class: "text-xs text-fg-subtle", {t!("From your sources")} }
                            ul { class: "flex flex-wrap gap-2",
                                for (index, image) in slot.automatic.iter().cloned().enumerate() {
                                    {
                                        let selected = shown_path.is_some()
                                            && match slot.choice {
                                                ArtworkChoice::Automatic => index == 0,
                                                _ => shown_path.as_deref() == Some(image.cached_path.as_str())
                                                    && !image.cached_path.is_empty(),
                                            };
                                        let artwork_id = image.id.clone();
                                        rsx! {
                                            li { key: "{image.id}",
                                                Button {
                                                    "data-part": "candidate",
                                                    "data-spatial": "focusable",
                                                    "data-selected": flag(selected),
                                                    "aria-pressed": if selected { "true" } else { "false" },
                                                    "aria-label": t!(
                                                        "Use {name} {0} from {source}",
                                                        name = name.clone(),
                                                        0 = index + 1,
                                                        source = image.source.clone()
                                                    ),
                                                    variant: ButtonVariant::Ghost,
                                                    class: if selected { "h-auto p-1 ring-2 ring-accent-alt" } else { "h-auto p-1" },
                                                    disabled: busy,
                                                    on_press: move |_| apply(ArtworkChange::Choose { artwork_id: artwork_id.clone() }),
                                                    Thumb { image: Some(image.clone()), kind, fill: false }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "flex flex-wrap gap-2",
                        Button {
                            "data-part": "import",
                            "data-spatial": "focusable",
                            icon: IMAGE_PLUS,
                            size: ButtonSize::Sm,
                            disabled: busy,
                            on_press: choose_file,
                            {t!("Import image…")}
                        }
                        if slot.choice != ArtworkChoice::Removed {
                            Button {
                                "data-part": "clear",
                                "data-spatial": "focusable",
                                icon: EYE_OFF,
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Ghost,
                                disabled: busy,
                                on_press: move |_| apply(ArtworkChange::Remove),
                                {t!("Show none")}
                            }
                        }
                        if slot.choice != ArtworkChoice::Automatic {
                            Button {
                                "data-part": "reset",
                                "data-spatial": "focusable",
                                icon: ROTATE_CCW,
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Ghost,
                                disabled: busy,
                                "aria-label": t!("Reset {name} to automatic", name = name.clone()),
                                on_press: move |_| apply(ArtworkChange::Reset),
                                {t!("Reset")}
                            }
                        }
                    }
                    if let Some(error) = error() {
                        p { role: "alert", "data-part": "error", class: "text-xs text-danger",
                            {t!("The image wasn't changed: {error}", error = error)}
                        }
                    }
                }
            }
        }
    }
}

fn shape(kind: ArtworkKind) -> &'static str {
    match kind {
        ArtworkKind::Hero => "aspect-video w-56",
        ArtworkKind::Icon => "aspect-square w-16",
        _ => "aspect-[2/3] w-32",
    }
}

/// An image's thumbnail, or what stands in for it.
#[component]
fn Thumb(image: Option<Artwork>, kind: ArtworkKind, fill: bool) -> Element {
    let size = if fill {
        "size-full"
    } else {
        match kind {
            ArtworkKind::Hero => "h-16 w-28",
            ArtworkKind::Icon => "size-10",
            _ => "h-24 w-16",
        }
    };
    let Some(image) = image else {
        return rsx! {
            span { class: "p-2 text-center text-xs text-fg-subtle", {t!("None")} }
        };
    };
    let url = crate::library::art_url_card(Some(&image))
        .or_else(|| crate::library::art_url_original(&image));
    match url {
        Some(url) => rsx! {
            img { src: "{url}", alt: "", class: "object-cover {size}" }
        },
        None => rsx! {
            span { class: "grid place-items-center bg-surface-overlay text-[10px] text-fg-subtle {size}",
                if image.source == USER_ARTWORK_SOURCE {
                    {t!("Your image")}
                } else {
                    "{image.source}"
                }
            }
        },
    }
}

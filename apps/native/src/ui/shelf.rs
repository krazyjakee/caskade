//! A titled row of cards, as `@caskade/ui`'s `Shelf`.

use caskade_ui_i18n::t;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::icons::{ARROW_RIGHT, Icon, Svg};

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("header", "The heading row."),
        ("icon", "The heading icon."),
        ("title", "The heading."),
        ("subtitle", "The text beside the heading."),
        ("view-all", "The View All button."),
        ("items", "The grid of cards."),
    ],
    contains: &["game-card"],
    slots: &["items"],
    ..Entry::new(
        "shelf",
        "Shelf (apps/native/src/ui/shelf.rs)",
        Source::Ui,
        "A titled row of cards.",
    )
};

#[component]
pub fn Shelf(
    title: String,
    icon: Icon,
    #[props(default)] subtitle: Option<String>,
    /// What View All does; the webview's leaves it to the view.
    #[props(default)]
    on_view_all: Option<EventHandler<()>>,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        section {
            "data-cs": "shelf",
            class: "mt-8",
            ..attributes,
            header { "data-part": "header", class: "mb-3 flex items-center gap-3",
                Svg { icon, part: "icon", class: "size-5 text-accent-alt" }
                h2 {
                    "data-part": "title",
                    class: "font-display text-lg font-semibold tracking-wide",
                    "{title}"
                }
                if let Some(subtitle) = &subtitle {
                    span { "data-part": "subtitle", class: "text-sm text-fg-subtle", "{subtitle}" }
                }
                span {
                    "aria-hidden": "true",
                    class: "h-px flex-1 bg-linear-to-r from-border-strong to-transparent",
                }
                button {
                    r#type: "button",
                    "data-part": "view-all",
                    class: "group flex items-center gap-1 font-mono text-xs tracking-wider text-accent-alt uppercase",
                    onclick: move |_| {
                        if let Some(view_all) = on_view_all {
                            view_all.call(());
                        }
                    },
                    {t!("View All")}
                    Svg {
                        icon: ARROW_RIGHT,
                        class: "size-4 transition-transform duration-(--cs-motion-duration-fast) group-hover:translate-x-1",
                    }
                }
            }
            div {
                "data-part": "items",
                class: "grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4",
                {children}
            }
        }
    }
}

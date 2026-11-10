//! The component gallery (#93): every component in `ui` in every state,
//! under any theme, as Storybook shows the webview's `@caskade/ui`.
//! `caskade-native --gallery [--themes <folder>]` (debug builds, or the
//! `gallery` feature) opens it in a window with a theme switcher (the
//! built-in themes and those in `--themes`). Its pages are also screens of
//! the screenshot harness (`--shots`, #90), so `tests/shots.rs` compares
//! every built-in theme's look of every component with committed images.
//!
//! States a pointer, keys or a pad would bring are forced through
//! [`Forced`]; overlays open from the Overlays page, and the screens that
//! show them open close back to it. The text is fixtures, in fixed English,
//! as the reference screens' (`shots/`).

mod pages;

use std::path::PathBuf;
use std::sync::OnceLock;

use caskade_core::settings::AppearanceSettings;
use caskade_core::themes::ThemeService;
use dioxus_native::prelude::*;

use crate::ui::{Forced, Link, Select, SelectOption};

/// The stylesheet: the app's with the gallery's and the reference screens'
/// classes (`scripts/build-css.mjs` leaves `gallery/` out of `app.css`).
pub const CSS: &str = include_str!("../shots/shots.css");

/// A page: what the gallery lists, or reaches from another page.
#[derive(Clone, Copy)]
pub struct Page {
    pub id: &'static str,
    pub title: &'static str,
    body: fn() -> Element,
    /// In the list; the open-overlay pages are reached from Overlays.
    listed: bool,
}

/// Every page, in the list's order.
pub const PAGES: [Page; 14] = [
    Page {
        id: "buttons",
        title: "Buttons and links",
        body: pages::Buttons,
        listed: true,
    },
    Page {
        id: "choices",
        title: "Choices",
        body: pages::Choices,
        listed: true,
    },
    Page {
        id: "values",
        title: "Values and tabs",
        body: pages::Values,
        listed: true,
    },
    Page {
        id: "fields",
        title: "Fields",
        body: pages::Fields,
        listed: true,
    },
    Page {
        id: "selects",
        title: "Selects",
        body: pages::Selects,
        listed: true,
    },
    Page {
        id: "feedback",
        title: "Feedback",
        body: pages::Feedback,
        listed: true,
    },
    Page {
        id: "cards",
        title: "Game cards",
        body: pages::Cards,
        listed: true,
    },
    Page {
        id: "rows",
        title: "Game rows and art",
        body: pages::Rows,
        listed: true,
    },
    Page {
        id: "home",
        title: "Hero and shelf",
        body: pages::Home,
        listed: true,
    },
    Page {
        id: "accounts",
        title: "Avatars",
        body: pages::Accounts,
        listed: true,
    },
    Page {
        id: "overlays",
        title: "Overlays",
        body: pages::Overlays,
        listed: true,
    },
    Page {
        id: "menu",
        title: "Overlays: a menu",
        body: pages::OpenMenu,
        listed: false,
    },
    Page {
        id: "select-list",
        title: "Overlays: a select's list",
        body: pages::OpenSelect,
        listed: false,
    },
    Page {
        id: "dialog",
        title: "Overlays: a dialog",
        body: pages::OpenDialog,
        listed: false,
    },
];

/// The page shown, for pages that move to another.
#[derive(Clone, Copy)]
pub struct Current(pub Signal<&'static str>);

/// The themes the switcher lists: the window's (unset for screenshots).
static THEMES: OnceLock<ThemeService> = OnceLock::new();

/// `--gallery`, when given.
pub fn requested() -> bool {
    std::env::args().any(|arg| arg == "--gallery")
}

/// Opens the gallery's window until it closes.
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let folder = args
        .iter()
        .position(|arg| arg == "--themes")
        .and_then(|at| args.get(at + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("caskade-gallery-themes"));
    let themes = ThemeService::new(folder);
    crate::theme::serve(&themes);
    crate::theme::use_app_css(CSS);
    let look = look_of(&themes, &AppearanceSettings::default().theme);
    let _ = THEMES.set(themes);
    let window = dioxus_native::WindowAttributes::default()
        .with_title("Caskade component gallery")
        .with_surface_size(dioxus_native::LogicalSize::new(1280.0, 800.0));
    crate::launch::run(app, window, &look, Window);
}

fn look_of(themes: &ThemeService, id: &str) -> crate::theme::Look {
    let active = themes.resolve(id);
    crate::theme::look(
        themes,
        &active.id,
        active.modes,
        AppearanceSettings::default(),
        false,
        crate::theme::prefers_reduced_motion(),
    )
}

struct Window;

impl crate::launch::Shell for Window {
    fn on_close(&mut self) -> crate::launch::OnClose {
        crate::launch::OnClose::Quit
    }
}

fn app() -> Element {
    // The window a restyle redraws.
    let window = dioxus_native::use_window();
    use_hook(move || crate::shell::set_main_window(window));
    rsx! {
        Gallery { start: "buttons" }
    }
}

/// The gallery at `start`.
#[component]
pub fn Gallery(start: &'static str) -> Element {
    let mut page = use_signal(|| start);
    use_context_provider(|| Current(page));
    let shown = PAGES
        .iter()
        .find(|candidate| candidate.id == page())
        .copied()
        .unwrap_or(PAGES[0]);
    // An open-overlay page is the Overlays page's.
    let listed = if shown.listed { shown.id } else { "overlays" };
    rsx! {
        crate::input::InputRoot {
            crate::ui::ToastRegion {
                div { class: "flex h-screen bg-surface-base text-fg",
                    nav {
                        "aria-label": "Gallery pages",
                        class: "flex w-56 shrink-0 flex-col gap-1 border-r border-border bg-surface-raised/40 p-4",
                        h1 { class: "mb-3 px-2 text-sm font-semibold", "Component gallery" }
                        for listed_page in PAGES.iter().filter(|candidate| candidate.listed) {
                            Link {
                                key: "{listed_page.id}",
                                href: "#{listed_page.id}",
                                current: listed_page.id == listed,
                                on_press: move |_| page.set(listed_page.id),
                                class: "rounded-control px-2 py-1.5 text-sm text-fg-muted data-hovered:bg-surface-hover data-hovered:text-fg aria-[current=page]:bg-accent/20 aria-[current=page]:text-fg",
                                "{listed_page.title}"
                            }
                        }
                        ThemeSwitcher {}
                    }
                    main { class: "min-w-0 flex-1 overflow-y-auto px-8 py-6",
                        h2 { class: "mb-5 text-xl font-semibold", "{shown.title}" }
                        PageBody { key: "{shown.id}", id: shown.id }
                    }
                }
            }
        }
    }
}

/// A page's own component, so its hooks are its own.
#[component]
fn PageBody(id: &'static str) -> Element {
    PAGES
        .iter()
        .find(|page| page.id == id)
        .map_or_else(|| rsx! {}, |page| (page.body)())
}

/// The theme the window shows (not in screenshots, which show each).
#[component]
fn ThemeSwitcher() -> Element {
    let mut chosen = use_signal(|| AppearanceSettings::default().theme);
    let Some(themes) = THEMES.get() else {
        return rsx! {};
    };
    let options: Vec<SelectOption> = themes
        .list()
        .into_iter()
        .filter(|info| info.problems.is_empty())
        .map(|info| SelectOption::new(info.id, info.name))
        .collect();
    rsx! {
        div { class: "mt-auto",
            Select {
                label: "Theme",
                options,
                value: Some(chosen()),
                on_change: move |id: String| {
                    crate::theme::show(look_of(themes, &id));
                    chosen.set(id);
                },
            }
        }
    }
}

/// A labelled specimen, in the states `forced` gives the controls in it.
#[component]
pub fn Specimen(
    label: &'static str,
    #[props(default)] forced: Forced,
    #[props(default)] class: String,
    children: Element,
) -> Element {
    use_context_provider(|| forced);
    rsx! {
        div { class: "flex flex-col items-start gap-2 {class}",
            span { class: "font-mono text-[10px] tracking-wider text-fg-subtle uppercase", "{label}" }
            {children}
        }
    }
}

/// A group of specimens under a heading.
#[component]
pub fn Section(title: &'static str, #[props(default)] class: String, children: Element) -> Element {
    rsx! {
        section { class: "mb-7",
            h3 { class: "mb-3 text-sm font-semibold text-fg-muted", "{title}" }
            div { class: "flex flex-wrap items-start gap-x-6 gap-y-4 {class}", {children} }
        }
    }
}

/// The states a control is shown in, beside its rest.
pub const HOVERED: Forced = Forced {
    hovered: true,
    pressed: false,
    focused: false,
    focus_visible: false,
};
pub const PRESSED: Forced = Forced {
    hovered: true,
    pressed: true,
    focused: false,
    focus_visible: false,
};
pub const FOCUS_VISIBLE: Forced = Forced {
    hovered: false,
    pressed: false,
    focused: true,
    focus_visible: true,
};

/// Each page as a screenshot screen (`gallery-<page>`).
macro_rules! screens {
    ($($id:literal => $name:ident),* $(,)?) => {
        $(
            #[allow(non_snake_case)]
            fn $name() -> Element {
                rsx! { Gallery { start: $id } }
            }
        )*
        /// The screenshot harness's screens of the gallery.
        #[cfg_attr(not(feature = "shots"), allow(dead_code))]
        pub const SCREENS: &[(&str, fn() -> Element)] = &[$((concat!("gallery-", $id), $name)),*];
    };
}

screens! {
    "buttons" => ShotButtons,
    "choices" => ShotChoices,
    "values" => ShotValues,
    "fields" => ShotFields,
    "selects" => ShotSelects,
    "feedback" => ShotFeedback,
    "cards" => ShotCards,
    "rows" => ShotRows,
    "home" => ShotHome,
    "accounts" => ShotAccounts,
    "menu" => ShotMenu,
    "select-list" => ShotSelectList,
    "dialog" => ShotDialog,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Duration;

    use blitz_dom::{DocumentConfig, LocalName};
    use blitz_traits::shell::{ColorScheme, Viewport};
    use dioxus_native::DioxusDocument;
    use dioxus_native::prelude::*;

    /// The `data-cs` roots a screen renders once it settles.
    fn roots(root: fn() -> Element) -> BTreeSet<String> {
        let mut doc = DioxusDocument::new(
            VirtualDom::new(root),
            DocumentConfig {
                viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
                ..Default::default()
            },
        );
        doc.initial_build();
        for _ in 0..20 {
            blitz_dom::Document::poll(&mut doc, None);
            doc.inner.borrow_mut().resolve(0.0);
            std::thread::sleep(Duration::from_millis(3));
        }
        let inner = doc.inner.borrow();
        let mut found = BTreeSet::new();
        inner.visit(|_, node| {
            if let Some(name) = node.attr(LocalName::from("data-cs")) {
                found.insert(name.to_owned());
            }
        });
        found
    }

    /// Every component the anatomy catalogues is on a gallery screen, so
    /// the screenshots show it under every built-in theme.
    #[test]
    fn the_gallery_shows_every_catalogued_component() {
        let _one = crate::input::tests::ONE_WINDOW
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut shown = BTreeSet::new();
        for (_, root) in super::SCREENS {
            shown.extend(roots(*root));
        }
        let missing: Vec<&str> = crate::ui::ANATOMY
            .iter()
            .map(|entry| entry.name)
            .filter(|name| !shown.contains(*name))
            .collect();
        assert!(missing.is_empty(), "not in the gallery: {missing:?}");
    }
}

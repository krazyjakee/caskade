//! The reference screens (#90): what every theme is rendered on and
//! compared across Blitz and WebKitGTK. They're fixed markup with the app's
//! classes and anatomy, no data and nothing animated, so two renders of a
//! screen are the same pixels.
//!
//! - `preview`: the same library with genres for subtitles, which the
//!   built-in themes' previews are rendered from (#125).
//! - `library`: the shell's sidebar, top bar with search, status bar, the
//!   backdrop and a grid of game cards with placeholder art, as the
//!   webview's.
//!
//! Every text is in the bundled fonts, so a render doesn't depend on the
//! machine's.

use dioxus_native::prelude::*;

/// A reference screen: its name and root.
pub type Screen = (&'static str, fn() -> Element);

/// Every reference screen.
pub const SCREENS: [Screen; 2] = [("library", Library), ("preview", Preview)];

/// A fixture game: title, subtitle, favourite.
type Fixture = (&'static str, Option<&'static str>, bool);

/// The fixture games.
const GAMES: [Fixture; 12] = [
    ("Hollow Pines", None, true),
    ("Star Courier 2", Some("Nintendo 64"), false),
    ("Tidewater", None, false),
    ("Moss & Iron", Some("SNES"), false),
    ("Neon Drift Remastered Collector's Edition", None, true),
    ("Quiet Harbour", Some("Game Boy Advance"), false),
    ("Lantern Keep", None, false),
    ("Riverbend Rally", Some("PlayStation +1"), false),
    ("Cinder Vale", None, false),
    ("Paper Moons", Some("Nintendo DS"), true),
    ("Glasswing", None, false),
    ("Orbit Gardens", None, false),
];

const NAV: [(&str, &str); 4] = [
    ("Home", "M3 10.5 12 3l9 7.5V21H3z"),
    ("Library", "M4 4h6v16H4zM14 4h6v16h-6z"),
    ("Store", "M4 7h16l-1.5 12h-13zM9 7V5a3 3 0 0 1 6 0v2"),
    ("Activity", "M3 12h4l3 8 4-16 3 8h4"),
];

/// The preview's games: the library's, with genres for subtitles, so a
/// theme's preview names no platform (#125).
const PREVIEW_GAMES: [Fixture; 12] = [
    ("Hollow Pines", None, true),
    ("Star Courier 2", Some("Space trading"), false),
    ("Tidewater", None, false),
    ("Moss & Iron", Some("Platformer"), false),
    ("Neon Drift Remastered Collector's Edition", None, true),
    ("Quiet Harbour", Some("Puzzle"), false),
    ("Lantern Keep", None, false),
    ("Riverbend Rally", Some("Racing"), false),
    ("Cinder Vale", None, false),
    ("Paper Moons", Some("Adventure"), true),
    ("Glasswing", None, false),
    ("Orbit Gardens", None, false),
];

const COLUMNS: usize = 4;
const ROW_HEIGHT: f64 = 312.0;

#[allow(non_snake_case)]
fn Library() -> Element {
    rsx! { LibraryOf { games: &GAMES } }
}

/// The library as a built-in theme's preview shows it
/// (`themes/<theme>/previews/home.webp`).
#[allow(non_snake_case)]
fn Preview() -> Element {
    rsx! { LibraryOf { games: &PREVIEW_GAMES } }
}

#[component]
fn LibraryOf(games: &'static [Fixture; 12]) -> Element {
    let rows = games.len().div_ceil(COLUMNS);
    rsx! {
        div { "data-cs": "app-shell", class: "relative flex h-screen flex-col overflow-clip",
            div { "data-cs": "backdrop", "aria-hidden": "true" }
            div { class: "flex min-h-0 flex-1",
                Sidebar {}
                div { "data-part": "main", class: "relative flex min-w-0 flex-1 flex-col",
                    header {
                        "data-cs": "topbar",
                        class: "relative z-10 flex h-topbar shrink-0 items-center gap-4 pr-4 pl-6",
                        div { "data-cs": "search", class: "w-full max-w-xl",
                            label { class: "flex h-11 items-center gap-3 rounded-control border border-border bg-surface-raised px-4 text-sm",
                                SearchIcon {}
                                input {
                                    "aria-label": "Search your library",
                                    class: "w-full bg-transparent focus:outline-none",
                                    value: "pines",
                                }
                            }
                        }
                        div { "data-part": "actions", class: "ml-auto flex items-center gap-4 text-sm text-fg-muted",
                            button { class: "rounded-control border border-border px-3 py-1.5 hover:bg-surface-hover",
                                "Add games"
                            }
                        }
                    }
                    main { "data-cs": "view", class: "min-h-0 flex-1 overflow-y-auto px-6 pb-8",
                        div { class: "flex min-h-full flex-col",
                            div { class: "flex items-baseline gap-3 pt-2 pb-4",
                                h1 { class: "font-display text-2xl font-semibold", "Library" }
                                span { class: "text-sm text-fg-muted", "{games.len()} games" }
                            }
                            section {
                                "data-cs": "virtual-library",
                                "data-layout": "grid",
                                "aria-label": "Library games",
                                class: "relative w-full",
                                style: "height: {rows as f64 * ROW_HEIGHT}px",
                                for row in 0..rows {
                                    div {
                                        key: "{row}",
                                        "data-part": "virtual-row",
                                        class: "absolute top-0 left-0 grid w-full gap-4 pb-4",
                                        style: "height: {ROW_HEIGHT}px; transform: translateY({row as f64 * ROW_HEIGHT}px); grid-template-columns: repeat({COLUMNS}, minmax(0, 1fr))",
                                        for (index, (title, subtitle, favorite)) in games.iter().enumerate().skip(row * COLUMNS).take(COLUMNS) {
                                            GameCard { key: "{index}", title, subtitle: *subtitle, favorite: *favorite, focused: index == 1 }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            footer {
                "data-cs": "status-bar",
                class: "relative z-10 flex min-h-9 shrink-0 items-center gap-4 border-t border-border bg-surface-raised px-4 py-2 text-xs text-fg-muted",
                output { class: "flex min-w-0 flex-1 items-center gap-2",
                    span { "data-part": "activity", class: "truncate", "Library up to date" }
                }
                span { "data-part": "version", class: "shrink-0 font-mono text-fg-subtle", "Caskade 0.1.4" }
            }
        }
    }
}

#[component]
fn GameCard(
    title: &'static str,
    subtitle: Option<&'static str>,
    favorite: bool,
    focused: bool,
) -> Element {
    rsx! {
        div {
            "data-cs": "game-card",
            "data-variant": "library",
            "data-part": "grid-item",
            class: "relative h-full min-w-0 rounded-card",
            a {
                "data-focused": focused.then_some("true"),
                class: "group relative flex h-full flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none hover:bg-surface-hover data-focused:focus-ring-over",
                div {
                    "data-cs": "game-art",
                    "data-part": "art",
                    "data-source": "placeholder",
                    "aria-hidden": "true",
                    class: "relative min-h-0 w-full flex-1 overflow-hidden",
                    style: "background: linear-gradient(145deg, var(--cs-color-surface-raised), var(--cs-color-surface-base))",
                }
                div { class: "h-14 min-w-0 shrink-0 px-3 py-2.5",
                    p { "data-part": "title", class: "truncate text-sm font-semibold", "{title}" }
                    if let Some(subtitle) = subtitle {
                        p { "data-part": "subtitle", class: "truncate text-xs text-fg-subtle", "{subtitle}" }
                    }
                }
                if favorite {
                    span { "data-part": "favorite", class: "absolute top-3 right-3 size-3.5 text-accent-alt", HeartIcon {} }
                }
            }
        }
    }
}

#[component]
fn Sidebar() -> Element {
    let item = "relative flex items-center gap-3 rounded-control px-3 py-2.5 text-sm text-fg-muted hover:translate-x-0.5 hover:bg-surface-hover hover:text-fg data-[status=active]:text-fg";
    rsx! {
        aside {
            "data-cs": "sidebar",
            class: "relative z-10 flex w-sidebar shrink-0 flex-col border-r border-border bg-surface-raised/80 px-4 pb-6",
            div { "data-part": "brand", class: "-mx-4 mb-4 flex h-topbar shrink-0 items-center gap-3 px-6",
                span { class: "font-display text-xl font-semibold tracking-[0.3em]", "CASKADE" }
            }
            nav { "data-part": "nav", class: "flex flex-col gap-1",
                for (label, path) in NAV {
                    a {
                        key: "{label}",
                        "data-part": "nav-item",
                        "data-status": if label == "Library" { "active" } else { "" },
                        class: item,
                        Icon { path }
                        span { "data-part": "label", class: "flex-1", "{label}" }
                    }
                }
            }
            hr { "data-part": "divider", class: "my-4 border-border" }
            nav { "data-part": "nav", class: "flex flex-col gap-1",
                a { "data-part": "nav-item", class: item,
                    Icon { path: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM4 12h2M18 12h2M12 4v2M12 18v2" }
                    span { "data-part": "label", class: "flex-1", "Settings" }
                }
            }
        }
    }
}

#[component]
fn Icon(path: &'static str) -> Element {
    rsx! {
        svg {
            "data-part": "icon",
            class: "size-5 shrink-0",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: path }
        }
    }
}

#[component]
fn SearchIcon() -> Element {
    rsx! {
        svg {
            "data-part": "icon",
            class: "size-4 shrink-0 text-fg-subtle",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            circle { cx: "11", cy: "11", r: "8" }
            path { d: "m21 21-4.3-4.3" }
        }
    }
}

#[component]
fn HeartIcon() -> Element {
    rsx! {
        svg {
            class: "size-3.5",
            view_box: "0 0 24 24",
            fill: "currentColor",
            stroke: "currentColor",
            stroke_width: "2",
            path { d: "M19 14c1.5-1.5 3-3.2 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.8 0-3 .5-4.5 2-1.5-1.5-2.7-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4 3 5.5l7 7z" }
        }
    }
}

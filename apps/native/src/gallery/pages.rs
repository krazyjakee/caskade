//! The gallery's pages: each component in its states, with fixture data.

#![allow(non_snake_case)]

use dioxus_native::prelude::*;

use super::{Current, FOCUS_VISIBLE, Forced, HOVERED, PRESSED, Section, Specimen};
use crate::ui::icons::{CLOCK, GAMEPAD, HARD_DRIVE, HEART, SEARCH, STORE};
use crate::ui::{
    Anchor, Button, ButtonSize, ButtonVariant, CardGame, CardVariant, Checkbox, Combobox,
    ConfirmDialog, Dialog, ErrorNotice, GameArt, GameCard, GameRow, GameStatus, Link, Menu,
    MenuButton, MenuEntry, MenuItem, NoticeVariant, Progress, RadioGroup, RadioOption, SearchField,
    Segment, SegmentedControl, Select, SelectList, SelectOption, Slider, StatusBadge, Switch,
    SwitchSize, Tab, Tabs, TextField, Tooltip, use_toasts,
};
use crate::ui::{
    Avatar, AvatarInfo, AvatarPicker, AvatarSize, DEFAULT_AVATAR, Hero, HeroGame, Shelf, avatars,
};

/// A column heading in a grid of states.
const HEADING: &str = "font-mono text-[10px] tracking-wider text-fg-subtle uppercase";

/// Controls in `forced`'s states, without a label.
#[component]
fn Forcing(forced: Forced, children: Element) -> Element {
    use_context_provider(|| forced);
    children
}

// ── Buttons and links ──

const VARIANTS: [(ButtonVariant, &str); 4] = [
    (ButtonVariant::Primary, "Primary"),
    (ButtonVariant::Secondary, "Secondary"),
    (ButtonVariant::Ghost, "Ghost"),
    (ButtonVariant::Danger, "Danger"),
];

pub fn Buttons() -> Element {
    let states: [(&str, Forced); 4] = [
        ("Rest", Forced::default()),
        ("Hovered", HOVERED),
        ("Pressed", PRESSED),
        ("Focus visible", FOCUS_VISIBLE),
    ];
    rsx! {
        Section { title: "Button",
            div { class: "grid grid-cols-7 items-center justify-items-start gap-x-6 gap-y-3",
                span {}
                for (label, _) in states {
                    span { class: HEADING, "{label}" }
                }
                span { class: HEADING, "Disabled" }
                span { class: HEADING, "Pending" }
                for (variant, name) in VARIANTS {
                    span { class: "text-sm text-fg-muted", "{name}" }
                    for (_, forced) in states {
                        Forcing { forced,
                            Button { variant, "Install" }
                        }
                    }
                    Button { variant, disabled: true, "Install" }
                    Button { variant, pending: true, "Install" }
                }
            }
        }
        Section { title: "Button sizes and icons",
            Specimen { label: "Small",
                Button { size: ButtonSize::Sm, variant: ButtonVariant::Secondary, "Small" }
            }
            Specimen { label: "Medium",
                Button { size: ButtonSize::Md, variant: ButtonVariant::Secondary, "Medium" }
            }
            Specimen { label: "With an icon",
                Button { icon: Some(GAMEPAD), "Play" }
            }
            Specimen { label: "Icon only",
                Button { icon: Some(HEART), variant: ButtonVariant::Ghost, "aria-label": "Favourite" }
            }
            Specimen { label: "Icon only, focus visible", forced: FOCUS_VISIBLE,
                Button { icon: Some(HEART), variant: ButtonVariant::Ghost, "aria-label": "Favourite" }
            }
        }
        Section { title: "Menu button",
            Specimen { label: "Rest",
                MenuButton { items: sort_items(), on_action: move |_| {}, label: "Sort", variant: ButtonVariant::Secondary }
            }
            Specimen { label: "Hovered", forced: HOVERED,
                MenuButton { items: sort_items(), on_action: move |_| {}, label: "Sort", variant: ButtonVariant::Secondary }
            }
            Specimen { label: "Focus visible", forced: FOCUS_VISIBLE,
                MenuButton { items: sort_items(), on_action: move |_| {}, label: "Sort", variant: ButtonVariant::Secondary }
            }
        }
        Section { title: "Link",
            for (label, forced) in [("Rest", Forced::default()), ("Hovered", HOVERED), ("Focus visible", FOCUS_VISIBLE)] {
                Specimen { label, forced,
                    Link { href: "#", class: LINK, "Open the store page" }
                }
            }
            Specimen { label: "Current page",
                Link { href: "#", current: true, class: LINK, "Library" }
            }
        }
        Section { title: "Tooltip",
            Specimen { label: "Hover or focus the button",
                Tooltip { text: "Adds a game from a folder",
                    Button { variant: ButtonVariant::Secondary, "Add game" }
                }
            }
        }
    }
}

const LINK: &str = "rounded-[4px] text-sm text-accent-alt underline-offset-2 data-hovered:underline data-focus-visible:focus-ring aria-[current=page]:font-semibold aria-[current=page]:text-fg";

fn sort_items() -> Vec<MenuEntry> {
    vec![
        MenuEntry::Item(MenuItem::new("title", "Title")),
        MenuEntry::Item(MenuItem::new("played", "Last played")),
        MenuEntry::Item(MenuItem::new("time", "Playtime")),
    ]
}

// ── Choices ──

pub fn Choices() -> Element {
    let mut sync = use_signal(|| Some("lan".to_owned()));
    let states: [(&str, Forced); 3] = [
        ("Rest", Forced::default()),
        ("Hovered", HOVERED),
        ("Focus visible", FOCUS_VISIBLE),
    ];
    let radio_options = vec![
        RadioOption {
            description: Some("Only devices on this network".into()),
            ..RadioOption::new("lan", "Local network")
        },
        RadioOption::new("everywhere", "Everywhere"),
        RadioOption {
            disabled: true,
            ..RadioOption::new("cloud", "Cloud (unavailable)")
        },
    ];
    rsx! {
        Section { title: "Switch and checkbox",
            div { class: "grid grid-cols-6 items-center justify-items-start gap-x-6 gap-y-3",
                span {}
                for (label, _) in states {
                    span { class: HEADING, "{label}" }
                }
                span { class: HEADING, "Disabled" }
                span { class: HEADING, "Read-only" }
                for checked in [false, true] {
                    span { class: "text-sm text-fg-muted", if checked { "Switch, on" } else { "Switch, off" } }
                    for (_, forced) in states {
                        Forcing { forced,
                            Switch { checked, label: "Sync".to_owned() }
                        }
                    }
                    Switch { checked, disabled: true, label: "Sync".to_owned() }
                    Switch { checked, readonly: true, label: "Sync".to_owned() }
                }
                for (name, checked, indeterminate) in [("Checkbox", false, false), ("Checked", true, false), ("Some checked", false, true)] {
                    span { class: "text-sm text-fg-muted", "{name}" }
                    for (_, forced) in states {
                        Forcing { forced,
                            Checkbox { checked, indeterminate, label: "Hidden games".to_owned() }
                        }
                    }
                    Checkbox { checked, indeterminate, disabled: true, label: "Hidden games".to_owned() }
                    Checkbox { checked, indeterminate, readonly: true, label: "Hidden games".to_owned() }
                }
            }
        }
        Section { title: "More switches and checkboxes",
            Specimen { label: "Small switch",
                Switch { checked: true, size: SwitchSize::Sm, label: "Compact".to_owned() }
            }
            Specimen { label: "Invalid checkbox",
                Checkbox { checked: false, invalid: true, label: "I agree".to_owned() }
            }
        }
        Section { title: "Radio group",
            Specimen { label: "Rest",
                RadioGroup { label: "Sync over".to_owned(), options: radio_options.clone(), value: sync(), on_change: move |value| sync.set(Some(value)) }
            }
            Specimen { label: "Hovered", forced: HOVERED,
                RadioGroup { label: "Sync over".to_owned(), options: radio_options.clone(), value: Some("lan".to_owned()), on_change: move |_| {} }
            }
            Specimen { label: "Focus visible", forced: FOCUS_VISIBLE,
                RadioGroup { label: "Sync over".to_owned(), options: radio_options, value: Some("everywhere".to_owned()), on_change: move |_| {} }
            }
        }
    }
}

// ── Values and tabs ──

pub fn Values() -> Element {
    let mut view = use_signal(|| "grid".to_owned());
    let mut volume = use_signal(|| 60.0);
    let mut tab = use_signal(|| "general".to_owned());
    let segments = vec![
        Segment::new("grid", "Grid"),
        Segment::new("list", "List"),
        Segment::new("shelves", "Shelves"),
    ];
    let tabs = vec![
        Tab::new("general", "General"),
        Tab::new("controls", "Controls"),
        Tab::new("saves", "Saves"),
    ];
    rsx! {
        Section { title: "Segmented control",
            for (label, forced) in [("Rest", Forced::default()), ("Hovered", HOVERED), ("Focus visible", FOCUS_VISIBLE)] {
                Specimen { label, forced,
                    SegmentedControl { options: segments.clone(), value: view(), on_change: move |value| view.set(value), "aria-label": "View" }
                }
            }
            Specimen { label: "Disabled",
                SegmentedControl { options: segments.clone(), value: "list", on_change: move |_| {}, disabled: true, "aria-label": "View" }
            }
        }
        Section { title: "Slider",
            for (label, forced) in [("Rest", Forced::default()), ("Hovered", HOVERED), ("Focus visible", FOCUS_VISIBLE)] {
                Specimen { label, forced, class: "w-52",
                    Slider {
                        value: volume(),
                        on_change: move |value| volume.set(value),
                        label: "Volume".to_owned(),
                        format: Callback::new(|value: f64| format!("{value:.0} %")),
                        class: "w-full",
                    }
                }
            }
            Specimen { label: "Disabled", class: "w-52",
                Slider { value: 30.0, on_change: move |_| {}, label: "Volume".to_owned(), disabled: true, class: "w-full" }
            }
        }
        Section { title: "Tabs",
            for (label, forced) in [("Rest", Forced::default()), ("Hovered", HOVERED), ("Focus visible", FOCUS_VISIBLE)] {
                Specimen { label, forced, class: "w-80",
                    Tabs {
                        tabs: tabs.clone(),
                        selected: tab(),
                        on_change: move |id| tab.set(id),
                        label: "Game settings",
                        p { class: "pt-3 text-sm text-fg-muted", "The {tab} tab's panel." }
                    }
                }
            }
        }
    }
}

// ── Fields ──

pub fn Fields() -> Element {
    let mut name = use_signal(|| "Hollow Pines".to_owned());
    let mut query = use_signal(String::new);
    let field = |value: &str| value.to_owned();
    rsx! {
        Section { title: "Text field", class: "[&>*]:w-56",
            Specimen { label: "Empty",
                TextField { value: "", on_input: move |_| {}, label: field("Name"), placeholder: field("A game's name"), class: "w-full" }
            }
            Specimen { label: "Filled",
                TextField { value: name(), on_input: move |value| name.set(value), label: field("Name"), class: "w-full" }
            }
            Specimen { label: "Hovered", forced: HOVERED,
                TextField { value: name(), on_input: move |_| {}, label: field("Name"), class: "w-full" }
            }
            Specimen { label: "Focus visible", forced: FOCUS_VISIBLE,
                TextField { value: name(), on_input: move |_| {}, label: field("Name"), class: "w-full" }
            }
            Specimen { label: "Description",
                TextField { value: name(), on_input: move |_| {}, label: field("Name"), description: field("As the library shows it."), class: "w-full" }
            }
            Specimen { label: "Error",
                TextField { value: "", on_input: move |_| {}, label: field("Name"), error: field("A game needs a name."), class: "w-full" }
            }
            Specimen { label: "Disabled",
                TextField { value: name(), on_input: move |_| {}, label: field("Name"), disabled: true, class: "w-full" }
            }
            Specimen { label: "Read-only",
                TextField { value: name(), on_input: move |_| {}, label: field("Name"), readonly: true, class: "w-full" }
            }
            Specimen { label: "Icon, monospace",
                TextField { value: "/games/hollow-pines", on_input: move |_| {}, label: field("Folder"), icon: Some(HARD_DRIVE), mono: true, class: "w-full" }
            }
            Specimen { label: "With an action",
                TextField {
                    value: "/games",
                    on_input: move |_| {},
                    label: field("Folder"),
                    action: rsx! { Button { variant: ButtonVariant::Secondary, "Browse…" } },
                    class: "w-full",
                }
            }
        }
        Section { title: "Search field", class: "[&>*]:w-56",
            Specimen { label: "Empty",
                SearchField { value: query(), on_input: move |value| query.set(value), label: "Search your library", placeholder: field("Search your library"), class: "w-full" }
            }
            Specimen { label: "With text",
                SearchField { value: "pines", on_input: move |_| {}, label: "Search your library", class: "w-full" }
            }
            Specimen { label: "Focus visible", forced: FOCUS_VISIBLE,
                SearchField { value: "pines", on_input: move |_| {}, label: "Search your library", class: "w-full" }
            }
        }
    }
}

// ── Selects ──

pub fn Selects() -> Element {
    let mut sort = use_signal(|| Some("played".to_owned()));
    let mut platform = use_signal(|| None::<String>);
    let sorts = vec![
        SelectOption::new("title", "Title"),
        SelectOption::new("played", "Last played"),
        SelectOption::new("time", "Playtime"),
    ];
    let platforms = vec![
        SelectOption::new("snes", "SNES"),
        SelectOption::new("n64", "Nintendo 64"),
        SelectOption::new("ds", "Nintendo DS"),
        SelectOption::new("gba", "Game Boy Advance"),
    ];
    let field = |value: &str| value.to_owned();
    rsx! {
        Section { title: "Select", class: "[&>*]:w-56",
            Specimen { label: "Placeholder",
                Select { options: sorts.clone(), value: None, on_change: move |_| {}, label: field("Sort by"), placeholder: field("Choose…"), class: "w-full" }
            }
            Specimen { label: "Chosen",
                Select { options: sorts.clone(), value: sort(), on_change: move |value| sort.set(Some(value)), label: field("Sort by"), class: "w-full" }
            }
            Specimen { label: "Hovered", forced: HOVERED,
                Select { options: sorts.clone(), value: sort(), on_change: move |_| {}, label: field("Sort by"), class: "w-full" }
            }
            Specimen { label: "Pressed", forced: PRESSED,
                Select { options: sorts.clone(), value: sort(), on_change: move |_| {}, label: field("Sort by"), class: "w-full" }
            }
            Specimen { label: "Focus visible", forced: FOCUS_VISIBLE,
                Select { options: sorts.clone(), value: sort(), on_change: move |_| {}, label: field("Sort by"), class: "w-full" }
            }
            Specimen { label: "Invalid",
                Select { options: sorts.clone(), value: None, on_change: move |_| {}, label: field("Sort by"), placeholder: field("Choose…"), invalid: true, class: "w-full" }
            }
            Specimen { label: "Disabled",
                Select { options: sorts.clone(), value: sort(), on_change: move |_| {}, label: field("Sort by"), disabled: true, class: "w-full" }
            }
        }
        Section { title: "Combobox", class: "[&>*]:w-56",
            Specimen { label: "Empty (type to filter)",
                Combobox { options: platforms.clone(), value: platform(), on_change: move |value| platform.set(Some(value)), label: "Platform", placeholder: field("Type to filter"), class: "w-full" }
            }
            Specimen { label: "Chosen",
                Combobox { options: platforms.clone(), value: Some("n64".to_owned()), on_change: move |_| {}, label: "Platform", class: "w-full" }
            }
            Specimen { label: "Focus visible", forced: FOCUS_VISIBLE,
                Combobox { options: platforms, value: Some("n64".to_owned()), on_change: move |_| {}, label: "Platform", class: "w-full" }
            }
        }
    }
}

// ── Feedback ──

pub fn Feedback() -> Element {
    let toasts = use_toasts();
    // The toasts that stay (an error, one with an action), for the
    // screenshots; the buttons show more.
    use_effect(move || {
        toasts.show("Couldn't reach RomM: check its address.", true, None);
        let _ = toasts.show(
            "Hollow Pines was removed.",
            false,
            Some(("Undo".to_owned(), "undo".to_owned())),
        );
    });
    rsx! {
        Section { title: "Progress", class: "[&>*]:w-56",
            Specimen { label: "Empty",
                Progress { value: Some(0.0), label: "Downloading".to_owned(), class: "w-full" }
            }
            Specimen { label: "Part way, with its readout",
                Progress { value: Some(0.4), label: "Installing".to_owned(), value_text: "1.2 GB of 3 GB".to_owned(), class: "w-full" }
            }
            Specimen { label: "Done",
                Progress { value: Some(1.0), label: "Verified".to_owned(), class: "w-full" }
            }
            Specimen { label: "Indeterminate",
                Progress { value: None, label: "Preparing".to_owned(), class: "w-full" }
            }
        }
        Section { title: "Status badge",
            for (label, status) in [("Installed", GameStatus::Installed), ("Not installed", GameStatus::NotInstalled), ("Available", GameStatus::Available), ("Emulated", GameStatus::Emulated)] {
                Specimen { label,
                    StatusBadge { status }
                }
            }
        }
        Section { title: "Error notice", class: "[&>*]:w-[30rem]",
            Specimen { label: "Block, details open",
                ErrorNotice {
                    title: "The game couldn't start".to_owned(),
                    summary: "Proton exited before the game's window opened.".to_owned(),
                    hints: vec!["Pick another Proton build in the game's settings.".to_owned()],
                    detail: "wine: could not load kernel32.dll, status c0000135".to_owned(),
                    reference: "E-4F21".to_owned(),
                    expanded: true,
                    actions: rsx! { Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, "Try again" } },
                    class: "w-full",
                }
            }
            div { class: "flex flex-col gap-4",
                Specimen { label: "Inline",
                    ErrorNotice { variant: NoticeVariant::Inline, summary: "The folder isn't there any more.".to_owned(), class: "w-full" }
                }
                Specimen { label: "Warning",
                    ErrorNotice { warning: true, summary: "Less than 2 GB is free on this drive.".to_owned(), class: "w-full" }
                }
            }
        }
        Section { title: "Toast",
            Specimen { label: "Show one",
                div { class: "flex gap-2",
                    Button {
                        variant: ButtonVariant::Secondary,
                        size: ButtonSize::Sm,
                        on_press: move |_| { toasts.show("Saved to the cloud.", false, None); },
                        "A message"
                    }
                    Button {
                        variant: ButtonVariant::Secondary,
                        size: ButtonSize::Sm,
                        on_press: move |_| { toasts.show("The download failed.", true, None); },
                        "An error"
                    }
                }
            }
        }
    }
}

// ── Games ──

const HOUR: u64 = 3_600_000;

fn game(id: &str, title: &str, subtitle: Option<&str>, status: GameStatus) -> CardGame {
    CardGame {
        id: id.to_owned(),
        title: title.to_owned(),
        subtitle: subtitle.map(str::to_owned),
        status,
        ..CardGame::default()
    }
}

/// A fixed day, so the "last played" text doesn't change.
const PLAYED: i64 = 1_762_000_000_000;

pub fn Cards() -> Element {
    let library = CardGame {
        favorite: true,
        playtime_ms: 42 * HOUR,
        ..game("1", "Hollow Pines", None, GameStatus::Installed)
    };
    let recent = CardGame {
        last_played_ms: Some(PLAYED),
        playtime_ms: 3 * HOUR,
        ..game(
            "2",
            "Star Courier 2",
            Some("Nintendo 64"),
            GameStatus::Emulated,
        )
    };
    let store = CardGame {
        stores: Some(3),
        size_bytes: Some(12_400_000_000),
        ..game("3", "Tidewater", Some("Adventure"), GameStatus::Available)
    };
    let web = game("4", "Paper Moons", Some("itch.io"), GameStatus::Installed);
    let unavailable = CardGame {
        storage_unavailable: true,
        ..game("5", "Moss & Iron", Some("SNES"), GameStatus::Installed)
    };
    rsx! {
        Section { title: "Variants",
            for (label, variant, card) in [("Library, favourite", CardVariant::Library, library.clone()), ("Recent", CardVariant::Recent, recent), ("Store", CardVariant::Store, store), ("Web", CardVariant::Web, web), ("Copy unavailable", CardVariant::Library, unavailable)] {
                Specimen { label,
                    div { class: CARD, GameCard { game: card, variant } }
                }
            }
        }
        Section { title: "States",
            Specimen { label: "Hovered", forced: HOVERED,
                div { class: CARD, GameCard { game: library.clone() } }
            }
            Specimen { label: "Focus visible", forced: FOCUS_VISIBLE,
                div { class: CARD, GameCard { game: library.clone() } }
            }
            Specimen { label: "Selected",
                div { class: CARD, GameCard { game: library.clone(), selected: Some(true) } }
            }
            Specimen { label: "Not selected",
                div { class: CARD, GameCard { game: library.clone(), selected: Some(false) } }
            }
            Specimen { label: "Starting",
                div { class: CARD, GameCard { game: library, starting: true } }
            }
        }
    }
}

/// A row's box: a library list's line.
const ROW: &str = "h-14 w-full";

/// A card's box: a library grid's cell.
const CARD: &str = "h-64 w-44";

pub fn Rows() -> Element {
    let row = CardGame {
        favorite: true,
        playtime_ms: 42 * HOUR,
        last_played_ms: Some(PLAYED),
        ..game("1", "Hollow Pines", Some("Steam"), GameStatus::Installed)
    };
    let emulated = CardGame {
        playtime_ms: 3 * HOUR,
        ..game(
            "2",
            "Star Courier 2",
            Some("Nintendo 64"),
            GameStatus::Emulated,
        )
    };
    let not_installed = game("6", "Lantern Keep", None, GameStatus::NotInstalled);
    rsx! {
        Section { title: "Game row", class: "flex-col [&>*]:w-full",
            Specimen { label: "Rest",
                div { class: ROW, GameRow { game: row.clone() } }
            }
            Specimen { label: "Hovered, not installed", forced: HOVERED,
                div { class: ROW, GameRow { game: not_installed } }
            }
            Specimen { label: "Focus visible", forced: FOCUS_VISIBLE,
                div { class: ROW, GameRow { game: emulated.clone() } }
            }
            Specimen { label: "Selected",
                div { class: ROW, GameRow { game: row.clone(), selected: Some(true) } }
            }
            Specimen { label: "Not selected",
                div { class: ROW, GameRow { game: emulated, selected: Some(false) } }
            }
            Specimen { label: "Starting, with actions",
                div { class: ROW,
                    GameRow {
                        game: row,
                        starting: true,
                        actions: rsx! { Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm, icon: Some(CLOCK), "History" } },
                    }
                }
            }
        }
        Section { title: "Game art",
            for (label, hues) in [("Placeholder", (210, 280)), ("Warm", (20, 340)), ("Green", (120, 180))] {
                Specimen { label, class: "h-24 w-20",
                    GameArt { hues: Some(hues), class: "size-full rounded-card" }
                }
            }
        }
    }
}

// ── Home and accounts ──

pub fn Home() -> Element {
    let hero = HeroGame {
        title: "Hollow Pines".to_owned(),
        genre: "Adventure".to_owned(),
        status: GameStatus::Installed,
        art: (210, 280),
        description: Some(
            "A quiet forest, a lost letter and a lighthouse that shouldn't be lit.".to_owned(),
        ),
        playtime_hours: Some(42.0),
        last_played: Some("Yesterday".to_owned()),
        size_gb: Some(12.4),
    };
    let shelf: Vec<CardGame> = [
        (
            "1",
            "Star Courier 2",
            Some("Nintendo 64"),
            GameStatus::Emulated,
        ),
        ("2", "Tidewater", None, GameStatus::Installed),
        (
            "3",
            "Paper Moons",
            Some("Nintendo DS"),
            GameStatus::Emulated,
        ),
        ("4", "Lantern Keep", None, GameStatus::NotInstalled),
        ("5", "Glasswing", None, GameStatus::Installed),
    ]
    .into_iter()
    .map(|(id, title, subtitle, status)| game(id, title, subtitle, status))
    .collect();
    rsx! {
        Section { title: "Hero", class: "[&>*]:w-full",
            Specimen { label: "Continue playing",
                div { class: "w-full",
                    Hero { game: hero, eyebrow: "Continue playing".to_owned() }
                }
            }
        }
        Section { title: "Shelf", class: "flex-nowrap items-start",
            Specimen { label: "Shelf", class: "min-w-0 flex-1",
                Shelf { title: "Recently added", icon: CLOCK, subtitle: "5 games".to_owned(), class: "w-full",
                    div { class: "flex gap-4",
                        for card in shelf {
                            div { key: "{card.id}", class: "h-56 w-36 shrink-0",
                                GameCard { game: card }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn Accounts() -> Element {
    let mut avatar = use_signal(|| Some(DEFAULT_AVATAR.to_owned()));
    let avatar_choices: Vec<AvatarInfo> = avatars().take(12).collect();
    rsx! {
        Section { title: "Avatar",
            for (label, size) in [("Small", AvatarSize::Sm), ("Medium", AvatarSize::Md), ("Large", AvatarSize::Lg), ("Extra large", AvatarSize::Xl)] {
                Specimen { label,
                    Avatar { avatar: Some(DEFAULT_AVATAR.to_owned()), name: "Ada", size }
                }
            }
            Specimen { label: "Initials",
                Avatar { avatar: None, name: "Ada Lovelace", size: AvatarSize::Lg }
            }
        }
        Section { title: "Avatar picker", class: "[&>*]:w-72",
            Specimen { label: "Rest",
                AvatarPicker { value: avatar(), on_change: move |id| avatar.set(Some(id)), options: avatar_choices.clone(), size: AvatarSize::Md, label: "Avatar".to_owned(), class: "w-full" }
            }
            Specimen { label: "Focus visible", forced: FOCUS_VISIBLE,
                AvatarPicker { value: avatar(), on_change: move |_| {}, options: avatar_choices.clone(), size: AvatarSize::Md, label: "Avatar".to_owned(), class: "w-full" }
            }
            Specimen { label: "Disabled",
                AvatarPicker { value: avatar(), on_change: move |_| {}, options: avatar_choices, disabled: true, size: AvatarSize::Md, label: "Avatar".to_owned(), class: "w-full" }
            }
        }
    }
}

// ── Overlays ──

pub fn Overlays() -> Element {
    let Current(mut page) = use_context::<Current>();
    let mut dialog = use_signal(|| false);
    let mut confirm = use_signal(|| false);
    let mut sort = use_signal(|| Some("played".to_owned()));
    let mut platform = use_signal(|| None::<String>);
    rsx! {
        Section { title: "Open one",
            Specimen { label: "Menu",
                MenuButton { items: menu_items(), on_action: move |_| {}, label: "Game actions", variant: ButtonVariant::Secondary }
            }
            Specimen { label: "Select", class: "w-52",
                Select {
                    options: vec![SelectOption::new("title", "Title"), SelectOption::new("played", "Last played"), SelectOption::new("time", "Playtime")],
                    value: sort(),
                    on_change: move |value| sort.set(Some(value)),
                    label: "Sort by".to_owned(),
                    class: "w-full",
                }
            }
            Specimen { label: "Combobox (type)", class: "w-52",
                Combobox {
                    options: vec![SelectOption::new("snes", "SNES"), SelectOption::new("n64", "Nintendo 64"), SelectOption::new("ds", "Nintendo DS")],
                    value: platform(),
                    on_change: move |value| platform.set(Some(value)),
                    label: "Platform",
                    class: "w-full",
                }
            }
            Specimen { label: "Dialog",
                Button { variant: ButtonVariant::Secondary, on_press: move |_| dialog.set(true), "Open a dialog" }
            }
            Specimen { label: "Confirmation",
                Button { variant: ButtonVariant::Danger, on_press: move |_| confirm.set(true), "Uninstall…" }
            }
        }
        Section { title: "Shown open (the screenshots' screens)",
            Specimen { label: "A menu",
                Button { variant: ButtonVariant::Ghost, on_press: move |_| page.set("menu"), "Show" }
            }
            Specimen { label: "A select's list",
                Button { variant: ButtonVariant::Ghost, on_press: move |_| page.set("select-list"), "Show" }
            }
            Specimen { label: "A confirmation",
                Button { variant: ButtonVariant::Ghost, on_press: move |_| page.set("dialog"), "Show" }
            }
        }
        if dialog() {
            Dialog { title: "About this game", on_close: move |_| dialog.set(false),
                p { class: "text-sm text-fg-muted", "Hollow Pines: 42 hours played, last on 1 November." }
                div { class: "mt-4 flex justify-end",
                    Button { on_press: move |_| dialog.set(false), "Close" }
                }
            }
        }
        if confirm() {
            Uninstall { on_close: move |_| confirm.set(false) }
        }
    }
}

#[component]
fn Uninstall(on_close: EventHandler<()>) -> Element {
    rsx! {
        ConfirmDialog {
            title: "Uninstall Hollow Pines?",
            confirm_label: "Uninstall",
            on_confirm: move |_| on_close.call(()),
            on_close,
            p { class: "text-sm text-fg-muted", "Its saves stay in the cloud. You can install it again at any time." }
        }
    }
}

fn menu_items() -> Vec<MenuEntry> {
    vec![
        MenuEntry::Section {
            title: Some("Play".to_owned()),
            items: vec![
                MenuItem {
                    icon: Some(GAMEPAD),
                    ..MenuItem::new("play", "Play")
                },
                MenuItem {
                    icon: Some(STORE),
                    description: Some("Steam, GOG".to_owned()),
                    ..MenuItem::new("store", "Store pages")
                },
                MenuItem {
                    icon: Some(SEARCH),
                    disabled: true,
                    ..MenuItem::new("find", "Find its files")
                },
            ],
        },
        MenuEntry::Separator,
        MenuEntry::Item(MenuItem {
            icon: Some(HARD_DRIVE),
            danger: true,
            ..MenuItem::new("uninstall", "Uninstall")
        }),
    ]
}

/// Where the open overlays sit: under the Overlays page's first row.
const ANCHOR: Anchor = Anchor {
    x: 288.0,
    y: 150.0,
    width: 208.0,
    height: 36.0,
    viewport_height: 800.0,
};

pub fn OpenMenu() -> Element {
    let Current(mut page) = use_context::<Current>();
    rsx! {
        Overlays {}
        Menu {
            anchor: ANCHOR,
            items: menu_items(),
            on_action: move |_| {},
            on_close: move |_| page.set("overlays"),
            label: "Game actions".to_owned(),
        }
    }
}

pub fn OpenSelect() -> Element {
    let Current(mut page) = use_context::<Current>();
    rsx! {
        Overlays {}
        SelectList {
            anchor: ANCHOR,
            options: vec![
                SelectOption::new("title", "Title"),
                SelectOption::new("played", "Last played"),
                SelectOption { disabled: true, ..SelectOption::new("size", "Size (installed only)") },
                SelectOption::new("time", "Playtime"),
            ],
            value: Some("played".to_owned()),
            label: Some("Sort by".to_owned()),
            on_change: move |_| {},
            on_close: move |_| page.set("overlays"),
            return_to: None,
        }
    }
}

pub fn OpenDialog() -> Element {
    let Current(mut page) = use_context::<Current>();
    rsx! {
        Overlays {}
        Uninstall { on_close: move |_| page.set("overlays") }
    }
}

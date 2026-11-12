//! First-run setup's questions (#97), as the webview's `LanguageStep.tsx`
//! and `Questions.tsx`: the language, where games go, what to look for,
//! and how the user plays.

use blitz_dom::NodeId;
use caskade_core::error::ErrorReport;
use caskade_ui_data::romm::{ConnectionReport, Romm, RommAuth};
use caskade_ui_data::storage::Storage;
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;
use serde_json::Value;

use super::run::{Choices, RommChoice, romm_server};
use super::{Continue, StepHeading};
use crate::data::change;
use crate::flows::paths::{PathRequest, PickKind, format_path, use_path_picker};
use crate::settings::{SettingsFeedback, use_settings_section};
use crate::ui::icons::{
    CHECK, FOLDER_OPEN, GAMEPAD_2, HARD_DRIVE, IMAGE, Icon, MONITOR, MONITOR_PLAY, SERVER, TV,
};
use crate::ui::interact::{Keys, flag, use_interaction_where};
use crate::ui::{Button, Segment, SegmentedControl, Svg, Switch, TextField};

/// The Language setting's value for the OS's language.
const SYSTEM: &str = "system";

/// RomM's name, a product name.
const ROMM: &str = "RomM";

// ---------------------------------------------------------------- language

/// The first question (#213): the language, System (the OS's) to start
/// with. A choice saves `core.general.language`, and the app switches at
/// once, so the rest of setup is in it.
#[component]
pub fn LanguageStep(on_next: EventHandler<()>) -> Element {
    let general = use_settings_section("core.general");
    let nodes = use_signal(Vec::<(String, NodeId)>::new);
    let settings = general.value();
    let value = settings
        .as_ref()
        .and_then(|settings| settings.get("language"))
        .and_then(Value::as_str)
        .map_or_else(|| SYSTEM.to_owned(), str::to_owned);
    let disabled = settings.is_none();
    let options = crate::settings::language_options(&value);
    let values: Vec<String> = options.iter().map(|option| option.value.clone()).collect();
    let stops = stops(&values, &value, disabled);
    let radios: Vec<_> = options
        .into_iter()
        .enumerate()
        .map(|(at, option)| {
            let (previous, next) = neighbours(&values, at);
            (at, option, previous, next, stops[at])
        })
        .collect();
    rsx! {
        section { "data-part": "question", "data-question": "language",
            StepHeading {
                title: t!("Choose your language"),
                description: t!("Caskade follows your system's language unless you pick another. You can change it later in Settings."),
            }
            div {
                role: "radiogroup",
                "aria-label": t!("Language"),
                "aria-disabled": flag(disabled),
                class: "mx-auto flex max-w-md flex-col gap-2",
                for (at, option, previous, next, stop) in radios {
                    CardRadio {
                        key: "{option.value}",
                        control_id: format!("onboarding-language-{}", option.value),
                        checked: option.value == value,
                        stop,
                        disabled,
                        nodes,
                        previous,
                        next,
                        on_check: move |language: String| general.set_field("language", Value::String(language)),
                        part: "language",
                        rise: Some(at + 3),
                        class: "group flex animate-rise cursor-default items-center justify-between gap-3 rounded-panel border border-border-strong bg-surface-raised px-4 py-3 transition-[border-color,box-shadow,background-color] duration-(--cs-motion-duration-base) data-focus-visible:focus-ring data-hovered:bg-surface-hover data-selected:border-accent data-selected:shadow-[0_0_20px_-8px] data-selected:shadow-glow",
                        value: option.value.clone(),
                        span { id: "onboarding-language-{option.value}-label", class: "font-medium", "{option.label}" }
                        Svg { icon: CHECK, class: "size-4 text-accent-alt opacity-0 group-data-selected:opacity-100" }
                    }
                }
            }
            div { class: "mt-3",
                SettingsFeedback { error: general.error(), saving: false }
            }
            Continue { on_press: move |_| on_next.call(()) }
        }
    }
}

// ------------------------------------------------------------------ folder

/// Where new games go: the default storage location, and choosing another.
#[component]
pub fn FolderStep(on_next: EventHandler<()>) -> Element {
    let storage = crate::flows::storage::use_storage().filter(Storage::manages_locations);
    let picker = use_path_picker();
    let lister = storage.clone();
    let mut roots = use_resource(move || {
        let storage = lister.clone();
        async move {
            match storage {
                Some(storage) => change(async move { storage.roots().await }).await,
                None => Err(unavailable()),
            }
        }
    });
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None::<ErrorReport>);
    let read = roots.read().clone();
    let checking = read.is_none();
    let (current, load_error) = match read {
        Some(Ok(list)) => (
            list.iter()
                .find(|root| root.root.is_default)
                .or(list.first())
                .cloned(),
            None,
        ),
        Some(Err(error)) => (None, Some(error)),
        None => (None, None),
    };
    let error = load_error.or(failed());
    let current_path = current
        .as_ref()
        .map(|root| root.root.path.to_string_lossy().into_owned());
    let choose = {
        let storage = storage.clone();
        let start = current_path.clone();
        move |_| {
            let storage = storage.clone();
            let start = start.clone();
            pending.set(true);
            failed.set(None);
            spawn(async move {
                let outcome = async {
                    let Some(storage) = storage else {
                        return Err(unavailable());
                    };
                    let mut request =
                        PathRequest::new(PickKind::Folder, t!("Where should new games go?"));
                    request.start_at = start;
                    let Some(path) = picker.pick(request).await else {
                        return Ok(());
                    };
                    change(async move {
                        let added = storage.add(path, None).await?;
                        storage.set_default(&added.root.id).await.map(|_| ())
                    })
                    .await
                }
                .await;
                pending.set(false);
                if let Err(error) = outcome {
                    failed.set(Some(error));
                }
                roots.restart();
            });
        }
    };
    let space = current.as_ref().and_then(|root| {
        let free = root.capabilities.free_bytes?;
        let total = root.capabilities.total_bytes.filter(|total| *total > 0);
        let text = match total {
            Some(total) => t!(
                "{free} free of {total}",
                free = format::size(Some(free)),
                total = format::size(Some(total))
            ),
            None => t!("{free} free", free = format::size(Some(free))),
        };
        let used = total.map(|total| (100.0 - free as f64 / total as f64 * 100.0).max(2.0));
        Some((text, used))
    });
    let has_current = current.is_some();
    rsx! {
        section { "data-part": "question", "data-question": "folder",
            StepHeading {
                title: t!("Where should new games go?"),
                description: t!("Games you install with Caskade are kept here. You can add more places later."),
            }
            div {
                "data-part": "card",
                class: "flex animate-rise items-center gap-4 rounded-panel border border-border-strong bg-surface-raised p-5 [--i:4]",
                Svg { icon: HARD_DRIVE, class: "size-8 shrink-0 text-accent-alt" }
                div { class: "min-w-0 flex-1",
                    if let Some(path) = current_path.as_deref().map(format_path) {
                        p { class: "truncate font-mono text-sm", title: "{path}", "{path}" }
                        if let Some((text, used)) = space {
                            p { class: "mt-1 text-sm text-fg-muted", "{text}" }
                            if let Some(used) = used {
                                div { class: "mt-2 h-1.5 overflow-hidden rounded-full bg-border-strong",
                                    div {
                                        class: "h-full rounded-full bg-linear-to-r from-accent to-accent-alt",
                                        style: "width: {used}%",
                                    }
                                }
                            }
                        }
                    } else {
                        p { class: "text-sm text-fg-muted",
                            if checking {
                                {t!("Checking…")}
                            } else {
                                {t!("Choose a folder for your games.")}
                            }
                        }
                    }
                }
                Button {
                    id: "onboarding-choose-folder",
                    "data-part": "choose",
                    icon: Some(FOLDER_OPEN),
                    disabled: pending(),
                    on_press: choose,
                    if has_current {
                        {tp!("folder", "Change")}
                    } else {
                        {tp!("folder", "Choose")}
                    }
                }
            }
            if let Some(error) = error {
                p { role: "alert", class: "mt-3 text-center text-sm text-danger", "{error.summary}" }
            }
            Continue { on_press: move |_| on_next.call(()), disabled: !has_current }
        }
    }
}

fn unavailable() -> ErrorReport {
    ErrorReport::from(t!("Storage is unavailable."))
}

// ----------------------------------------------------------------- sources

/// One thing to look for: a switch, and what it needs (`more`) once it's
/// on.
#[component]
fn Source(
    icon: Icon,
    title: String,
    description: String,
    selected: bool,
    on_change: EventHandler<bool>,
    index: usize,
    control_id: &'static str,
    #[props(default)] more: Option<Element>,
) -> Element {
    let shown = more.filter(|_| selected);
    rsx! {
        div {
            "data-part": "source",
            "data-status": if selected { "on" } else { "off" },
            style: "--i: {index + 3}",
            class: "animate-rise rounded-panel border border-border bg-surface-raised p-4 transition-colors duration-(--cs-motion-duration-base) data-[status=on]:border-accent/50",
            div { class: "flex items-center gap-4",
                Svg { icon, class: "size-6 shrink-0 text-accent-alt" }
                div { class: "min-w-0 flex-1",
                    p { class: "font-medium", "{title}" }
                    p { class: "text-sm text-fg-muted", "{description}" }
                }
                Switch {
                    id: control_id,
                    "aria-label": title.clone(),
                    checked: selected,
                    on_change: move |on: bool| on_change.call(on),
                }
            }
            if let Some(more) = shown {
                div { class: "mt-4 ml-10 flex flex-col gap-3", {more} }
            }
        }
    }
}

/// What to look for: games on this PC, a ROM folder, a RomM server, and
/// artwork online.
#[component]
pub fn SourcesStep(
    choices: Choices,
    on_change: EventHandler<Choices>,
    on_next: EventHandler<()>,
) -> Element {
    let picker = use_path_picker();
    let romm = try_use_context::<Romm>().or_else(|| crate::shell::services().and_then(Romm::of));
    let mut retro = use_signal(|| choices.rom_folder.is_some());
    let mut folder = use_signal(|| choices.rom_folder.clone());
    let mut use_romm = use_signal(|| choices.romm.is_some());
    let mut server = use_signal(|| choices.romm.clone().unwrap_or_default());
    let mut checked = use_signal(|| None::<ConnectionReport>);
    let mut checking = use_signal(|| false);
    let mut check_error = use_signal(|| None::<ErrorReport>);

    let set = {
        let choices = choices.clone();
        move |patch: fn(&mut Choices, bool)| {
            let choices = choices.clone();
            move |on: bool| {
                let mut next = choices.clone();
                patch(&mut next, on);
                on_change.call(next);
            }
        }
    };
    let pick = move |_| {
        let start = folder.peek().clone();
        spawn(async move {
            let mut request = PathRequest::new(PickKind::Folder, t!("Your ROM folder"));
            request.start_at = start;
            if let Some(path) = picker.pick(request).await {
                folder.set(Some(path));
            }
        });
    };
    let check = move |_| {
        let choice = server.peek().clone();
        let romm = romm.clone();
        checking.set(true);
        check_error.set(None);
        spawn(async move {
            let outcome = match romm {
                Some(romm) => {
                    let secret = Some(choice.secret.trim().to_owned()).filter(|s| !s.is_empty());
                    let config = romm_server(&choice, &[]);
                    change(async move { romm.test(config, secret).await }).await
                }
                None => Err(ErrorReport::from(t!(
                    "This part of Caskade didn't start this session. Restarting Caskade may fix it."
                ))),
            };
            checking.set(false);
            match outcome {
                Ok(found) => checked.set(Some(found)),
                Err(error) => {
                    checked.set(None);
                    check_error.set(Some(error));
                }
            }
        });
    };
    let next = {
        let choices = choices.clone();
        move |_| {
            on_change.call(Choices {
                rom_folder: if retro() { folder() } else { None },
                romm: if use_romm() { Some(server()) } else { None },
                ..choices.clone()
            });
            on_next.call(());
        }
    };
    let mut edit = move |apply: fn(&mut RommChoice, String), value: String| {
        checked.set(None);
        apply(&mut server.write(), value);
    };

    let current = server();
    let token = current.auth == RommAuth::Token;
    // A token server can't sign in without its token.
    let romm_ready =
        !current.url.trim().is_empty() && (!token || !current.secret.trim().is_empty());
    let ready = (!retro() || folder().is_some()) && (!use_romm() || romm_ready);
    let auths = vec![
        Segment::new("password", t!("Username & password")),
        Segment::new("token", t!("API token")),
    ];
    let shown_folder = folder().map(|path| format_path(&path));
    let connected = checked().map(|report| match report.version {
        Some(version) => t!("Connected · RomM {version}", version = version),
        None => t!("Connected"),
    });

    let play_time = rsx! {
        crate::ui::Checkbox {
            id: "onboarding-play-time",
            checked: choices.play_time,
            label: t!("Keep my play time"),
            on_change: set(|choices, on| choices.play_time = on),
        }
    };
    let rom_folder = rsx! {
        div { class: "flex items-center gap-3",
            Button { id: "onboarding-rom-folder", icon: Some(FOLDER_OPEN), on_press: pick,
                if shown_folder.is_some() {
                    {t!("Change folder")}
                } else {
                    {t!("Choose folder")}
                }
            }
            if let Some(path) = shown_folder.clone() {
                span { class: "min-w-0 truncate font-mono text-sm text-fg-muted", title: "{path}", "{path}" }
            }
        }
    };
    let romm_fields = rsx! {
        TextField {
            id: "onboarding-romm-url",
            label: tp!("server", "Address"),
            placeholder: t!(note = "An example server address", "https://romm.example.com"),
            value: current.url.clone(),
            on_input: move |value: String| edit(|server, value| server.url = value, value),
        }
        SegmentedControl {
            id: "onboarding-romm-auth",
            "aria-label": t!("Sign in with"),
            options: auths,
            value: if token { "token" } else { "password" },
            class: "self-start",
            on_change: move |auth: String| {
                checked.set(None);
                // A password isn't a token: the other one starts empty.
                let mut server = server.write();
                server.auth = if auth == "token" { RommAuth::Token } else { RommAuth::Password };
                server.secret = String::new();
            },
        }
        if token {
            TextField {
                id: "onboarding-romm-secret",
                label: t!("API token"),
                kind: "password",
                placeholder: t!(note = "How a RomM API token starts; keep it as it is", "rmm_…"),
                value: current.secret.clone(),
                on_input: move |value: String| edit(|server, value| server.secret = value, value),
            }
        } else {
            div { class: "grid grid-cols-2 gap-3",
                TextField {
                    id: "onboarding-romm-username",
                    label: t!("Username"),
                    value: current.username.clone(),
                    on_input: move |value: String| edit(|server, value| server.username = value, value),
                }
                TextField {
                    id: "onboarding-romm-secret",
                    label: t!("Password"),
                    kind: "password",
                    value: current.secret.clone(),
                    on_input: move |value: String| edit(|server, value| server.secret = value, value),
                }
            }
        }
        div { class: "flex items-center gap-3",
            Button {
                id: "onboarding-romm-check",
                disabled: !romm_ready || checking(),
                on_press: check,
                if checking() {
                    {t!("Checking…")}
                } else {
                    {tp!(note = "Tests the RomM server", "action", "Check")}
                }
            }
            if let Some(connected) = connected {
                span { role: "status", class: "text-sm text-status-installed", "{connected}" }
            }
            if let Some(error) = check_error() {
                span { role: "alert", class: "text-sm text-danger", "{error.summary}" }
            }
        }
    };

    rsx! {
        section { "data-part": "question", "data-question": "sources",
            StepHeading {
                title: t!("What should Caskade look for?"),
                description: t!("Turn on what you have. Everything can be changed later in Settings."),
            }
            div { class: "flex flex-col gap-3",
                Source {
                    index: 0,
                    control_id: "onboarding-pc-games",
                    icon: MONITOR,
                    title: t!("Games on this PC"),
                    description: t!("From Steam, Epic, GOG, Heroic, Lutris and more."),
                    selected: choices.pc_games,
                    on_change: set(|choices, on| choices.pc_games = on),
                    more: play_time,
                }
                Source {
                    index: 1,
                    control_id: "onboarding-retro",
                    icon: GAMEPAD_2,
                    title: t!("Retro games"),
                    description: t!(
                        note = "snes and psx are folder names; keep them as they are",
                        "A ROMs folder with a folder per console, like snes or psx."
                    ),
                    selected: retro(),
                    on_change: move |on: bool| retro.set(on),
                    more: rom_folder,
                }
                Source {
                    index: 2,
                    control_id: "onboarding-romm",
                    icon: SERVER,
                    title: ROMM.to_owned(),
                    description: t!("Games from your own RomM server."),
                    selected: use_romm(),
                    on_change: move |on: bool| use_romm.set(on),
                    more: romm_fields,
                }
                Source {
                    index: 3,
                    control_id: "onboarding-online",
                    icon: IMAGE,
                    title: t!("Artwork and game info"),
                    description: t!("Covers, descriptions and genres, looked up online."),
                    selected: choices.online,
                    on_change: set(|choices, on| choices.online = on),
                }
            }
            Continue { on_press: next, disabled: !ready }
        }
    }
}

// -------------------------------------------------------------------- mode

/// How the user plays. The native app has no console mode yet (#107): the
/// choice is saved as the mode Caskade starts in (`core.console.start`).
/// `last`: setup starts from here; otherwise the account question follows.
#[component]
pub fn ModeStep(on_next: EventHandler<()>, last: bool) -> Element {
    let console = use_settings_section("core.console");
    let nodes = use_signal(Vec::<(String, NodeId)>::new);
    // A pad is plugged in: a hint towards console mode.
    let pads = use_hook(crate::input::connected_pads);
    let saved = console.value().and_then(|settings| {
        settings
            .get("start")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    // Automatic (or not loaded yet) shows the mode this app is in.
    let mode = match saved.as_deref() {
        Some("console") => "console",
        _ => "desktop",
    };
    let values = vec!["desktop".to_owned(), "console".to_owned()];
    let stops = stops(&values, mode, false);
    let cards = [
        (
            MONITOR_PLAY,
            tp!("mode", "Desktop"),
            t!("In a window, with mouse and keyboard."),
        ),
        (
            TV,
            t!("Console mode"),
            t!("Full screen, made for a controller and a TV."),
        ),
    ];
    let radios: Vec<_> = cards
        .into_iter()
        .enumerate()
        .map(|(at, (icon, title, description))| {
            let (previous, next) = neighbours(&values, at);
            (
                values[at].clone(),
                icon,
                title,
                description,
                previous,
                next,
                stops[at],
            )
        })
        .collect();
    let hint = pads.first().map(|pad| {
        if pad.name.is_empty() {
            t!("A controller is connected.")
        } else {
            t!("{controller} is connected.", controller = pad.name.clone())
        }
    });
    let label = last.then(|| {
        t!(
            note = "Finishes first-run setup's questions and starts the work",
            "Set up"
        )
    });
    rsx! {
        section { "data-part": "question", "data-question": "mode",
            StepHeading {
                title: t!("How do you play?"),
                description: t!(
                    note = "View is a controller button",
                    "Switch any time with the button at the top, or View on a controller."
                ),
            }
            div {
                role: "radiogroup",
                "aria-label": t!("How do you play?"),
                class: "grid grid-cols-2 gap-4",
                for (value, icon, title, description, previous, next, stop) in radios {
                    CardRadio {
                        key: "{value}",
                        control_id: format!("onboarding-mode-{value}"),
                        checked: value == mode,
                        stop,
                        nodes,
                        previous,
                        next,
                        on_check: move |start: String| console.set_field("start", Value::String(start)),
                        part: "mode",
                        class: "group flex animate-rise cursor-default flex-col items-center gap-3 rounded-panel border border-border-strong bg-surface-raised p-6 text-center transition-[border-color,box-shadow,background-color] duration-(--cs-motion-duration-base) data-focus-visible:focus-ring data-hovered:bg-surface-hover data-selected:border-accent data-selected:shadow-[0_0_28px_-8px] data-selected:shadow-glow",
                        value: value.clone(),
                        Svg { icon, class: "size-12 text-fg-muted transition-colors duration-(--cs-motion-duration-base) group-data-selected:text-accent-alt" }
                        span { id: "onboarding-mode-{value}-label", class: "font-display text-lg font-semibold", "{title}" }
                        span { class: "text-sm text-fg-muted", "{description}" }
                    }
                }
            }
            if let Some(hint) = hint {
                p { class: "mt-4 animate-rise text-center text-sm text-fg-muted [--i:6]",
                    Svg { icon: GAMEPAD_2, class: "mr-1.5 inline size-4 text-accent-alt" }
                    "{hint}"
                }
            }
            Continue { on_press: move |_| on_next.call(()), label }
        }
    }
}

// ------------------------------------------------------------ card radios

/// The radios before and after the one at `at`, wrapping: where arrows go.
fn neighbours(values: &[String], at: usize) -> (Option<String>, Option<String>) {
    let count = values.len();
    if count == 0 {
        return (None, None);
    }
    (
        Some(values[(at + count - 1) % count].clone()),
        Some(values[(at + 1) % count].clone()),
    )
}

/// Which radio is the group's tab stop: the checked one, else the first;
/// none while the group is disabled.
fn stops(values: &[String], value: &str, disabled: bool) -> Vec<bool> {
    let any = values.iter().any(|known| known == value);
    values
        .iter()
        .enumerate()
        .map(|(at, known)| !disabled && if any { known == value } else { at == 0 })
        .collect()
}

/// A radio drawn as a card, as react-aria's `Radio` with the webview's
/// classes: a click, Space or a pad's A checks it; arrows check the next
/// one and move there, as in `ui::RadioGroup`. `children` are its content;
/// one of them carries `{control_id}-label`, which names it.
#[component]
fn CardRadio(
    value: String,
    control_id: String,
    checked: bool,
    stop: bool,
    #[props(default)] disabled: bool,
    nodes: Signal<Vec<(String, NodeId)>>,
    previous: Option<String>,
    next: Option<String>,
    on_check: EventHandler<String>,
    part: &'static str,
    /// Its place in the rise-in animation (`--i`).
    #[props(default)]
    rise: Option<usize>,
    class: &'static str,
    children: Element,
) -> Element {
    let interaction = use_interaction_where(stop);
    let input = crate::input::use_input();
    let arrow = use_callback(move |forward: bool| {
        let target = if forward {
            next.clone()
        } else {
            previous.clone()
        };
        if let Some(target) = target {
            on_check.call(target.clone());
            if let Some((_, node)) = nodes.peek().iter().find(|(known, _)| *known == target) {
                input.focus(*node);
            }
        }
    });
    let picked = value.clone();
    let check = use_callback(move |()| {
        if !disabled && !checked {
            on_check.call(picked.clone());
        }
    });
    let mount_value = value.clone();
    rsx! {
        div {
            id: "{control_id}",
            role: "radio",
            tabindex: if stop { "0" } else { "-1" },
            "aria-checked": if checked { "true" } else { "false" },
            "aria-disabled": flag(disabled),
            "aria-labelledby": "{control_id}-label",
            "data-part": part,
            "data-selected": flag(checked),
            "data-disabled": flag(disabled),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            style: rise.map(|at| format!("--i: {at}")),
            class,
            onmounted: move |event| {
                if let Some(node) = event.data().downcast::<NodeHandle>().map(NodeHandle::node_id) {
                    let mut nodes = nodes;
                    nodes.write().retain(|(known, _)| *known != mount_value);
                    nodes.write().push((mount_value.clone(), node));
                }
            },
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onblur: move |_| interaction.blur(),
            onclick: move |_| check(()),
            onkeydown: move |event| {
                let forward = match event.key() {
                    Key::Named(NamedKey::ArrowDown | NamedKey::ArrowRight) => Some(true),
                    Key::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft) => Some(false),
                    _ => None,
                };
                if disabled {
                    return;
                }
                if let Some(forward) = forward {
                    event.prevent_default();
                    event.stop_propagation();
                    arrow(forward);
                } else {
                    interaction.key_down(&event, Keys::Space, || {});
                }
            },
            onkeyup: move |event| interaction.key_up(&event, || check(())),
            {children}
        }
    }
}

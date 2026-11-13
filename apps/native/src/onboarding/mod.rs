//! First-run setup (#97), as the webview's `onboarding/`: a welcome, short
//! questions (the language, where games go, what to look for, how the user
//! plays, and last the admin's account when there is one to name), then
//! the work they start, shown as it happens, with sounds. `OnboardingGate`
//! shows it in place of the app while `core.onboarding.completed` is
//! false, and again when Settings asks for it; finishing opens the library.

mod account;
mod device;
mod questions;
pub mod run;
mod setup;
pub mod sound;
#[cfg(test)]
mod tests;

use caskade_ui_i18n::t;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;
use serde_json::Value;

use crate::router::use_router;
use crate::settings::use_settings_section;
use crate::ui::Svg;
use crate::ui::icons::{VOLUME_2, VOLUME_X};
use run::Choices;
use sound::{Cue, Sounds};

/// Setup's catalogued root.
pub const ANATOMY: &[Entry] = &[ONBOARDING];

pub const ONBOARDING: Entry = Entry {
    parts: &[
        ("titlebar", "The row at the top that moves the window."),
        ("sound", "Turns setup's sounds on and off."),
        ("skip", "Skips setup, keeping the defaults."),
        ("main", "The current step."),
        ("welcome", "The first screen."),
        ("logo", "The logo on the welcome screen, with its glow."),
        ("start", "Get started."),
        ("question", "A question; `data-question` names it."),
        (
            "card",
            "The games folder, its free space and its Change button.",
        ),
        ("choose", "Chooses another games folder."),
        (
            "source",
            "Something to look for; `data-status` is `on` or `off`.",
        ),
        (
            "mode",
            "Desktop or console mode; `data-selected` marks the choice.",
        ),
        (
            "language",
            "A language, or System; `data-selected` marks the choice.",
        ),
        ("next", "Continue, Set up, or Open my library."),
        (
            "progress",
            "The step dots; each has `data-status` `active`, `done` or `next`.",
        ),
        (
            "setup",
            "The work being done; `data-status` is `running` or `done`.",
        ),
        (
            "covers",
            "Covers of the games found so far, behind the list.",
        ),
        ("count", "How many games the library has."),
        ("tasks", "The list of tasks."),
        (
            "task",
            "One task; `data-task` names it, `data-status` is `waiting`, `running`, `done`, `warning` or `failed`.",
        ),
    ],
    states: &[(
        "data-step",
        &[
            "welcome", "language", "folder", "sources", "mode", "account", "setup",
        ],
    )],
    contains: &[
        "backdrop",
        "window-controls",
        "button",
        "switch",
        "checkbox",
        "text-field",
        "game-art",
        "avatar-picker",
    ],
    ..Entry::new(
        "onboarding",
        "Onboarding (apps/native/src/onboarding/mod.rs)",
        Source::App,
        "First-run setup, shown in place of the app shell: a welcome, the questions (`language`, `folder`, `sources`, `mode`, and `account`, the admin's name, avatar and PIN), then the work they start (`setup`). Things arriving use `animate-rise`; the running task's sweep pauses with `--cs-motion-ambient`.",
    )
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Device,
    Language,
    Folder,
    Games,
    Sources,
    Mode,
    Account,
    Setup,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Device => "device",
            Self::Language => "language",
            Self::Folder => "folder",
            Self::Games => "games",
            Self::Sources => "sources",
            Self::Mode => "mode",
            Self::Account => "account",
            Self::Setup => "setup",
        }
    }
}

const BASICS: [Step; 4] = [Step::Language, Step::Folder, Step::Sources, Step::Mode];

/// `children` (the app) once setup is done; setup in its place until then.
#[component]
pub fn OnboardingGate(children: Element) -> Element {
    let prefs = use_settings_section("core.onboarding");
    let router = use_router();
    let mut open = use_signal(|| None::<bool>);
    let completed = prefs
        .value()
        .and_then(|value| value.get("completed").and_then(Value::as_bool));
    let failed = prefs.load_error().is_some();
    // Finishing doesn't close it here: setup marks itself done before its
    // last screen, and closes with `on_done`.
    use_effect(use_reactive!(|completed, failed| {
        if completed == Some(false) {
            open.set(Some(true));
        } else if (completed == Some(true) || failed) && open.peek().is_none() {
            open.set(Some(false));
        }
    }));
    match open() {
        None => rsx! {},
        Some(false) => children,
        Some(true) => rsx! {
            Onboarding {
                on_done: move |_| {
                    open.set(Some(false));
                    router.navigate("/library?filter=all");
                },
            }
        },
    }
}

/// The heading every question shares.
#[component]
pub fn StepHeading(title: String, description: String) -> Element {
    rsx! {
        div { class: "mb-8 text-center",
            h1 { class: "animate-rise font-display text-3xl font-semibold", "{title}" }
            p { class: "mt-2 animate-rise text-fg-muted [--i:2]", "{description}" }
        }
    }
}

const PRIMARY: &str = "inline-flex h-11 items-center justify-center gap-2 rounded-control bg-linear-to-r from-accent to-accent-strong px-8 font-semibold text-accent-fg transition-[filter,box-shadow,scale,opacity] duration-(--cs-motion-duration-fast) data-disabled:opacity-40 data-focus-visible:focus-ring data-hovered:shadow-[0_0_22px_-4px] data-hovered:shadow-glow data-hovered:brightness-115 data-pressed:scale-[0.97]";

/// Continue at the bottom of a question (`label` in its place); a pad
/// starts on it.
#[component]
pub fn Continue(
    on_press: EventHandler<()>,
    #[props(default)] disabled: bool,
    #[props(default)] label: Option<String>,
) -> Element {
    rsx! {
        div { class: "mt-10 flex animate-rise justify-center [--i:8]",
            crate::ui::Button {
                id: "onboarding-next",
                "data-part": "next",
                "data-focus-default": (!disabled).then_some("true"),
                disabled,
                class: PRIMARY,
                on_press: move |_| on_press.call(()),
                {label.unwrap_or_else(|| t!("Continue"))}
            }
        }
    }
}

#[component]
fn Onboarding(on_done: EventHandler<()>) -> Element {
    let prefs = use_settings_section("core.onboarding");
    // Local too, so the toggle works when the setting can't be saved.
    let mut muted = use_signal(|| None::<bool>);
    let saved_on = prefs
        .value()
        .and_then(|value| value.get("sounds").and_then(Value::as_bool))
        .unwrap_or(true);
    let sounds_on = muted().map_or(saved_on, |muted| !muted);
    let mut enabled = use_signal(|| sounds_on);
    if *enabled.peek() != sounds_on {
        enabled.set(sounds_on);
    }
    let sounds = use_hook(|| {
        if cfg!(test) {
            Sounds::silent()
        } else {
            Sounds::new(move || *enabled.peek())
        }
    });
    let mut step = use_signal(|| Step::Welcome);
    // Set up from another device (#242): its library comes over first.
    let mut from_device = use_signal(|| false);
    let mut choices = use_signal(Choices::default);
    let accounts = crate::accounts::use_account_list();
    let list = accounts
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok().cloned());
    // The admin names their account last, where there are accounts.
    let admin = list.as_ref().and_then(|list| {
        list.accounts
            .iter()
            .find(|account| Some(&account.id) == list.active_id.as_ref())
            .filter(|account| account.role == caskade_ui_data::accounts::AccountRole::Admin)
            .cloned()
    });
    let questions: Vec<Step> = if from_device() {
        vec![
            Step::Language,
            Step::Folder,
            Step::Games,
            Step::Sources,
            Step::Mode,
        ]
    } else if admin.is_some() {
        BASICS.iter().copied().chain([Step::Account]).collect()
    } else {
        BASICS.to_vec()
    };
    let mut order = vec![Step::Welcome];
    if from_device() {
        order.push(Step::Device);
    }
    order.extend(questions.iter().copied());
    order.push(Step::Setup);
    let current = step();
    let index = order
        .iter()
        .position(|known| *known == current)
        .unwrap_or(0);
    let go = {
        let sounds = sounds.clone();
        move |next: Step| {
            sounds.play(Cue::Step);
            step.set(next);
        }
    };
    let next = {
        let order = order.clone();
        let mut go = go.clone();
        move || go(order.get(index + 1).copied().unwrap_or(Step::Setup))
    };
    let back = {
        let order = order.clone();
        let questions = questions.clone();
        let mut go = go.clone();
        move || {
            if questions.contains(&current) && index > 0 {
                go(order[index - 1]);
            }
        }
    };
    let skip = {
        let settings = crate::data::use_settings();
        move |_| {
            let settings = settings.clone();
            spawn(async move {
                let _ = run::mark_completed(&settings).await;
                on_done.call(());
            });
        }
    };
    let toggle_sounds = {
        let sounds = sounds.clone();
        move |_| {
            let on = !sounds_on;
            muted.set(Some(!on));
            if !on {
                sounds.silence();
            }
            prefs.set_field("sounds", Value::Bool(on));
        }
    };
    use_input_back(back.clone());
    // The other device's admin has a PIN: asked here before setup goes on,
    // as at any start.
    if from_device()
        && let Some(list) = list.clone()
        && !list.accounts.is_empty()
        && !list
            .accounts
            .iter()
            .any(|account| Some(&account.id) == list.active_id.as_ref())
    {
        return rsx! {
            crate::accounts::AccountPicker { list, on_signed_in: |_| {} }
        };
    }
    let total = questions.len();
    let position = questions.iter().position(|known| *known == current);
    let next = use_callback(move |()| {
        let mut next = next.clone();
        next()
    });
    rsx! {
        div {
            "data-cs": "onboarding",
            "data-step": current.name(),
            class: "relative flex h-screen flex-col overflow-clip",
            crate::backdrop::Backdrop {}
            header {
                "data-part": "titlebar",
                "data-window-drag": "true",
                class: "relative z-10 flex h-topbar shrink-0 items-center justify-end gap-2 pr-4 pl-6",
                if let Some(position) = position {
                    ol {
                        "data-part": "progress",
                        "aria-label": t!(
                            note = "First-run setup's progress dots",
                            "Step {step} of {total}",
                            step = position + 1,
                            total = total
                        ),
                        class: "pointer-events-none absolute left-1/2 flex -translate-x-1/2 gap-2",
                        for (at, question) in questions.iter().enumerate() {
                            li {
                                key: "{question.name()}",
                                "data-status": if at == position { "active" } else if at < position { "done" } else { "next" },
                                class: "h-1 w-8 rounded-full bg-border-strong transition-colors duration-(--cs-motion-duration-base) data-[status=active]:bg-accent-alt data-[status=done]:bg-accent",
                            }
                        }
                    }
                }
                crate::ui::Button {
                    id: "onboarding-sound",
                    "data-part": "sound",
                    variant: crate::ui::ButtonVariant::Ghost,
                    size: crate::ui::ButtonSize::Sm,
                    "aria-label": if sounds_on { t!("Turn sounds off") } else { t!("Turn sounds on") },
                    on_press: toggle_sounds,
                    Svg { icon: if sounds_on { VOLUME_2 } else { VOLUME_X }, class: "size-4" }
                }
                if current != Step::Setup {
                    crate::ui::Button {
                        id: "onboarding-skip",
                        "data-part": "skip",
                        variant: crate::ui::ButtonVariant::Ghost,
                        size: crate::ui::ButtonSize::Sm,
                        on_press: skip,
                        {t!("Skip setup")}
                    }
                }
                crate::frame::window::WindowControls {}
            }
            main {
                "data-part": "main",
                "data-focus-section": "main",
                class: "relative z-10 flex min-h-0 flex-1 flex-col items-center overflow-y-auto px-6 pb-6",
                div {
                    key: "{current.name()}",
                    // The welcome sits in the middle; the other steps keep
                    // their heading in one place.
                    class: if current == Step::Welcome { "my-auto w-full max-w-2xl py-6" } else { "mt-[8vh] w-full max-w-2xl py-6" },
                    match current {
                        Step::Welcome => rsx! {
                            Welcome {
                                sounds: sounds.clone(),
                                on_start: move |_| step.set(Step::Language),
                                can_pair: crate::flows::pairing::use_pairing().is_some(),
                                on_from_device: move |_| {
                                    from_device.set(true);
                                    step.set(Step::Device);
                                },
                            }
                        },
                        Step::Device => rsx! {
                            device::DeviceStep { on_next: move |_| next(()) }
                        },
                        Step::Language => rsx! {
                            questions::LanguageStep { on_next: move |_| next(()) }
                        },
                        Step::Folder => rsx! {
                            questions::FolderStep { on_next: move |_| next(()) }
                        },
                        Step::Games => rsx! {
                            device::GamesStep { on_next: move |_| next(()) }
                        },
                        Step::Sources => rsx! {
                            questions::SourcesStep {
                                choices: choices(),
                                on_change: move |next: Choices| choices.set(next),
                                on_next: move |_| next(()),
                            }
                        },
                        Step::Mode => rsx! {
                            questions::ModeStep { on_next: move |_| next(()), last: admin.is_none() }
                        },
                        Step::Account => match admin.clone() {
                            Some(account) => rsx! {
                                account::AccountStep { account, on_next: move |_| next(()) }
                            },
                            None => rsx! {},
                        },
                        Step::Setup => rsx! {
                            setup::SetupStep { choices: choices(), sounds: sounds.clone(), on_done }
                        },
                    }
                }
            }
        }
    }
}

/// A pad's B, Escape: back a question.
fn use_input_back(back: impl FnMut() + Clone + 'static) {
    let back = std::rc::Rc::new(std::cell::RefCell::new(back));
    crate::input::use_input_action(
        caskade_ui_input::Action::Back,
        crate::input::Bind::default(),
        move || {
            (back.borrow_mut())();
            true
        },
    );
}

#[component]
fn Welcome(
    sounds: Sounds,
    on_start: EventHandler<()>,
    /// Set up from another device, where devices can pair.
    can_pair: bool,
    on_from_device: EventHandler<()>,
) -> Element {
    // The startup sound comes in with the logo.
    use_hook({
        let sounds = sounds.clone();
        move || sounds.play(Cue::Start)
    });
    rsx! {
        section { "data-part": "welcome", class: "flex flex-col items-center text-center",
            div { "data-part": "logo", class: "relative mb-10 animate-rise",
                div {
                    "aria-hidden": "true",
                    class: "absolute inset-0 -z-10 scale-150 rounded-full bg-glow opacity-60 blur-3xl",
                }
                crate::frame::Brand { class: "size-28 drop-shadow-[0_0_24px_var(--cs-color-glow)]" }
            }
            h1 { class: "animate-rise font-display text-5xl font-semibold tracking-[0.4em] [--i:3]",
                // The product's name, as its logo spells it.
                {WORDMARK}
            }
            crate::ui::Button {
                id: "onboarding-start",
                "data-part": "start",
                "data-focus-default": "true",
                class: "mt-12 h-12 animate-rise rounded-control bg-linear-to-r from-accent to-accent-strong px-10 font-display text-base font-semibold tracking-widest text-accent-fg uppercase [--i:8]",
                on_press: move |_| on_start.call(()),
                {t!(note = "First-run setup's welcome button", "Get started")}
            }
            if can_pair {
                crate::ui::Button {
                    id: "onboarding-from-device",
                    "data-part": "from-device",
                    variant: crate::ui::ButtonVariant::Ghost,
                    class: "mt-4 animate-rise [--i:9]",
                    on_press: move |_| on_from_device.call(()),
                    {t!("Set up from another device")}
                }
            }
        }
    }
}

/// The product's name on the welcome.
const WORDMARK: &str = "CASKADE";

//! An emulated release on the game page (#100, the webview's
//! `views/game/EmulatedEntry.tsx`): its platform and whether it can start,
//! what to set up when it can't (with Configure), the emulator and profile
//! it launches with, extra arguments, and its discs and BIOS files.

use super::data::{use_answer, use_page_data};
use crate::router::{Route, RouteLink};
use crate::ui::{Button, DotTone, Select, SelectOption, StatusDot, TextField};
use caskade_core::emulation::{ArgsOverride, BiosState, EmulatorTarget, GameEmulation, Readiness};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

/// Whether an emulated release can start (asking at launch counts).
pub fn can_start(game: Option<&GameEmulation>) -> bool {
    matches!(
        game.map(|game| &game.readiness),
        Some(Readiness::Ready | Readiness::Ask)
    )
}

fn target_key(target: &EmulatorTarget) -> String {
    match target {
        EmulatorTarget::PlatformDefault => "platform_default".to_owned(),
        EmulatorTarget::AskEmulator => "ask_emulator".to_owned(),
        EmulatorTarget::AskProfile { emulator_id } => format!("ask:{emulator_id}"),
        EmulatorTarget::Profile {
            emulator_id,
            profile_id,
        } => format!("profile:{emulator_id}:{profile_id}"),
    }
}

fn target_of(key: &str) -> EmulatorTarget {
    let mut parts = key.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("profile"), Some(emulator), Some(profile)) => EmulatorTarget::Profile {
            emulator_id: emulator.to_owned(),
            profile_id: profile.to_owned(),
        },
        (Some("ask"), Some(emulator), _) => EmulatorTarget::AskProfile {
            emulator_id: emulator.to_owned(),
        },
        (Some("ask_emulator"), ..) => EmulatorTarget::AskEmulator,
        _ => EmulatorTarget::PlatformDefault,
    }
}

fn readiness_tone(readiness: &Readiness) -> DotTone {
    match readiness {
        Readiness::Ready | Readiness::Ask => DotTone::Ok,
        Readiness::MissingBios { .. } | Readiness::MissingFiles { .. } => DotTone::Warning,
        _ => DotTone::Danger,
    }
}

fn readiness_name(readiness: &Readiness) -> &'static str {
    match readiness {
        Readiness::Ready => "ready",
        Readiness::Ask => "ask",
        Readiness::NoEmulator => "no_emulator",
        Readiness::ProfileGone => "profile_gone",
        Readiness::NotInstalled => "not_installed",
        Readiness::MissingFiles { .. } => "missing_files",
        Readiness::Invalid { .. } => "invalid",
        Readiness::MissingBios { .. } => "missing_bios",
        Readiness::RomMissing { .. } => "rom_missing",
        Readiness::NotEmulated => "not_emulated",
    }
}

fn bios_tone(state: Option<BiosState>, required: bool) -> DotTone {
    match state {
        Some(BiosState::Found) => DotTone::Ok,
        Some(BiosState::WrongHash) => DotTone::Warning,
        Some(BiosState::Missing) if required => DotTone::Danger,
        _ => DotTone::Off,
    }
}

fn bios_label(state: Option<BiosState>) -> String {
    match state {
        Some(BiosState::Found) => t!(note = "A BIOS file's state", "Found"),
        Some(BiosState::WrongHash) => t!(
            note = "A BIOS file's state: the file's hash doesn't match",
            "Wrong dump"
        ),
        Some(BiosState::Missing) => t!(note = "A file's state", "Missing"),
        None => t!(note = "A BIOS file's state", "Unknown"),
    }
}

#[component]
pub fn EmulatedEntry(game_id: String, entry_id: String) -> Element {
    let data = use_page_data();
    let query = {
        let (data, entry_id) = (data.clone(), entry_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.emulation)(entry_id.clone())
        })
    };
    let mut args_text = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let read = query.read();
    let game = match &*read {
        None => {
            return rsx! { p { class: "text-sm text-fg-muted", {t!("Checking the emulator…")} } };
        }
        Some(Err(report)) => {
            let summary = report.summary.clone();
            return rsx! { p { role: "alert", class: "text-sm text-danger", "{summary}" } };
        }
        Some(Ok(game)) => game.clone(),
    };
    drop(read);
    if game.readiness == Readiness::NotEmulated {
        return rsx! {};
    }
    let appended = match &game.args {
        ArgsOverride::Append { args } => args.clone(),
        _ => Vec::new(),
    };
    let profile_label = |emulator: &str, profile: &str, available: bool| {
        if available {
            format!("{emulator} · {profile}")
        } else {
            t!(
                note = "An emulator and its profile, whose files are missing",
                "{emulator} · {profile} (missing files)",
                emulator = emulator.to_owned(),
                profile = profile.to_owned()
            )
        }
    };
    let resolved = (game.target == EmulatorTarget::PlatformDefault)
        .then_some(game.profile.as_ref())
        .flatten()
        .map(|profile| profile_label(&profile.emulator_name, &profile.profile_name, true));
    let mut options = vec![SelectOption::new(
        "platform_default",
        match resolved {
            Some(resolved) => t!(
                note =
                    "The platform's default emulator, then the emulator and profile it resolves to",
                "Platform default ({resolvedProfile})",
                resolvedProfile = resolved
            ),
            None => t!("Platform default"),
        },
    )];
    options.extend(game.compatible.iter().map(|profile| {
        SelectOption::new(
            format!("profile:{}:{}", profile.emulator_id, profile.profile_id),
            profile_label(
                &profile.emulator_name,
                &profile.profile_name,
                profile.available,
            ),
        )
    }));
    options.push(SelectOption::new("ask_emulator", t!("Ask when launching")));
    let current = target_key(&game.target);
    if !options.iter().any(|option| option.value == current) {
        options.push(SelectOption::new(current.clone(), t!("A removed emulator")));
    }
    let set = {
        let (data, entry_id) = (data.clone(), entry_id.clone());
        move |target: EmulatorTarget, args: ArgsOverride| {
            let (data, entry_id) = (data.clone(), entry_id.clone());
            busy.set(true);
            error.set(None);
            spawn(async move {
                match (data.set_emulation)(entry_id, target, args).await {
                    Ok(_) => args_text.set(None),
                    Err(report) => error.set(Some(report.summary)),
                }
                busy.set(false);
            });
        }
    };
    let save_args = {
        let data = data.clone();
        let target = game.target.clone();
        let appended = appended.clone();
        let set = set.clone();
        move |_| {
            let text = args_text
                .peek()
                .clone()
                .unwrap_or_else(|| appended.join(" "));
            let (data, target) = (data.clone(), target.clone());
            let mut set = set.clone();
            spawn(async move {
                let args = if text.trim().is_empty() {
                    Ok(Vec::new())
                } else {
                    (data.split_args)(text).await
                };
                match args {
                    Ok(args) if args.is_empty() => set(target, ArgsOverride::Default),
                    Ok(args) => set(target, ArgsOverride::Append { args }),
                    Err(report) => error.set(Some(report.summary)),
                }
            });
        }
    };
    let platform = game
        .platform
        .as_ref()
        .map(|platform| platform.short.clone())
        .unwrap_or_else(|| game.platform_id.clone());
    let discs_label = if game.discs.len() > 1 {
        t!(note = "The heading over a game's disc images", "Discs")
    } else {
        t!(note = "The heading over a game's single ROM file", "ROM")
    };
    let bios_heading = tp!("table column", "BIOS");
    let state_label = match game.readiness {
        Readiness::Ready => t!(note = "An emulated game's state: it can start", "Ready"),
        Readiness::Ask => t!("Asks for an emulator when launching"),
        _ => t!("Needs setup"),
    };
    let variable = "{RomPath}";
    let current_args = args_text().unwrap_or_else(|| appended.join(" "));
    rsx! {
        div {
            "data-cs": "emulation",
            "data-status": readiness_name(&game.readiness),
            class: "grid grid-cols-[repeat(auto-fit,minmax(min(100%,18rem),1fr))] gap-x-6 gap-y-3 text-sm",
            div { class: "col-span-full flex flex-wrap items-center gap-3",
                span { class: "font-mono text-xs text-fg-muted", "{platform}" }
                StatusDot { tone: readiness_tone(&game.readiness), "{state_label}" }
            }
            if let Some(message) = game.message.clone() {
                p { role: "status", "data-part": "readiness", class: "col-span-full flex flex-wrap items-center gap-2 text-warning",
                    "{message}"
                    RouteLink {
                        to: Route::Settings { tab: "emulators", section: Some("platforms".to_owned()) }.href(),
                        current: false,
                        class: "font-semibold text-accent-alt",
                        {t!("Configure")}
                    }
                }
            }
            div { class: "grid content-start gap-3",
                Select {
                    label: t!("Launch with"),
                    "data-part": "launch-with",
                    options,
                    value: Some(current),
                    disabled: busy(),
                    on_change: {
                        let mut set = set.clone();
                        let args = game.args.clone();
                        move |key: String| set(target_of(&key), args.clone())
                    },
                }
                div { class: "flex items-end gap-2",
                    TextField {
                        class: "flex-1",
                        "data-part": "args",
                        label: t!("Extra arguments"),
                        description: t!(
                            note = "{variable} is a launch variable, e.g. {RomPath}",
                            "Added after the profile's arguments; variables like {variable} work.",
                            variable = variable
                        ),
                        mono: true,
                        value: current_args,
                        on_input: move |text: String| args_text.set(Some(text)),
                    }
                    Button {
                        "data-part": "save-args",
                        disabled: args_text().is_none() || busy(),
                        on_press: save_args,
                        {t!("Save")}
                    }
                }
                if let Some(error) = error() {
                    p { role: "alert", class: "text-danger", "{error}" }
                }
            }
            if !game.discs.is_empty() || !game.bios.is_empty() {
                div { class: "grid content-start gap-3",
                    if !game.discs.is_empty() {
                        div {
                            h4 { class: "font-semibold", "{discs_label}" }
                            ul { class: "mt-1 grid gap-1", "aria-label": "{discs_label}",
                                for disc in game.discs.clone() {
                                    li { key: "{disc.path}", "data-part": "disc", class: "flex items-center gap-2",
                                        StatusDot { tone: if disc.exists { DotTone::Ok } else { DotTone::Danger },
                                            {disc.name.clone().unwrap_or_else(|| if disc.exists {
                                                t!(note = "A file's state", "Present")
                                            } else {
                                                t!(note = "A file's state", "Missing")
                                            })}
                                        }
                                        span { class: "break-all font-mono text-xs text-fg-subtle", "{disc.path}" }
                                    }
                                }
                            }
                        }
                    }
                    if !game.bios.is_empty() {
                        div {
                            h4 { class: "font-semibold", "{bios_heading}" }
                            ul { class: "mt-1 grid gap-1", "aria-label": "{bios_heading}",
                                for bios in game.bios.clone() {
                                    li { key: "{bios.file}", "data-part": "bios", class: "flex items-center gap-2",
                                        StatusDot { tone: bios_tone(bios.state, bios.required), {bios_label(bios.state)} }
                                        span { class: "font-mono text-xs", "{bios.file}" }
                                        if !bios.required {
                                            span { class: "text-xs text-fg-subtle",
                                                {t!(note = "A BIOS file the platform can do without", "optional")}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

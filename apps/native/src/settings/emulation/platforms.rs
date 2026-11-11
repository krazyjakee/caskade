//! Settings → Emulation → Platforms, as the webview's
//! `emulators/ConfiguredPlatforms.tsx`: each platform with ROMs, a default
//! or a compatible profile, its default emulator, BIOS files and folder,
//! and the file extensions it takes.

use std::collections::HashMap;
use std::path::PathBuf;

use caskade_core::emulation::{Availability, BiosState, PlatformSettings, ProfileRef};
use caskade_core::error::ErrorReport;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

use super::fields::{BiosSummary, PathField, StackedCell, StackedRow, StackedTable};
use super::setup::FetchBios;
use crate::data::change;
use crate::ui::icons::{LAYERS, SCAN_SEARCH};
use crate::ui::{Button, DotTone, Select, SelectOption, SettingsSection, StatusDot};

/// A configured profile that runs a platform.
#[derive(Clone, PartialEq)]
struct Choice {
    emulator_id: String,
    profile_id: String,
    emulator_name: String,
    profile_name: String,
    available: bool,
    extensions: Vec<String>,
}

impl Choice {
    /// Its option's value.
    fn key(&self) -> String {
        key(&self.emulator_id, &self.profile_id)
    }
}

fn key(emulator_id: &str, profile_id: &str) -> String {
    format!("{emulator_id}/{profile_id}")
}

/// Every configured profile that runs `platform`, in the emulators' order.
fn compatible(snapshot: &super::Snapshot, platform: &str) -> Vec<Choice> {
    snapshot
        .emulators
        .iter()
        .flat_map(|view| {
            view.emulator.profiles.iter().filter_map(move |config| {
                let state = view.profiles.iter().find(|state| state.id == config.id())?;
                state
                    .platforms
                    .iter()
                    .any(|id| id == platform)
                    .then(|| Choice {
                        emulator_id: view.emulator.id.clone(),
                        profile_id: state.id.clone(),
                        emulator_name: view.emulator.name.clone(),
                        profile_name: state.name.clone(),
                        available: state.availability == Availability::Available,
                        extensions: state.extensions.clone(),
                    })
            })
        })
        .collect()
}

fn path_text(path: Option<&PathBuf>) -> String {
    path.map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[component]
pub fn ConfiguredPlatforms(snapshot: super::Snapshot, on_detect: EventHandler<()>) -> Element {
    let emulation = super::use_emulation();
    let pending = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let mut folders = use_signal(HashMap::<String, String>::new);
    // The platform's change comes back as an emulation change, which reads
    // the snapshot again.
    let save = use_callback(move |settings: PlatformSettings| {
        let emulation = emulation.clone();
        let (mut pending, mut failure) = (pending, failure);
        pending.set(true);
        failure.set(None);
        spawn(async move {
            let result = change(async move { emulation.set_platform(settings).await }).await;
            pending.set(false);
            if let Err(report) = result {
                failure.set(Some(report));
            }
        });
    });
    let platforms: Vec<_> = snapshot
        .platforms
        .iter()
        .map(|view| (view.clone(), compatible(&snapshot, &view.platform.id)))
        .filter(|(view, choices)| {
            view.rom_count > 0 || view.settings.default.is_some() || !choices.is_empty()
        })
        .collect();
    let roms = tp!("table column", "ROMs");
    let emulator_column = tp!("table column", "Default emulator");
    let bios_column = tp!("table column", "BIOS");
    let extensions_column = tp!("table column", "Extensions");
    rsx! {
        SettingsSection {
            icon: LAYERS,
            title: tp!("emulation section", "Platforms"),
            description: t!("Defaults apply to compatible games that use Platform default. BIOS folders are saved per platform."),
            if let Some(report) = failure() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
            }
            if platforms.is_empty() {
                div { class: "mt-4 space-y-3 rounded-control border border-dashed border-border p-6 text-sm text-fg-muted",
                    p { {t!("No configured profiles or ROMs yet.")} }
                    Button {
                        id: "platforms-detect",
                        icon: SCAN_SEARCH,
                        on_press: move |_| on_detect.call(()),
                        {t!("Detect emulators")}
                    }
                }
            } else {
                StackedTable { class: "mt-4",
                    for (view, choices) in platforms {
                        {
                            let id = view.platform.id.clone();
                            let short = view.platform.short.clone();
                            let default = view.settings.default.clone();
                            let bios_dir = view.settings.bios_dir.clone();
                            let saved = path_text(bios_dir.as_ref());
                            let selected = default.as_ref().and_then(|default| {
                                choices.iter().find(|choice| {
                                    choice.emulator_id == default.emulator_id
                                        && choice.profile_id == default.profile_id
                                })
                            }).cloned();
                            let folder = folders.read().get(&id).cloned().unwrap_or_else(|| saved.clone());
                            let mut options = vec![SelectOption::new("none", tp!("default emulator", "None"))];
                            options.extend(choices.iter().map(|choice| {
                                SelectOption::new(choice.key(), format!("{} · {}", choice.emulator_name, choice.profile_name))
                            }));
                            let value = default
                                .as_ref()
                                .map(|default| key(&default.emulator_id, &default.profile_id))
                                .unwrap_or_else(|| "none".to_owned());
                            let needs_bios = default.is_some()
                                && view.platform.bios.iter().any(|file| {
                                    file.found == Some(false) || file.state == Some(BiosState::WrongHash)
                                });
                            let extensions = selected
                                .as_ref()
                                .map(|choice| &choice.extensions)
                                .unwrap_or(&view.platform.extensions)
                                .join(", ");
                            let extensions = if extensions.is_empty() {
                                tp!("file extensions", "None")
                            } else {
                                extensions
                            };
                            let unavailable = selected.as_ref().is_some_and(|choice| !choice.available);
                            let on_choose = {
                                let id = id.clone();
                                let bios_dir = bios_dir.clone();
                                move |value: String| {
                                    let choice = choices.iter().find(|choice| choice.key() == value);
                                    save.call(PlatformSettings {
                                        platform: id.clone(),
                                        default: choice.map(|choice| ProfileRef {
                                            emulator_id: choice.emulator_id.clone(),
                                            profile_id: choice.profile_id.clone(),
                                        }),
                                        bios_dir: bios_dir.clone(),
                                    });
                                }
                            };
                            let on_folder = {
                                let id = id.clone();
                                move |value: String| {
                                    folders.write().insert(id.clone(), value);
                                }
                            };
                            // Saved as it's left, like the default beside it (#55).
                            let on_commit = {
                                let id = id.clone();
                                let default = default.clone();
                                move |value: String| {
                                    if value == saved {
                                        return;
                                    }
                                    save.call(PlatformSettings {
                                        platform: id.clone(),
                                        default: default.clone(),
                                        bios_dir: (!value.is_empty()).then(|| PathBuf::from(value)),
                                    });
                                }
                            };
                            rsx! {
                                StackedRow { key: "{id}", "data-part": "platform-row", class: "align-top",
                                    StackedCell {
                                        p { class: "font-medium", "{view.platform.name}" }
                                        p { class: "mt-1 font-mono text-xs text-fg-muted", "{short}" }
                                    }
                                    StackedCell { label: roms.clone(),
                                        span { class: "font-mono", "{view.rom_count}" }
                                    }
                                    StackedCell { label: emulator_column.clone(), full: true, class: "space-y-2",
                                        Select {
                                            id: "platform-default-{id}",
                                            "aria-label": t!("Default emulator for {platform}", platform = view.platform.name.clone()),
                                            options,
                                            value: Some(value),
                                            disabled: pending(),
                                            on_change: on_choose,
                                        }
                                        if unavailable {
                                            StatusDot { tone: DotTone::Warning, {t!("Default profile is unavailable")} }
                                        }
                                        if selected.is_none() {
                                            p { class: "text-xs text-fg-muted", {t!("Games ask which emulator to use.")} }
                                        }
                                    }
                                    StackedCell { label: bios_column.clone(), full: true, class: "space-y-3",
                                        BiosSummary { files: view.platform.bios.clone() }
                                        if needs_bios {
                                            FetchBios { platform_id: id.clone() }
                                        }
                                        PathField {
                                            id: "platform-bios-{id}",
                                            label: t!("BIOS folder for {platform}", platform = short.clone()),
                                            value: folder,
                                            on_change: on_folder,
                                            on_commit,
                                        }
                                    }
                                    StackedCell { label: extensions_column.clone(), full: true,
                                        span { class: "font-mono text-xs text-fg-muted", "{extensions}" }
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

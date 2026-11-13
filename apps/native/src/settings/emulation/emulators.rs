//! Settings → Emulation → Emulators (#115, #116), as the webview's
//! `ConfiguredEmulators` and `ProfileEditor`: the configured emulators, one
//! chosen at a time with its install, its profiles (whether each can play,
//! the files it needs, its command line) and Remove; adding one by hand;
//! adding a built-in or custom profile and editing one, with a live preview
//! of the command it runs and its own scripts. Every change is checked by
//! the core when it's saved; nothing here starts an emulator. A right click
//! on an emulator or a profile offers its buttons.

use crate::data::sleep;
use std::time::Duration;

use caskade_core::emulation::{
    ArgsOverride, Availability, Emulator, EmulatorInstall, EmulatorProfile, EmulatorStatus,
    EmulatorView, ProfileDraft, ProfileHooks, Tracking,
};
use caskade_core::error::ErrorReport;
use caskade_core::scripts::ScriptScope;
use caskade_ui_data::emulation::Emulation;
use caskade_ui_i18n::{t, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use super::Snapshot;
use super::fields::{BiosSummary, PathField, PreviewTokens, install_location};
use super::setup::ManagedEmulatorInfo;
use super::use_emulation;
use crate::data::change;
use crate::flows::scripts::ScriptsPanel;
use crate::settings::unsaved::{UnsavedForm, UnsavedHint, use_unsaved_changes};
use crate::ui::icons::{CPU, LAYERS, PENCIL, PLUS, SETTINGS_2, TRASH, X};
use crate::ui::{
    Anchor, Button, ButtonSize, ButtonVariant, Checkbox, ConfirmDialog, DotTone, Menu, MenuButton,
    MenuEntry, MenuItem, Segment, SegmentedControl, Select, SelectOption, SettingsSection,
    StatusDot, TextField,
};

mod emulator_editor;
mod profile_editor;
mod profile_fields;
mod profiles;
use emulator_editor::EmulatorForm;
use profile_editor::{ProfileEdit, ProfileEditor, tracking_id, tracking_modes};
use profile_fields::{CustomFields, parse_extensions};
use profiles::{Profiles, profile_key};

/// The key of a form that adds rather than edits.
const NEW_KEY: &str = "new";

#[component]
fn Status(emulator: EmulatorView) -> Element {
    let (tone, text) = match emulator.status {
        EmulatorStatus::NotFound => (
            DotTone::Danger,
            t!(note = "An emulator's status", "Not found"),
        ),
        EmulatorStatus::SomeMissing => (
            DotTone::Warning,
            t!(note = "An emulator's status", "Some profiles missing"),
        ),
        EmulatorStatus::Ready => (DotTone::Ok, t!(note = "An emulator's status", "Ready")),
    };
    rsx! {
        StatusDot { tone, "{text}" }
    }
}

/// What a removal would remove; it asks first (#49).
#[derive(Clone, PartialEq)]
enum Removing {
    Emulator {
        emulator_id: String,
        name: String,
    },
    Profile {
        emulator_id: String,
        profile_id: String,
        name: String,
    },
}

impl Removing {
    fn name(&self) -> &str {
        match self {
            Self::Emulator { name, .. } | Self::Profile { name, .. } => name,
        }
    }
}

/// A right click's menu: where, and what it offers.
#[derive(Clone, PartialEq)]
struct RowMenu {
    anchor: Anchor,
    items: Vec<MenuEntry>,
}

/// The configured emulators and the chosen one's settings and profiles.
#[component]
pub fn ConfiguredEmulators(
    snapshot: Snapshot,
    selected: Option<String>,
    adding: bool,
    on_select: EventHandler<String>,
    on_add: EventHandler<()>,
    on_cancel_add: EventHandler<()>,
) -> Element {
    let emulation = use_emulation();
    let mut edit = use_signal(|| None::<(String, ProfileEdit)>);
    let mut removing = use_signal(|| None::<Removing>);
    let mut writing = use_signal(|| false);
    let mut write_failure = use_signal(|| None::<ErrorReport>);
    let mut menu = use_signal(|| None::<RowMenu>);
    let chosen = snapshot
        .emulators
        .iter()
        .find(|row| Some(&row.emulator.id) == selected.as_ref())
        .or(snapshot.emulators.first())
        .cloned();
    let ask_remove = move |next: Removing| {
        let (mut failure, mut asking) = (write_failure, removing);
        failure.set(None);
        asking.set(Some(next));
    };
    let remove = {
        let emulation = emulation.clone();
        move |_| {
            let Some(target) = removing() else { return };
            let emulation = emulation.clone();
            writing.set(true);
            write_failure.set(None);
            spawn(async move {
                let was_emulator = matches!(target, Removing::Emulator { .. });
                let done = change(async move {
                    match target {
                        Removing::Emulator { emulator_id, .. } => {
                            emulation.delete_emulator(&emulator_id).await
                        }
                        Removing::Profile {
                            emulator_id,
                            profile_id,
                            ..
                        } => {
                            let mut stored = emulation
                                .emulators()
                                .await?
                                .into_iter()
                                .find(|row| row.emulator.id == emulator_id)
                                .map(|row| row.emulator)
                                .ok_or_else(gone)?;
                            stored.profiles.retain(|profile| profile.id() != profile_id);
                            emulation.save_emulator(stored).await.map(|_| ())
                        }
                    }
                })
                .await;
                writing.set(false);
                match done {
                    Ok(()) => {
                        if was_emulator {
                            edit.set(None);
                        }
                        removing.set(None);
                    }
                    Err(report) => write_failure.set(Some(report)),
                }
            });
        }
    };
    // The row a right click opens a menu for, to place it.
    let mut list_node = use_signal(|| None::<NodeHandle>);
    let on_menu_action = move |action: String| {
        menu.set(None);
        let mut parts = action.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("open"), Some(id), _) => {
                edit.set(None);
                on_select.call(id.to_owned());
            }
            (Some("remove-emulator"), Some(id), Some(name)) => ask_remove(Removing::Emulator {
                emulator_id: id.to_owned(),
                name: name.to_owned(),
            }),
            _ => {}
        }
    };
    rsx! {
        div { class: "grid items-start gap-4",
            SettingsSection {
                icon: CPU,
                title: tp!("emulation section", "Emulators"),
                description: t!(
                    note = "How many emulators are configured",
                    "{count} configured",
                    count = snapshot.emulators.len()
                ),
                ul {
                    role: "listbox",
                    "aria-label": tp!("emulation section", "Emulators"),
                    class: "mt-3 space-y-2",
                    onmounted: move |event| list_node.set(event.data().downcast::<NodeHandle>().cloned()),
                    for view in snapshot.emulators.iter().cloned() {
                        li {
                            key: "{view.emulator.id}",
                            role: "option",
                            tabindex: "0",
                            "data-spatial": "focusable",
                            "aria-selected": if !adding && chosen.as_ref().is_some_and(|c| c.emulator.id == view.emulator.id) { "true" } else { "false" },
                            "data-selected": (!adding && chosen.as_ref().is_some_and(|c| c.emulator.id == view.emulator.id)).then_some("true"),
                            class: "cursor-default rounded-control border border-border p-3 outline-none data-selected:border-accent-alt data-selected:bg-accent/10 focus-visible:focus-ring",
                            onclick: {
                                let id = view.emulator.id.clone();
                                move |_| {
                                    edit.set(None);
                                    on_select.call(id.clone());
                                }
                            },
                            onkeydown: {
                                let id = view.emulator.id.clone();
                                move |event: KeyboardEvent| {
                                    use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
                                    if matches!(event.key(), Key::Named(NamedKey::Enter)) || event.key() == Key::Character(" ".into()) {
                                        event.prevent_default();
                                        edit.set(None);
                                        on_select.call(id.clone());
                                    }
                                }
                            },
                            oncontextmenu: {
                                let (id, name) = (view.emulator.id.clone(), view.emulator.name.clone());
                                move |event: MouseEvent| {
                                    event.prevent_default();
                                    event.stop_propagation();
                                    let anchor = crate::frame::pointer_anchor(list_node.peek().as_ref(), &event);
                                    let open = MenuItem { icon: Some(SETTINGS_2), ..MenuItem::new(format!("open:{id}"), t!("Show settings")) };
                                    let remove = MenuItem {
                                        icon: Some(TRASH),
                                        danger: true,
                                        disabled: writing(),
                                        ..MenuItem::new(format!("remove-emulator:{id}:{name}"), t!("Remove emulator…"))
                                    };
                                    menu.set(Some(RowMenu { anchor, items: vec![MenuEntry::Item(open), MenuEntry::Item(remove)] }));
                                }
                            },
                            p { class: "font-medium", "{view.emulator.name}" }
                            p { class: "mt-1 break-all font-mono text-xs text-fg-muted", {install_location(&view.emulator.install)} }
                            div { class: "mt-2",
                                Status { emulator: view.clone() }
                            }
                        }
                    }
                }
                Button { id: "emulators-add", class: "mt-3 w-full", icon: PLUS, on_press: move |_| on_add.call(()), {t!("Add emulator")} }
            }
            if adding {
                EmulatorForm {
                    key: "{NEW_KEY}",
                    emulator: None,
                    snapshot: snapshot.clone(),
                    on_close: move |_| on_cancel_add.call(()),
                    on_created: move |id: String| on_select.call(id),
                }
            } else if let Some(view) = chosen.clone() {
                div { class: "min-w-0 space-y-4",
                    ManagedEmulatorInfo { emulator_id: view.emulator.id.clone() }
                    EmulatorForm {
                        key: "{view.emulator.id}",
                        emulator: Some(view.clone()),
                        snapshot: snapshot.clone(),
                        on_close: |_| {},
                        on_created: |_| {},
                    }
                    Profiles {
                        view: view.clone(),
                        snapshot: snapshot.clone(),
                        writing: writing(),
                        on_edit: move |value: ProfileEdit| edit.set(Some((String::new(), value))),
                        on_remove: move |next: Removing| ask_remove(next),
                    }
                    if let Some(report) = write_failure().filter(|_| removing().is_none()) {
                        p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
                    }
                    if let Some((_, value)) = edit() {
                        ProfileEditor {
                            key: "{profile_key(&value.profile)}",
                            emulator: view.clone(),
                            initial: value,
                            snapshot: snapshot.clone(),
                            on_close: move |_| edit.set(None),
                        }
                    }
                    Button {
                        id: "emulator-remove",
                        variant: ButtonVariant::Danger,
                        icon: TRASH,
                        disabled: writing(),
                        on_press: {
                            let (id, name) = (view.emulator.id.clone(), view.emulator.name.clone());
                            move |_| ask_remove(Removing::Emulator { emulator_id: id.clone(), name: name.clone() })
                        },
                        {t!("Remove emulator…")}
                    }
                }
            } else {
                div { class: "rounded-panel border border-dashed border-border p-8 text-sm text-fg-muted",
                    {t!("No emulators yet. Detect installed emulators or add one by hand.")}
                }
            }
            if let Some(open) = menu() {
                Menu {
                    anchor: open.anchor,
                    items: open.items,
                    on_action: on_menu_action,
                    on_close: move |_| menu.set(None),
                }
            }
            if let Some(target) = removing() {
                ConfirmDialog {
                    title: t!("Remove {name}?", name = target.name().to_owned()),
                    confirm_label: t!(note = "Confirms removing an emulator or profile", "Remove"),
                    pending_label: t!("Removing…"),
                    pending: writing(),
                    on_confirm: remove,
                    on_close: move |_| removing.set(None),
                    p {
                        if matches!(target, Removing::Emulator { .. }) {
                            {t!("Its profiles go with it, and platforms that played with it need another emulator. Its own files stay where they are.")}
                        } else {
                            {t!("Games that play with this profile need another one.")}
                        }
                    }
                    if let Some(report) = write_failure() {
                        p { role: "alert", class: "text-danger", "{report.summary}" }
                    }
                }
            }
        }
    }
}

fn gone() -> ErrorReport {
    ErrorReport::new(
        caskade_core::error::ErrorCode::Conflict,
        t!("This emulator no longer exists. Refresh the list."),
    )
}

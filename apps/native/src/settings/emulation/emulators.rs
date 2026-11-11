//! Settings → Emulation → Emulators (#115, #116), as the webview's
//! `ConfiguredEmulators` and `ProfileEditor`: the configured emulators, one
//! chosen at a time with its install, its profiles (whether each can play,
//! the files it needs, its command line) and Remove; adding one by hand;
//! adding a built-in or custom profile and editing one, with a live preview
//! of the command it runs and its own scripts. Every change is checked by
//! the core when it's saved; nothing here starts an emulator. A right click
//! on an emulator or a profile offers its buttons.

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

/// The key of a form that adds rather than edits.
const NEW_KEY: &str = "new";

/// Display and edit quoting only, for tokens that need it; the core splits
/// and runs them.
pub fn argv_text(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if !arg.is_empty()
                && !arg
                    .chars()
                    .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '\\')
            {
                arg.clone()
            } else {
                format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A profile as its editor holds it: the profile, and its arguments and
/// hooks as typed. The core splits the text when it's saved or previewed.
#[derive(Clone, PartialEq)]
pub struct ProfileEdit {
    pub profile: EmulatorProfile,
    pub args_text: String,
    pub before_text: String,
    pub after_exit_text: String,
}

impl ProfileEdit {
    pub fn of(profile: &EmulatorProfile) -> Self {
        let (args, hooks) = match profile {
            EmulatorProfile::Custom { args, hooks, .. } => (args.clone(), hooks),
            EmulatorProfile::BuiltIn { args, hooks, .. } => (args.args().to_vec(), hooks),
        };
        Self {
            profile: profile.clone(),
            args_text: argv_text(&args),
            before_text: argv_text(hooks.before.as_deref().unwrap_or_default()),
            after_exit_text: argv_text(hooks.after_exit.as_deref().unwrap_or_default()),
        }
    }

    fn built_in(name: &str) -> Self {
        Self {
            profile: EmulatorProfile::built_in(name),
            args_text: String::new(),
            before_text: String::new(),
            after_exit_text: String::new(),
        }
    }

    fn custom() -> Self {
        Self {
            profile: EmulatorProfile::Custom {
                id: String::new(),
                name: t!("Custom profile"),
                executable: None,
                args: Vec::new(),
                working_dir: None,
                platforms: Vec::new(),
                extensions: Vec::new(),
                env: Vec::new(),
                tracking: Tracking::Default,
                hooks: ProfileHooks::default(),
            },
            args_text: "\"{RomPath}\"".to_owned(),
            before_text: String::new(),
            after_exit_text: String::new(),
        }
    }

    /// The profile with its typed arguments and hooks split, as the core
    /// will save it.
    fn parsed(&self) -> Result<EmulatorProfile, ErrorReport> {
        let args = Emulation::split_args(&self.args_text)?;
        let before = Emulation::split_args(&self.before_text)?;
        let after_exit = Emulation::split_args(&self.after_exit_text)?;
        let hooks = ProfileHooks {
            before: (!before.is_empty()).then_some(before),
            after_exit: (!after_exit.is_empty()).then_some(after_exit),
        };
        Ok(match self.profile.clone() {
            EmulatorProfile::Custom {
                id,
                name,
                executable,
                working_dir,
                platforms,
                extensions,
                env,
                tracking,
                ..
            } => EmulatorProfile::Custom {
                id,
                name,
                executable,
                args,
                working_dir,
                platforms,
                extensions,
                env,
                tracking,
                hooks,
            },
            EmulatorProfile::BuiltIn {
                id,
                definition_profile,
                args: old,
                ..
            } => EmulatorProfile::BuiltIn {
                id,
                definition_profile,
                hooks,
                args: match old {
                    ArgsOverride::Default => ArgsOverride::Default,
                    ArgsOverride::Append { .. } => ArgsOverride::Append { args },
                    // An empty `replaced` keeps the stored one; the editor
                    // sets it when the user reviews a drift.
                    ArgsOverride::Replace { replaced, .. } => {
                        ArgsOverride::Replace { args, replaced }
                    }
                },
            },
        })
    }
}

fn empty_install(kind: &str) -> EmulatorInstall {
    match kind {
        "path" => EmulatorInstall::Path {
            binary: String::new(),
        },
        "flatpak" => EmulatorInstall::Flatpak {
            app_id: String::new(),
        },
        "app_image" => EmulatorInstall::AppImage {
            file: Default::default(),
        },
        "app_bundle" => EmulatorInstall::AppBundle {
            bundle: Default::default(),
        },
        _ => EmulatorInstall::Folder {
            dir: Default::default(),
            executable: Default::default(),
        },
    }
}

fn install_kind(install: &EmulatorInstall) -> &'static str {
    match install {
        EmulatorInstall::Folder { .. } => "folder",
        EmulatorInstall::Path { .. } => "path",
        EmulatorInstall::Flatpak { .. } => "flatpak",
        EmulatorInstall::AppImage { .. } => "app_image",
        EmulatorInstall::AppBundle { .. } => "app_bundle",
    }
}

fn install_kinds() -> Vec<SelectOption> {
    vec![
        SelectOption::new(
            "folder",
            t!(note = "How an emulator is installed: in a folder", "Folder"),
        ),
        SelectOption::new(
            "path",
            t!(
                note = "How an emulator is installed: a program found on the PATH",
                "$PATH binary"
            ),
        ),
        // Product names.
        SelectOption::new("flatpak", "Flatpak"),
        SelectOption::new("app_image", "AppImage"),
        SelectOption::new(
            "app_bundle",
            t!(
                note = "How an emulator is installed: a macOS app",
                "App bundle"
            ),
        ),
    ]
}

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

/// An emulator's name, which emulator it is and its install; `emulator`
/// absent adds one.
#[component]
fn EmulatorForm(
    emulator: Option<EmulatorView>,
    snapshot: Snapshot,
    on_close: EventHandler<()>,
    on_created: EventHandler<String>,
) -> Element {
    let emulation = use_emulation();
    let start = use_hook(|| match &emulator {
        Some(view) => view.emulator.clone(),
        None => Emulator {
            id: String::new(),
            name: t!(note = "The name a new emulator starts with", "New emulator"),
            definition: None,
            install: empty_install("folder"),
            profiles: Vec::new(),
        },
    });
    let mut draft = use_signal(|| start.clone());
    let mut saving = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let dirty = {
        let now = draft.read();
        now.name != start.name || now.definition != start.definition || now.install != start.install
    };
    let save = {
        let emulation = emulation.clone();
        let adding = emulator.is_none();
        std::rc::Rc::new(move || {
            let emulation = emulation.clone();
            let mut next = draft.peek().clone();
            Box::pin(async move {
                saving.set(true);
                failure.set(None);
                let saved = change(async move {
                    // Another emulator here drops the old one's built-in
                    // profiles; the user's own stay.
                    if !next.id.is_empty() {
                        let stored = emulation
                            .emulators()
                            .await?
                            .into_iter()
                            .find(|row| row.emulator.id == next.id)
                            .map(|row| row.emulator);
                        if let Some(stored) = stored {
                            next.profiles = if stored.definition == next.definition {
                                stored.profiles
                            } else {
                                stored
                                    .profiles
                                    .into_iter()
                                    .filter(|profile| {
                                        matches!(profile, EmulatorProfile::Custom { .. })
                                    })
                                    .collect()
                            };
                        }
                    }
                    emulation.save_emulator(next).await
                })
                .await;
                saving.set(false);
                match saved {
                    Ok(view) => {
                        if adding {
                            on_created.call(view.emulator.id);
                        }
                        true
                    }
                    Err(report) => {
                        failure.set(Some(report));
                        false
                    }
                }
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = bool>>>
        })
    };
    // The emulator's fields wait for Save emulator (#43): leaving the
    // section asks first.
    use_unsaved_changes(
        dirty,
        UnsavedForm {
            label: match &emulator {
                Some(view) => view.emulator.name.clone(),
                None => t!("Add emulator"),
            },
            save: save.clone(),
            discard: std::rc::Rc::new({
                let start = start.clone();
                move || {
                    let mut draft = draft;
                    draft.set(start.clone());
                }
            }),
        },
    );
    let current = draft();
    let mut definitions = vec![SelectOption::new("none", t!("None (custom)"))];
    definitions.extend(
        snapshot
            .catalog
            .definitions
            .iter()
            .map(|row| SelectOption::new(row.id.clone(), row.name.clone())),
    );
    let mut set_install = move |install: EmulatorInstall| draft.write().install = install;
    let changed_definition = emulator
        .as_ref()
        .is_some_and(|view| view.emulator.definition != current.definition);
    rsx! {
        SettingsSection {
            icon: CPU,
            title: match &emulator {
                Some(view) => view.emulator.name.clone(),
                None => t!("Add emulator"),
            },
            description: t!("Caskade checks the emulator is there when you save. Nothing here starts it."),
            div { class: "mt-4 grid gap-4",
                TextField {
                    id: "emulator-name",
                    label: t!("Emulator name"),
                    value: current.name.clone(),
                    on_input: move |name: String| draft.write().name = name,
                }
                Select {
                    id: "emulator-definition",
                    label: t!("Which emulator"),
                    options: definitions,
                    value: Some(current.definition.clone().unwrap_or_else(|| "none".to_owned())),
                    on_change: move |id: String| draft.write().definition = (id != "none").then_some(id),
                }
                Select {
                    id: "emulator-install-type",
                    label: t!("Install type"),
                    options: install_kinds(),
                    value: Some(install_kind(&current.install).to_owned()),
                    on_change: move |kind: String| set_install(empty_install(&kind)),
                }
                match current.install.clone() {
                    EmulatorInstall::Folder { dir, executable } => rsx! {
                        PathField {
                            id: "emulator-folder",
                            label: t!("Emulator folder"),
                            value: dir.to_string_lossy().into_owned(),
                            on_change: {
                                let executable = executable.clone();
                                move |dir: String| set_install(EmulatorInstall::Folder { dir: dir.into(), executable: executable.clone() })
                            },
                        }
                        PathField {
                            id: "emulator-executable",
                            label: t!("Emulator executable"),
                            file: true,
                            value: executable.to_string_lossy().into_owned(),
                            on_change: move |executable: String| set_install(EmulatorInstall::Folder { dir: dir.clone(), executable: executable.into() }),
                        }
                    },
                    EmulatorInstall::Path { binary } => rsx! {
                        TextField {
                            id: "emulator-binary",
                            label: t!("Binary name"),
                            mono: true,
                            value: binary,
                            on_input: move |binary: String| set_install(EmulatorInstall::Path { binary }),
                        }
                    },
                    EmulatorInstall::Flatpak { app_id } => rsx! {
                        TextField {
                            id: "emulator-flatpak",
                            label: t!("Flatpak app ID"),
                            mono: true,
                            value: app_id,
                            on_input: move |app_id: String| set_install(EmulatorInstall::Flatpak { app_id }),
                        }
                    },
                    EmulatorInstall::AppImage { file } => rsx! {
                        PathField {
                            id: "emulator-appimage",
                            label: t!("AppImage file"),
                            file: true,
                            value: file.to_string_lossy().into_owned(),
                            on_change: move |file: String| set_install(EmulatorInstall::AppImage { file: file.into() }),
                        }
                    },
                    EmulatorInstall::AppBundle { bundle } => rsx! {
                        PathField {
                            id: "emulator-bundle",
                            label: t!(note = "A macOS app bundle's path", "App bundle"),
                            value: bundle.to_string_lossy().into_owned(),
                            on_change: move |bundle: String| set_install(EmulatorInstall::AppBundle { bundle: bundle.into() }),
                        }
                    },
                }
            }
            if changed_definition {
                p { class: "mt-3 text-sm text-warning",
                    {t!("Saving another emulator here removes the old one's built-in profiles. Your own profiles are kept.")}
                }
            }
            if let Some(view) = emulator.clone() {
                div { class: "mt-3",
                    Status { emulator: view }
                }
            }
            if let Some(report) = failure() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
            }
            div { class: "mt-4 flex items-center justify-end gap-2",
                div { class: "mr-auto",
                    UnsavedHint { dirty }
                }
                if emulator.is_none() {
                    Button { disabled: saving(), on_press: move |_| on_close.call(()), {t!("Cancel")} }
                }
                Button {
                    id: "emulator-save",
                    variant: ButtonVariant::Primary,
                    disabled: saving(),
                    on_press: move |_| {
                        let save = save.clone();
                        // A failure shows above the buttons.
                        spawn(async move {
                            save().await;
                        });
                    },
                    if saving() {
                        {t!("Saving…")}
                    } else {
                        {t!("Save emulator")}
                    }
                }
            }
        }
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

/// A profile editor's identity: its id, or what it adds.
fn profile_key(profile: &EmulatorProfile) -> String {
    match profile {
        EmulatorProfile::BuiltIn {
            id,
            definition_profile,
            ..
        } if id.is_empty() => format!("built_in:{definition_profile}"),
        EmulatorProfile::Custom { id, .. } if id.is_empty() => "custom:new".to_owned(),
        _ => profile.id().to_owned(),
    }
}

/// The chosen emulator's profiles, each opening to its details, and
/// adding one.
#[component]
fn Profiles(
    view: EmulatorView,
    snapshot: Snapshot,
    writing: bool,
    on_edit: EventHandler<ProfileEdit>,
    on_remove: EventHandler<Removing>,
) -> Element {
    let mut open = use_signal(std::collections::BTreeSet::<String>::new);
    let mut menu = use_signal(|| None::<RowMenu>);
    let mut list_node = use_signal(|| None::<NodeHandle>);
    let no_platforms = t!(note = "An emulator profile's platforms", "None selected");
    let no_extensions = tp!("file extensions", "None");
    let has_definition = snapshot
        .catalog
        .definitions
        .iter()
        .any(|row| Some(&row.id) == view.emulator.definition.as_ref());
    let mut addable = view.addable.clone();
    addable.sort_by_key(|profile| !profile.available);
    let addable_items: Vec<MenuEntry> = addable
        .iter()
        .map(|profile| {
            MenuEntry::Item(MenuItem {
                description: Some(if profile.available {
                    t!(note = "An emulator profile's status", "Available")
                } else {
                    t!(note = "An emulator profile's status", "Missing files")
                }),
                ..MenuItem::new(profile.name.clone(), profile.name.clone())
            })
        })
        .collect();
    let profile_of = {
        let view = view.clone();
        move |id: &str| {
            view.emulator
                .profiles
                .iter()
                .find(|profile| profile.id() == id)
                .cloned()
        }
    };
    let on_menu_action = {
        let profile_of = profile_of.clone();
        let emulator_id = view.emulator.id.clone();
        move |action: String| {
            menu.set(None);
            let mut parts = action.splitn(3, ':');
            match (parts.next(), parts.next(), parts.next()) {
                (Some("edit"), Some(id), _) => {
                    if let Some(profile) = profile_of(id) {
                        on_edit.call(ProfileEdit::of(&profile));
                    }
                }
                (Some("remove"), Some(id), Some(name)) => on_remove.call(Removing::Profile {
                    emulator_id: emulator_id.clone(),
                    profile_id: id.to_owned(),
                    name: name.to_owned(),
                }),
                _ => {}
            }
        }
    };
    rsx! {
        SettingsSection {
            icon: LAYERS,
            title: tp!("emulator profiles", "Profiles"),
            description: t!("Whether each profile can play, the files it needs and the command it runs."),
            if view.profiles.is_empty() {
                p { class: "mt-3 rounded-control border border-dashed border-border p-5 text-sm text-fg-muted",
                    {t!("No profiles yet. Add a built-in or custom profile before launching games.")}
                }
            }
            div {
                class: "mt-3 space-y-3",
                onmounted: move |event| list_node.set(event.data().downcast::<NodeHandle>().cloned()),
                for state in view.profiles.iter().cloned() {
                    div {
                        key: "{state.id}",
                        "data-expanded": open.read().contains(&state.id).then_some("true"),
                        class: "rounded-control border border-border bg-surface-overlay/40",
                        oncontextmenu: {
                            let (id, name) = (state.id.clone(), state.name.clone());
                            move |event: MouseEvent| {
                                event.prevent_default();
                                event.stop_propagation();
                                let anchor = crate::frame::pointer_anchor(list_node.peek().as_ref(), &event);
                                let edit = MenuItem { icon: Some(PENCIL), ..MenuItem::new(format!("edit:{id}"), t!("Edit {profile}", profile = name.clone())) };
                                let remove = MenuItem {
                                    icon: Some(TRASH),
                                    danger: true,
                                    disabled: writing,
                                    ..MenuItem::new(format!("remove:{id}:{name}"), t!("Remove {profile}…", profile = name.clone()))
                                };
                                menu.set(Some(RowMenu { anchor, items: vec![MenuEntry::Item(edit), MenuEntry::Item(remove)] }));
                            }
                        },
                        h3 {
                            Button {
                                variant: ButtonVariant::Ghost,
                                "aria-expanded": if open.read().contains(&state.id) { "true" } else { "false" },
                                class: "flex w-full flex-wrap items-center justify-between gap-2 p-3 text-left",
                                on_press: {
                                    let id = state.id.clone();
                                    move |_| {
                                        let mut open = open.write();
                                        if !open.remove(&id) {
                                            open.insert(id.clone());
                                        }
                                    }
                                },
                                span { class: "font-medium", "{state.name}" }
                                {
                                    let available = state.availability == Availability::Available;
                                    let found = view.status != EmulatorStatus::NotFound;
                                    let (tone, text) = if available {
                                        (DotTone::Ok, t!(note = "An emulator's status", "Ready"))
                                    } else if found {
                                        (DotTone::Warning, t!(note = "An emulator profile's status", "Missing files"))
                                    } else {
                                        (DotTone::Danger, t!("Emulator not found"))
                                    };
                                    rsx! { StatusDot { tone, "{text}" } }
                                }
                            }
                        }
                        if open.read().contains(&state.id) {
                            div { class: "space-y-3 border-t border-border p-3",
                                p { class: "text-xs text-fg-muted",
                                    {
                                        let names = state
                                            .platforms
                                            .iter()
                                            .map(|id| {
                                                snapshot
                                                    .platforms
                                                    .iter()
                                                    .find(|row| &row.platform.id == id)
                                                    .map_or_else(|| id.clone(), |row| row.platform.short.clone())
                                            })
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        t!(
                                            note = "An emulator profile's platforms",
                                            "Platforms: {platforms}",
                                            platforms = if names.is_empty() { no_platforms.clone() } else { names }
                                        )
                                    }
                                }
                                p { class: "font-mono text-xs text-fg-muted",
                                    {
                                        let extensions = state.extensions.join(", ");
                                        t!(
                                            note = "The file extensions an emulator profile opens",
                                            "File types: {extensions}",
                                            extensions = if extensions.is_empty() { no_extensions.clone() } else { extensions }
                                        )
                                    }
                                }
                                if let Availability::MissingFiles { missing } = &state.availability {
                                    ul { class: "space-y-1 font-mono text-xs text-warning",
                                        for path in missing.iter() {
                                            li { key: "{path}",
                                                {t!(note = "A file an emulator profile needs", "Missing: {path}", path = path.clone())}
                                            }
                                        }
                                    }
                                }
                                if state.drift.is_some() {
                                    p { "data-part": "drift", class: "text-sm text-warning",
                                        {t!("Caskade's arguments for this emulator changed since you replaced them. Edit the profile to review them.")}
                                    }
                                }
                                BiosSummary { files: state.bios.clone() }
                                PreviewTokens { preview: Some(state.preview.clone()) }
                                div { class: "flex flex-wrap justify-end gap-2",
                                    Button {
                                        size: ButtonSize::Sm,
                                        on_press: {
                                            let profile_of = profile_of.clone();
                                            let id = state.id.clone();
                                            move |_| {
                                                if let Some(profile) = profile_of(&id) {
                                                    on_edit.call(ProfileEdit::of(&profile));
                                                }
                                            }
                                        },
                                        {t!("Edit {profile}", profile = state.name.clone())}
                                    }
                                    Button {
                                        size: ButtonSize::Sm,
                                        variant: ButtonVariant::Danger,
                                        disabled: writing,
                                        on_press: {
                                            let (emulator_id, id, name) = (view.emulator.id.clone(), state.id.clone(), state.name.clone());
                                            move |_| on_remove.call(Removing::Profile {
                                                emulator_id: emulator_id.clone(),
                                                profile_id: id.clone(),
                                                name: name.clone(),
                                            })
                                        },
                                        {t!("Remove {profile}…", profile = state.name.clone())}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "mt-3 flex flex-wrap gap-2",
                if has_definition {
                    MenuButton {
                        id: "profile-add-built-in",
                        label: t!("Add built-in profile"),
                        icon: PLUS,
                        variant: ButtonVariant::Secondary,
                        disabled: addable_items.is_empty(),
                        items: addable_items,
                        on_action: move |name: String| on_edit.call(ProfileEdit::built_in(&name)),
                    }
                }
                Button {
                    id: "profile-add-custom",
                    icon: PLUS,
                    variant: ButtonVariant::Ghost,
                    on_press: move |_| on_edit.call(ProfileEdit::custom()),
                    {t!("Add custom profile")}
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
        }
    }
}

/// How playtime is tracked, by id.
fn tracking_modes() -> Vec<SelectOption> {
    vec![
        SelectOption::new("default", tp!("playtime tracking", "Default")),
        SelectOption::new("process_tree", tp!("playtime tracking", "Process tree")),
        SelectOption::new(
            "original_process",
            tp!("playtime tracking", "Original process only"),
        ),
        SelectOption::new("folder", tp!("playtime tracking", "Any process in folder")),
        SelectOption::new("process_name", tp!("playtime tracking", "Process name")),
    ]
}

fn tracking_id(tracking: &Tracking) -> &'static str {
    match tracking {
        Tracking::Default => "default",
        Tracking::ProcessTree => "process_tree",
        Tracking::OriginalProcess => "original_process",
        Tracking::Folder { .. } => "folder",
        Tracking::ProcessName { .. } => "process_name",
    }
}

fn args_mode(args: &ArgsOverride) -> &'static str {
    match args {
        ArgsOverride::Default => "default",
        ArgsOverride::Replace { .. } => "replace",
        ArgsOverride::Append { .. } => "append",
    }
}

async fn sleep(time: Duration) {
    // Timers need the app runtime's reactor; Dioxus's executor has none.
    let _ = caskade_app::runtime::spawn(async move { tokio::time::sleep(time).await }).await;
}

/// Adds or changes one of `emulator`'s profiles.
#[component]
fn ProfileEditor(
    emulator: EmulatorView,
    initial: ProfileEdit,
    snapshot: Snapshot,
    on_close: EventHandler<()>,
) -> Element {
    let emulation = use_emulation();
    let mut draft = use_signal(|| initial.clone());
    let mut extensions = use_signal(|| match &initial.profile {
        EmulatorProfile::Custom { extensions, .. } => extensions.join(", "),
        _ => String::new(),
    });
    let mut saving = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let mut preview =
        use_signal(|| None::<Result<caskade_core::emulation::ArgvPreview, ErrorReport>>);
    let mut previewing = use_signal(|| false);
    let dirty = draft() != initial;
    let current = draft();
    let profile = current.profile.clone();
    let title = match &profile {
        EmulatorProfile::Custom { .. } => t!("Custom profile"),
        EmulatorProfile::BuiltIn {
            definition_profile, ..
        } => definition_profile.clone(),
    };
    // The command line, again 200 ms after the last change.
    let mut generation = use_signal(|| 0u64);
    {
        let emulation = emulation.clone();
        let (install, definition) = (
            emulator.emulator.install.clone(),
            emulator.emulator.definition.clone(),
        );
        use_effect(use_reactive!(|current| {
            *generation.write() += 1;
            let mine = *generation.peek();
            let emulation = emulation.clone();
            let (install, definition) = (install.clone(), definition.clone());
            spawn(async move {
                sleep(Duration::from_millis(200)).await;
                if *generation.peek() != mine {
                    return;
                }
                previewing.set(true);
                let shown = current.parsed().map(|profile| {
                    emulation.preview(&ProfileDraft {
                        install,
                        definition,
                        profile,
                    })
                });
                previewing.set(false);
                preview.set(Some(shown));
            });
        }));
    }
    let save = {
        let emulation = emulation.clone();
        let emulator_id = emulator.emulator.id.clone();
        std::rc::Rc::new(move || {
            let emulation = emulation.clone();
            let emulator_id = emulator_id.clone();
            let edit = draft.peek().clone();
            Box::pin(async move {
                saving.set(true);
                failure.set(None);
                let saved = change(async move {
                    let parsed = edit.parsed()?;
                    let mut stored = emulation
                        .emulators()
                        .await?
                        .into_iter()
                        .find(|row| row.emulator.id == emulator_id)
                        .map(|row| row.emulator)
                        .ok_or_else(gone)?;
                    match stored
                        .profiles
                        .iter_mut()
                        .find(|profile| !parsed.id().is_empty() && profile.id() == parsed.id())
                    {
                        Some(profile) => *profile = parsed,
                        None => stored.profiles.push(parsed),
                    }
                    emulation.save_emulator(stored).await
                })
                .await;
                saving.set(false);
                match saved {
                    Ok(_) => {
                        on_close.call(());
                        true
                    }
                    Err(report) => {
                        failure.set(Some(report));
                        false
                    }
                }
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = bool>>>
        })
    };
    // The profile waits for Save profile (#43): leaving the section asks
    // first.
    use_unsaved_changes(
        dirty,
        UnsavedForm {
            label: format!("{}: {title}", emulator.emulator.name),
            save: save.clone(),
            discard: std::rc::Rc::new({
                let initial = initial.clone();
                move || {
                    let (mut draft, mut extensions) = (draft, extensions);
                    extensions.set(match &initial.profile {
                        EmulatorProfile::Custom { extensions, .. } => extensions.join(", "),
                        _ => String::new(),
                    });
                    draft.set(initial.clone());
                }
            }),
        },
    );
    let state = emulator
        .profiles
        .iter()
        .find(|row| !profile.id().is_empty() && row.id == profile.id())
        .cloned();
    // Cleared once the user picks how to resolve it; saving then records
    // the reviewed arguments.
    let drift = state.as_ref().and_then(|row| row.drift.clone()).filter(|drift| {
        !matches!(&profile, EmulatorProfile::BuiltIn { args: ArgsOverride::Replace { replaced, .. }, .. } if *replaced == drift.definition_args)
    });
    let definition = snapshot
        .catalog
        .definitions
        .iter()
        .find(|row| Some(&row.id) == emulator.emulator.definition.as_ref())
        .cloned();
    let definition_profile = match &profile {
        EmulatorProfile::BuiltIn {
            definition_profile, ..
        } => definition.as_ref().and_then(|definition| {
            definition
                .profiles
                .iter()
                .find(|row| &row.name == definition_profile)
                .cloned()
        }),
        _ => None,
    };
    let mut variables: Vec<(String, String)> = snapshot
        .catalog
        .variables
        .iter()
        .flat_map(|row| {
            std::iter::once((row.name.clone(), row.description.clone())).chain(
                row.aliases
                    .iter()
                    .map(|alias| (alias.clone(), format!("Alias for {{{}}}", row.name))),
            )
        })
        .collect();
    for name in definition
        .as_ref()
        .map(|d| d.vars.clone())
        .unwrap_or_default()
    {
        if !variables.iter().any(|(known, _)| *known == name) {
            variables.push((name, t!("Set by Caskade for this emulator")));
        }
    }
    let mut set_profile = move |next: EmulatorProfile| draft.write().profile = next;
    let shows_args = match &profile {
        EmulatorProfile::Custom { .. } => true,
        EmulatorProfile::BuiltIn { args, .. } => *args != ArgsOverride::Default,
    };
    let args_label = match &profile {
        EmulatorProfile::Custom { .. } => t!("Arguments"),
        EmulatorProfile::BuiltIn {
            args: ArgsOverride::Replace { .. },
            ..
        } => t!("Replacement arguments"),
        _ => t!("Extra arguments"),
    };
    let no_args = t!(note = "No arguments", "(none)");
    let shown_preview = match preview() {
        Some(Ok(preview)) => Some(preview),
        _ => state.as_ref().map(|row| row.preview.clone()),
    };
    let preview_error = match preview() {
        Some(Err(report)) => Some(report.summary),
        _ => None,
    };
    rsx! {
        SettingsSection {
            title: title.clone(),
            description: t!("Caskade checks the profile when you save. Nothing here starts the emulator."),
            aside: rsx! {
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    icon: X,
                    "aria-label": t!("Close profile editor"),
                    disabled: saving(),
                    on_press: move |_| on_close.call(()),
                }
            },
            div { class: "mt-4 space-y-4",
                match profile.clone() {
                    EmulatorProfile::Custom { .. } => rsx! {
                        CustomFields {
                            profile: profile.clone(),
                            extensions: extensions(),
                            snapshot: snapshot.clone(),
                            on_change: move |next: EmulatorProfile| set_profile(next),
                            on_extensions: move |text: String| {
                                let parsed = parse_extensions(&text);
                                extensions.set(text);
                                if let EmulatorProfile::Custom { extensions, .. } = &mut draft.write().profile {
                                    *extensions = parsed;
                                }
                            },
                        }
                    },
                    EmulatorProfile::BuiltIn { args, .. } => rsx! {
                        p { class: "text-sm text-fg-muted",
                            {
                                let names = definition_profile
                                    .as_ref()
                                    .map(|row| {
                                        row.platforms
                                            .iter()
                                            .map(|id| {
                                                snapshot
                                                    .platforms
                                                    .iter()
                                                    .find(|platform| &platform.platform.id == id)
                                                    .map_or_else(|| id.clone(), |platform| platform.platform.name.clone())
                                            })
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                    .unwrap_or_default();
                                t!(
                                    note = "An emulator profile's platforms",
                                    "Platforms: {platforms}",
                                    platforms = if names.is_empty() { t!("Unknown built-in profile") } else { names }
                                )
                            }
                        }
                        p { class: "font-mono text-xs text-fg-muted",
                            {
                                let extensions = definition_profile.as_ref().map(|row| row.extensions.join(", ")).unwrap_or_default();
                                t!(
                                    note = "The file extensions an emulator profile opens",
                                    "File types: {extensions}",
                                    extensions = if extensions.is_empty() { tp!("file extensions", "None") } else { extensions }
                                )
                            }
                        }
                        if let Some(row) = definition_profile.clone().filter(|row| !row.requires.is_empty()) {
                            ul { class: "space-y-1 text-xs text-fg-muted",
                                for path in row.requires {
                                    li { key: "{path}",
                                        {t!(note = "A file an emulator profile needs", "Requires: {path}", path = path.clone())}
                                    }
                                }
                            }
                        }
                        SegmentedControl {
                            "aria-label": t!("Arguments mode"),
                            options: vec![
                                Segment::new("default", tp!(note = "Use the specification's arguments", "emulator arguments", "Default")),
                                Segment::new("replace", tp!(note = "Replace the specification's arguments", "emulator arguments", "Replace")),
                                Segment::new("append", tp!(note = "Add arguments after the specification's", "emulator arguments", "Append")),
                            ],
                            value: args_mode(&args),
                            on_change: {
                                let definition_args = definition_profile.as_ref().map(|row| row.args.clone()).unwrap_or_default();
                                move |mode: String| {
                                    let mut next = draft.write();
                                    // Replace starts from the specification's
                                    // arguments, so it is an edit.
                                    if mode == "replace" && next.args_text.trim().is_empty() {
                                        next.args_text = argv_text(&definition_args);
                                    }
                                    if let EmulatorProfile::BuiltIn { args, .. } = &mut next.profile {
                                        *args = match mode.as_str() {
                                            "replace" => ArgsOverride::Replace { args: Vec::new(), replaced: Vec::new() },
                                            "append" => ArgsOverride::Append { args: Vec::new() },
                                            _ => ArgsOverride::Default,
                                        };
                                    }
                                }
                            },
                        }
                        if let Some(drift) = drift.clone().filter(|_| matches!(args, ArgsOverride::Replace { .. })) {
                            div {
                                role: "status",
                                "data-part": "drift",
                                class: "space-y-2 rounded-control border border-warning/40 p-3 text-sm text-warning",
                                p { {t!("Caskade's arguments for this emulator changed since you replaced them.")} }
                                p { class: "break-all font-mono text-xs text-fg-muted",
                                    {
                                        let then = argv_text(&drift.overridden_from);
                                        t!(note = "The arguments a Replace override was saved with", "Replaced then: {args}", args = if then.is_empty() { no_args.clone() } else { then })
                                    }
                                }
                                p { class: "break-all font-mono text-xs text-fg-muted",
                                    {
                                        let now = argv_text(&drift.definition_args);
                                        t!(note = "The arguments Caskade's definition of the emulator gives now", "Caskade's now: {args}", args = if now.is_empty() { no_args.clone() } else { now })
                                    }
                                }
                                div { class: "flex flex-wrap gap-2",
                                    Button {
                                        size: ButtonSize::Sm,
                                        on_press: {
                                            let drift = drift.clone();
                                            move |_| {
                                                let mut next = draft.write();
                                                next.args_text = argv_text(&drift.definition_args);
                                                if let EmulatorProfile::BuiltIn { args, .. } = &mut next.profile {
                                                    *args = ArgsOverride::Replace { args: Vec::new(), replaced: drift.definition_args.clone() };
                                                }
                                            }
                                        },
                                        {t!("Start from the new arguments")}
                                    }
                                    Button {
                                        size: ButtonSize::Sm,
                                        on_press: {
                                            let drift = drift.clone();
                                            move |_| {
                                                if let EmulatorProfile::BuiltIn { args: ArgsOverride::Replace { replaced, .. }, .. } = &mut draft.write().profile {
                                                    *replaced = drift.definition_args.clone();
                                                }
                                            }
                                        },
                                        {t!("Keep my arguments")}
                                    }
                                }
                            }
                        }
                    },
                }
                if shows_args {
                    TextField {
                        id: "profile-args",
                        label: args_label,
                        mono: true,
                        value: current.args_text.clone(),
                        description: t!("Split into arguments as a shell would, but no shell runs. Quote a value that has spaces."),
                        on_input: move |text: String| draft.write().args_text = text,
                    }
                    div { "data-part": "variables", class: "flex flex-wrap gap-1.5",
                        for (name, description) in variables {
                            Button {
                                key: "{name}",
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Ghost,
                                "aria-label": t!(
                                    note = "Inserts a launch variable into the arguments",
                                    "Insert {variable}: {description}",
                                    variable = format!("{{{name}}}"),
                                    description = description.clone()
                                ),
                                on_press: {
                                    let name = name.clone();
                                    move |_| {
                                        let mut next = draft.write();
                                        let trimmed = next.args_text.trim_end().to_owned();
                                        let space = if next.args_text.trim().is_empty() { "" } else { " " };
                                        next.args_text = format!("{trimmed}{space}\"{{{name}}}\"");
                                    }
                                },
                                {format!("{{{name}}}")}
                            }
                        }
                    }
                }
                if let EmulatorProfile::Custom { env, .. } = profile.clone() {
                    fieldset {
                        legend { class: "mb-2 text-sm font-medium", {t!(note = "Environment variables", "Environment")} }
                        div { class: "space-y-2",
                            for (index, (name, value)) in env.iter().cloned().enumerate() {
                                div { key: "{index}", class: "grid items-end gap-2 sm:grid-cols-[1fr_2fr_auto]",
                                    TextField {
                                        "aria-label": t!("Environment name {row}", row = index + 1),
                                        value: name,
                                        mono: true,
                                        on_input: move |next: String| {
                                            if let EmulatorProfile::Custom { env, .. } = &mut draft.write().profile && let Some(row) = env.get_mut(index) {
                                                row.0 = next;
                                            }
                                        },
                                    }
                                    TextField {
                                        "aria-label": t!("Environment value {row}", row = index + 1),
                                        value,
                                        mono: true,
                                        on_input: move |next: String| {
                                            if let EmulatorProfile::Custom { env, .. } = &mut draft.write().profile && let Some(row) = env.get_mut(index) {
                                                row.1 = next;
                                            }
                                        },
                                    }
                                    Button {
                                        "aria-label": t!("Remove environment row {row}", row = index + 1),
                                        on_press: move |_| {
                                            if let EmulatorProfile::Custom { env, .. } = &mut draft.write().profile && index < env.len() {
                                                env.remove(index);
                                            }
                                        },
                                        {tp!("environment row", "Remove")}
                                    }
                                }
                            }
                        }
                        Button {
                            class: "mt-2",
                            size: ButtonSize::Sm,
                            icon: PLUS,
                            on_press: move |_| {
                                if let EmulatorProfile::Custom { env, .. } = &mut draft.write().profile {
                                    env.push((String::new(), String::new()));
                                }
                            },
                            {t!("Add environment variable")}
                        }
                    }
                }
                div { class: "grid gap-4",
                    TextField {
                        id: "profile-before",
                        label: t!("Before launch"),
                        description: t!("Optional. A command to run first; if it fails, the game doesn't start."),
                        mono: true,
                        value: current.before_text.clone(),
                        on_input: move |text: String| draft.write().before_text = text,
                    }
                    TextField {
                        id: "profile-after",
                        label: t!("After exit"),
                        mono: true,
                        value: current.after_exit_text.clone(),
                        on_input: move |text: String| draft.write().after_exit_text = text,
                    }
                }
                PreviewTokens { preview: shown_preview, pending: previewing(), error: preview_error }
                if let Some(report) = failure() {
                    p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
                }
                div { class: "flex items-center justify-end gap-2",
                    div { class: "mr-auto",
                        UnsavedHint { dirty }
                    }
                    Button { disabled: saving(), on_press: move |_| on_close.call(()), {t!("Cancel")} }
                    Button {
                        id: "profile-save",
                        variant: ButtonVariant::Primary,
                        disabled: saving(),
                        on_press: move |_| {
                            let save = save.clone();
                            // A failure shows above the buttons.
                            spawn(async move {
                                save().await;
                            });
                        },
                        if saving() {
                            {t!("Saving…")}
                        } else {
                            {t!("Save profile")}
                        }
                    }
                }
                // The user's lifecycle scripts for this profile (#180),
                // saved on their own, apart from the fields above.
                if state.is_some() {
                    section { "aria-label": t!("Profile scripts"), class: "border-t border-border pt-4",
                        h3 { class: "mb-3 font-display text-base font-semibold", {t!("Profile scripts")} }
                        ScriptsPanel {
                            scope: ScriptScope::Profile { profile_id: profile.id().to_owned() },
                            profile_id: profile.id().to_owned(),
                        }
                    }
                }
            }
        }
    }
}

/// "nes, .SFC, , <none>" → `["nes", "sfc", "<none>"]`, once each.
fn parse_extensions(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in text.split(',') {
        let token = token.trim();
        let token = token.strip_prefix('.').unwrap_or(token).to_lowercase();
        if !token.is_empty() && !out.contains(&token) {
            out.push(token);
        }
    }
    out
}

/// A custom profile's own fields: name, program, folder, file types,
/// platforms and playtime tracking.
#[component]
fn CustomFields(
    profile: EmulatorProfile,
    extensions: String,
    snapshot: Snapshot,
    on_change: EventHandler<EmulatorProfile>,
    on_extensions: EventHandler<String>,
) -> Element {
    let EmulatorProfile::Custom {
        name,
        executable,
        working_dir,
        platforms,
        tracking,
        ..
    } = profile.clone()
    else {
        return rsx! {};
    };
    let with = {
        let profile = profile.clone();
        move |change: &dyn Fn(&mut EmulatorProfile)| {
            let mut next = profile.clone();
            change(&mut next);
            on_change.call(next);
        }
    };
    let with = std::rc::Rc::new(with);
    rsx! {
        div { class: "grid gap-4",
            TextField {
                id: "profile-name",
                label: t!("Profile name"),
                value: name,
                on_input: {
                    let with = with.clone();
                    move |next: String| with(&|profile| {
                        if let EmulatorProfile::Custom { name, .. } = profile {
                            *name = next.clone();
                        }
                    })
                },
            }
            PathField {
                id: "profile-executable",
                label: t!(note = "An emulator profile's program", "Executable"),
                file: true,
                value: executable.map(|path| path.to_string_lossy().into_owned()).unwrap_or_default(),
                on_change: {
                    let with = with.clone();
                    move |next: String| with(&|profile| {
                        if let EmulatorProfile::Custom { executable, .. } = profile {
                            *executable = (!next.is_empty()).then(|| next.clone().into());
                        }
                    })
                },
            }
            TextField {
                id: "profile-working-dir",
                label: t!("Working directory"),
                mono: true,
                value: working_dir.unwrap_or_default(),
                on_input: {
                    let with = with.clone();
                    move |next: String| with(&|profile| {
                        if let EmulatorProfile::Custom { working_dir, .. } = profile {
                            *working_dir = (!next.is_empty()).then(|| next.clone());
                        }
                    })
                },
            }
            TextField {
                id: "profile-extensions",
                label: t!("File types"),
                description: t!(
                    note = "<none> is typed as is; don't translate it",
                    "Comma-separated, without dots. <none> matches a file without an extension."
                ),
                value: extensions,
                on_input: move |text: String| on_extensions.call(text),
            }
        }
        fieldset {
            legend { class: "mb-2 text-sm font-medium", {tp!("emulation section", "Platforms")} }
            div { class: "grid gap-2 sm:grid-cols-2",
                for platform in snapshot.platforms.iter().map(|row| row.platform.clone()) {
                    Checkbox {
                        key: "{platform.id}",
                        label: platform.name.clone(),
                        checked: platforms.contains(&platform.id),
                        on_change: {
                            let with = with.clone();
                            let id = platform.id.clone();
                            move |selected: bool| with(&|profile| {
                                if let EmulatorProfile::Custom { platforms, .. } = profile {
                                    platforms.retain(|known| *known != id);
                                    if selected {
                                        platforms.push(id.clone());
                                    }
                                }
                            })
                        },
                    }
                }
            }
        }
        Select {
            id: "profile-tracking",
            label: t!("Playtime tracking"),
            options: tracking_modes(),
            value: Some(tracking_id(&tracking).to_owned()),
            on_change: {
                let with = with.clone();
                move |mode: String| with(&|profile| {
                    if let EmulatorProfile::Custom { tracking, .. } = profile {
                        *tracking = match mode.as_str() {
                            "process_tree" => Tracking::ProcessTree,
                            "original_process" => Tracking::OriginalProcess,
                            "folder" => Tracking::Folder { path: String::new() },
                            "process_name" => Tracking::ProcessName { name: String::new() },
                            _ => Tracking::Default,
                        };
                    }
                })
            },
        }
        match tracking {
            Tracking::Folder { path } => rsx! {
                TextField {
                    id: "profile-tracking-folder",
                    label: t!("Tracking folder"),
                    mono: true,
                    value: path,
                    on_input: {
                        let with = with.clone();
                        move |next: String| with(&|profile| {
                            if let EmulatorProfile::Custom { tracking, .. } = profile {
                                *tracking = Tracking::Folder { path: next.clone() };
                            }
                        })
                    },
                }
            },
            Tracking::ProcessName { name } => rsx! {
                TextField {
                    id: "profile-tracking-name",
                    label: tp!("playtime tracking", "Process name"),
                    mono: true,
                    value: name,
                    on_input: {
                        let with = with.clone();
                        move |next: String| with(&|profile| {
                            if let EmulatorProfile::Custom { tracking, .. } = profile {
                                *tracking = Tracking::ProcessName { name: next.clone() };
                            }
                        })
                    },
                }
            },
            _ => rsx! {},
        }
    }
}

//! Settings → Storage (#104), as the webview's `StorageSettings` and
//! `StoragePanel`: where Caskade installs games. Each location as a card
//! with its status, space and games (a right click offers its actions);
//! adding one (the path picker, or typed); the selected one's label and
//! folder template, made the default, or removed, keeping its files or
//! moving its games to another location first (one job in Activity, #64);
//! and "always use the default". Over engineer 2's `Storage` handle (#108),
//! which the install and move flows share.

use caskade_core::storage::{RootStatus, RootSummary};
use caskade_ui_data::storage::{MoveError, Storage};
use caskade_ui_i18n::{format, t, tn, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use serde_json::Value;

use super::state::{SettingsFeedback, use_settings_section};
use super::unsaved::{UnsavedForm, UnsavedHint, use_unsaved_changes};
use crate::data::change;
use crate::flows::paths::{PathRequest, PickKind, format_path, use_path_picker};
use crate::flows::storage::use_storage;
use crate::ui::icons::{FOLDER_PLUS, HARD_DRIVE, REFRESH_CW, STAR, TRASH};
use crate::ui::{
    Button, ButtonVariant, ConfirmDialog, Menu, MenuEntry, MenuItem, Select, SelectOption,
    SettingRow, SettingsSection, Switch, TextField,
};

fn status_label(status: RootStatus) -> String {
    match status {
        RootStatus::Online => tp!("storage location status", "online"),
        RootStatus::Offline => tp!("storage location status", "offline"),
        RootStatus::ReadOnly => tp!("storage location status", "read only"),
        RootStatus::IdentityMismatch => tp!(
            note = "The folder at the location's path isn't the location Caskade registered",
            "storage location status",
            "identity mismatch"
        ),
    }
}

fn status_name(status: RootStatus) -> &'static str {
    match status {
        RootStatus::Online => "online",
        RootStatus::Offline => "offline",
        RootStatus::ReadOnly => "read_only",
        RootStatus::IdentityMismatch => "identity_mismatch",
    }
}

fn location_name(root: &RootSummary) -> String {
    root.root
        .label
        .clone()
        .unwrap_or_else(|| format_path(&root.root.path.to_string_lossy()))
}

fn size(bytes: Option<u64>) -> String {
    format::size(bytes)
}

/// Settings → Storage: the panel, or why there's none.
#[component]
pub fn StorageSettings() -> Element {
    match use_storage().filter(Storage::manages_locations) {
        Some(storage) => rsx! {
            StoragePanel { storage }
        },
        None => rsx! {
            p { role: "alert", class: "p-4 text-sm text-fg-muted",
                {t!("Storage is unavailable. Open the Caskade desktop app to manage locations.")}
            }
        },
    }
}

#[component]
pub fn StoragePanel(storage: Storage) -> Element {
    let list = storage.clone();
    let mut roots = use_resource(move || {
        let storage = list.clone();
        change(async move { storage.roots().await })
    });
    let prefs = use_settings_section("core.storage");
    let picker = use_path_picker();
    let mut selected = use_signal(|| None::<String>);
    let mut path = use_signal(String::new);
    let mut label = use_signal(String::new);
    let mut target = use_signal(String::new);
    let mut remove = use_signal(|| false);
    let mut confirm_move = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None);
    // Runs a change to the locations, then reads them again: a move that
    // stopped part way changed them too.
    let run = use_callback(
        move |work: std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<Option<String>, caskade_ui_data::ErrorReport>,
                    > + Send,
            >,
        >| {
            message.set(String::new());
            failed.set(None);
            pending.set(true);
            spawn(async move {
                let outcome = change(work).await;
                pending.set(false);
                match outcome {
                    Ok(Some(done)) => message.set(done),
                    Ok(None) => {}
                    Err(error) => failed.set(Some(error)),
                }
                roots.restart();
            });
        },
    );
    let read = roots.read().clone();
    let (list, load_error) = match read {
        Some(Ok(list)) => (list, None),
        Some(Err(error)) => (Vec::new(), Some(error)),
        None => (Vec::new(), None),
    };
    let loading = roots.read().is_none();
    let active = list
        .iter()
        .find(|root| Some(&root.root.id) == selected.read().as_ref())
        .or(list.first())
        .cloned();
    let destinations: Vec<RootSummary> = list
        .iter()
        .filter(|root| {
            Some(&root.root.id) != active.as_ref().map(|active| &active.root.id)
                && root.status == RootStatus::Online
        })
        .cloned()
        .collect();
    let destination = destinations
        .iter()
        .find(|root| root.root.id == target())
        .or(destinations.first())
        .cloned();
    let mut choose = move |id: String| {
        selected.set(Some(id));
        remove.set(false);
    };
    let error = load_error.or(failed());
    let working = pending();
    let set_default = {
        let storage = storage.clone();
        move |id: String| {
            let storage = storage.clone();
            run(Box::pin(async move {
                storage.set_default(&id).await.map(|_| None)
            }));
        }
    };
    let browse = move |_| {
        let start = path.peek().trim().to_owned();
        spawn(async move {
            let mut request = PathRequest::new(PickKind::Folder, t!("Add a storage location"));
            if !start.is_empty() {
                request.start_at = Some(start);
            }
            if let Some(chosen) = picker.pick(request).await {
                path.set(chosen);
            }
        });
    };
    let add = {
        let storage = storage.clone();
        move |_| {
            let storage = storage.clone();
            let folder = path.peek().trim().to_owned();
            let name = Some(label.peek().trim().to_owned()).filter(|name| !name.is_empty());
            spawn(async move {
                message.set(String::new());
                failed.set(None);
                pending.set(true);
                let outcome = change(async move { storage.add(folder, name).await }).await;
                pending.set(false);
                match outcome {
                    Ok(root) => {
                        selected.set(Some(root.root.id));
                        path.set(String::new());
                        label.set(String::new());
                        message.set(t!(
                            "Location added. Caskade looked there for games it installed before."
                        ));
                    }
                    Err(error) => failed.set(Some(error)),
                }
                roots.restart();
            });
        }
    };
    rsx! {
        div { "data-cs": "storage-settings", "data-settings": "storage", class: "flex flex-col gap-6",
            SettingsFeedback { error: error.clone(), saving: working }
            SettingsSection {
                icon: Some(HARD_DRIVE),
                title: t!("Storage locations"),
                description: t!("Choose where Caskade installs games. Disconnected locations remain in your library."),
                div { class: "flex items-center justify-between gap-3 py-3",
                    p { class: "text-sm text-fg-muted",
                        if error.is_some() && list.is_empty() {
                            {t!("Locations unavailable")}
                        } else {
                            {tn!("# registered location", "# registered locations", list.len())}
                        }
                    }
                    Button {
                        icon: Some(REFRESH_CW),
                        disabled: loading,
                        on_press: move |_| roots.restart(),
                        {t!("Refresh")}
                    }
                }
                if loading {
                    p { role: "status", {t!("Checking locations…")} }
                } else if error.is_none() && list.is_empty() {
                    p { class: "p-3 text-sm text-fg-muted",
                        {t!("Add a folder below to choose your first install location.")}
                    }
                }
                div { class: "grid gap-3 md:grid-cols-2",
                    for root in list.clone() {
                        LocationCard {
                            key: "{root.root.id}",
                            selected: Some(&root.root.id) == active.as_ref().map(|active| &active.root.id),
                            working,
                            on_choose: move |id: String| choose(id),
                            on_default: {
                                let set_default = set_default.clone();
                                move |id: String| set_default(id)
                            },
                            on_remove: move |id: String| {
                                choose(id);
                                remove.set(true);
                            },
                            root,
                        }
                    }
                }
                div {
                    "data-part": "add-root",
                    class: "mt-4 flex flex-col gap-3 rounded-panel border border-border p-4",
                    h3 { class: "font-medium", {t!("Add a location")} }
                    TextField {
                        label: t!("Folder"),
                        value: path(),
                        mono: true,
                        on_input: move |next: String| path.set(next),
                        action: rsx! {
                            Button { disabled: working, on_press: browse, {t!("Browse…")} }
                        },
                    }
                    TextField {
                        label: t!(note = "A storage location's name", "Label"),
                        value: label(),
                        placeholder: t!(note = "An example name for a storage location", "Games SSD"),
                        on_input: move |next: String| label.set(next),
                    }
                    div {
                        Button {
                            icon: Some(FOLDER_PLUS),
                            disabled: path().trim().is_empty() || working,
                            on_press: add,
                            {t!("Add location")}
                        }
                    }
                }
            }
            // Beside the locations it's about (#54).
            if !message().is_empty() {
                p { role: "status", class: "text-sm text-fg-muted", "{message}" }
            }
            if let Some(active) = active.clone() {
                SettingsSection {
                    title: active.root.label.clone().unwrap_or_else(|| t!("Selected location")),
                    description: format_path(&active.root.path.to_string_lossy()),
                    "data-part": "selected-root",
                    RootEditor {
                        key: "{active.root.id}",
                        root: active.clone(),
                        disabled: working,
                        storage: storage.clone(),
                        on_saved: move |_| roots.restart(),
                        on_failed: move |error| failed.set(Some(error)),
                    }
                    div { class: "my-4 flex flex-wrap gap-2",
                        Button {
                            disabled: active.root.is_default || active.status != RootStatus::Online || working,
                            on_press: {
                                let set_default = set_default.clone();
                                let id = active.root.id.clone();
                                move |_| set_default(id.clone())
                            },
                            {tp!("storage location", "Make default")}
                        }
                        Button {
                            disabled: active.root.is_default || working,
                            on_press: move |_| remove.toggle(),
                            {t!("Remove location…")}
                        }
                    }
                    if active.root.is_default {
                        p { class: "text-xs text-fg-muted",
                            {t!("Choose another default before removing this location.")}
                        }
                    }
                    if remove() {
                        RemoveRoot {
                            root: active.clone(),
                            destinations: destinations.clone(),
                            destination: destination.clone(),
                            working,
                            on_target: move |id: String| target.set(id),
                            on_cancel: move |_| remove.set(false),
                            on_keep_files: {
                                let storage = storage.clone();
                                let id = active.root.id.clone();
                                move |_| {
                                    let storage = storage.clone();
                                    let id = id.clone();
                                    selected.set(None);
                                    remove.set(false);
                                    run(Box::pin(async move { storage.remove(&id).await.map(|_| None) }));
                                }
                            },
                            on_move: move |_| confirm_move.set(true),
                        }
                    }
                    if confirm_move() {
                        if let Some(to) = destination.clone() {
                            MoveAndRemoveDialog {
                                root: active.clone(),
                                to: to.clone(),
                                on_close: move |_| confirm_move.set(false),
                                on_confirm: {
                                    let storage = storage.clone();
                                    let id = active.root.id.clone();
                                    move |_| {
                                        confirm_move.set(false);
                                        let storage = storage.clone();
                                        let from = id.clone();
                                        let into = to.root.id.clone();
                                        selected.set(None);
                                        remove.set(false);
                                        // One job in Activity: this waits for it to end.
                                        run(Box::pin(async move {
                                            storage
                                                .move_and_remove(from, into)
                                                .await
                                                .map(|()| Some(t!("Move completed. Library identity and playtime were preserved.")))
                                                .map_err(move_error)
                                        }));
                                    }
                                },
                            }
                        }
                    }
                    if working {
                        p { role: "status", class: "mt-3 text-sm text-fg-muted",
                            {t!("Working… progress and cancellation are available in Activity.")}
                        }
                    }
                }
            }
            SettingsSection {
                title: t!("Install location preference"),
                description: t!("Free space and required capabilities are checked for every job."),
                SettingRow {
                    label: t!("Always use the default location"),
                    description: t!("Skip the picker when the default location is available. A single eligible location is selected automatically."),
                    Switch {
                        disabled: prefs.value().is_none() || prefs.saving(),
                        checked: prefs
                            .value()
                            .and_then(|value| value.get("alwaysUseDefault").and_then(Value::as_bool))
                            .unwrap_or(false),
                        on_change: move |on: bool| prefs.set_field("alwaysUseDefault", Value::Bool(on)),
                    }
                }
                SettingsFeedback { error: prefs.error(), saving: prefs.saving() }
            }
        }
    }
}

/// What a move that didn't finish says.
fn move_error(error: MoveError) -> caskade_ui_data::ErrorReport {
    let summary = match error {
        MoveError::Unavailable => {
            t!("Storage is unavailable. Open the Caskade desktop app to manage locations.")
        }
        MoveError::NeedsUser => {
            t!("This job needs a response in Activity before it can continue.")
        }
        MoveError::Failed(message) => message,
    };
    caskade_ui_data::ErrorReport::new(caskade_ui_data::ErrorCode::Other, summary)
}

/// One location: a card that selects it, its status, path, space and
/// games; a right click offers Make default and Remove location….
#[component]
fn LocationCard(
    root: RootSummary,
    selected: bool,
    working: bool,
    on_choose: EventHandler<String>,
    on_default: EventHandler<String>,
    on_remove: EventHandler<String>,
) -> Element {
    let interaction = crate::ui::interact::use_interaction();
    let mut card = use_signal(|| None::<NodeHandle>);
    let mut menu = use_signal(|| None);
    let id = root.root.id.clone();
    let press = {
        let id = id.clone();
        move || on_choose.call(id.clone())
    };
    let status = status_label(root.status);
    let heading = root
        .root
        .label
        .clone()
        .unwrap_or_else(|| t!("Game storage"));
    let state = if root.root.is_default {
        t!(
            note = "The default storage location and its status",
            "Default · {status}",
            status = status.clone()
        )
    } else {
        status.clone()
    };
    let caps = &root.capabilities;
    let mut details = t!(
        note = "A storage location's games, free space and size",
        "{games} · {free} free / {size} total",
        games = tn!("# game", "# games", root.games.len()),
        free = size(caps.free_bytes),
        size = size(caps.total_bytes)
    );
    for extra in [&caps.filesystem, &caps.volume].into_iter().flatten() {
        details.push_str(&format!(" · {extra}"));
    }
    let mut make_default = MenuItem::new("default", tp!("storage location", "Make default"));
    make_default.icon = Some(STAR);
    make_default.disabled = root.root.is_default || root.status != RootStatus::Online || working;
    let mut remove = MenuItem::new("remove", t!("Remove location…"));
    remove.icon = Some(TRASH);
    remove.danger = true;
    remove.disabled = root.root.is_default || working;
    remove.description = root
        .root
        .is_default
        .then(|| t!("Choose another default first"));
    let items = vec![MenuEntry::Item(make_default), MenuEntry::Item(remove)];
    let name = location_name(&root);
    let key_press = press.clone();
    let up_press = press.clone();
    rsx! {
        div {
            role: "button",
            tabindex: "0",
            "aria-pressed": if selected { "true" } else { "false" },
            "data-part": "root",
            "data-status": status_name(root.status),
            "data-selected": crate::ui::interact::flag(selected),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            class: "block w-full min-w-0 cursor-default rounded-panel border border-border p-4 text-left outline-none transition-colors data-hovered:border-accent-alt/50 data-focus-visible:focus-ring data-selected:border-accent-alt",
            onmounted: move |event: Event<MountedData>| card.set(event.data().downcast::<NodeHandle>().cloned()),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onclick: move |_| press(),
            onkeydown: move |event| {
                interaction.key_down(&event, crate::ui::interact::Keys::EnterAndSpace, key_press.clone());
            },
            onkeyup: move |event| interaction.key_up(&event, up_press.clone()),
            oncontextmenu: move |event: MouseEvent| {
                menu.set(Some(crate::frame::pointer_anchor(card.peek().as_ref(), &event)));
            },
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items,
                    label: name.clone(),
                    on_close: move |_| menu.set(None),
                    on_action: {
                        let id = id.clone();
                        move |action: String| {
                            menu.set(None);
                            if action == "default" {
                                on_default.call(id.clone());
                            } else {
                                on_remove.call(id.clone());
                            }
                        }
                    },
                }
            }
            div { class: "flex justify-between gap-3",
                strong { "{heading}" }
                span { class: "text-xs text-fg-muted", "{state}" }
            }
            p { class: "mt-1 break-all font-mono text-xs text-fg-muted",
                {format_path(&root.root.path.to_string_lossy())}
            }
            Capacity { root: root.clone() }
            p { class: "mt-2 text-xs text-fg-muted", "{details}" }
            for warning in root.capabilities.warnings.iter() {
                p { key: "{warning}", class: "mt-1 text-xs text-warning", "{warning}" }
            }
        }
    }
}

/// How a location's space is used: games, other files, free.
#[component]
fn Capacity(root: RootSummary) -> Element {
    let total = root.capabilities.total_bytes.filter(|total| *total > 0);
    let share = |value: u64| match total {
        Some(total) => format!(
            "{}%",
            (value as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
        ),
        None => "0%".to_owned(),
    };
    let label = t!(
        note = "How a storage location's space is used",
        "Games {games}, other files {other}, free {free}",
        games = size(Some(root.games_bytes)),
        other = size(root.other_bytes),
        free = size(root.capabilities.free_bytes)
    );
    rsx! {
        div {
            "data-part": "capacity",
            role: "img",
            "aria-label": "{label}",
            class: "mt-3 flex h-2 overflow-hidden rounded-full bg-surface-base",
            div { "data-part": "capacity-games", class: "bg-accent", style: "width: {share(root.games_bytes)}" }
            div {
                "data-part": "capacity-other",
                class: "bg-fg-subtle/40",
                style: "width: {share(root.other_bytes.unwrap_or(0))}",
            }
        }
    }
}

/// The selected location's label and folder template, which wait for Save
/// location (#43): leaving the section asks first.
#[component]
fn RootEditor(
    root: RootSummary,
    disabled: bool,
    storage: Storage,
    on_saved: EventHandler<()>,
    on_failed: EventHandler<caskade_ui_data::ErrorReport>,
) -> Element {
    let saved_label = root.root.label.clone().unwrap_or_default();
    let saved_template = root.folder_template.clone();
    let mut label = use_signal(|| saved_label.clone());
    let mut template = use_signal(|| saved_template.clone());
    let dirty = label() != saved_label || template() != saved_template;
    let id = root.root.id.clone();
    let save = use_callback(move |()| {
        let storage = storage.clone();
        let id = id.clone();
        let name = Some(label.peek().clone()).filter(|name| !name.trim().is_empty());
        let folder = template.peek().clone();
        Box::pin(async move {
            match change(async move { storage.edit(&id, name, folder).await }).await {
                Ok(_) => {
                    on_saved.call(());
                    true
                }
                Err(error) => {
                    // The failure shows with the location's other operations.
                    on_failed.call(error);
                    false
                }
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = bool>>>
    });
    let discard = {
        let saved_label = saved_label.clone();
        let saved_template = saved_template.clone();
        use_callback(move |()| {
            label.set(saved_label.clone());
            template.set(saved_template.clone());
        })
    };
    use_unsaved_changes(
        dirty,
        UnsavedForm {
            label: root
                .root
                .label
                .clone()
                .unwrap_or_else(|| t!("Selected location")),
            save: std::rc::Rc::new(move || save(())),
            discard: std::rc::Rc::new(move || discard(())),
        },
    );
    rsx! {
        div { class: "mt-3 grid items-end gap-3 md:grid-cols-3",
            TextField {
                label: t!("Location label"),
                value: label(),
                on_input: move |next: String| label.set(next),
            }
            TextField {
                label: t!("Folder-name template"),
                description: t!(
                    note = "{title} is shown as is: the template's placeholder for the game's title",
                    "A single folder name containing {title}.",
                    title = "{title}"
                ),
                value: template(),
                on_input: move |next: String| template.set(next),
            }
            div { class: "flex flex-col items-start gap-1",
                if dirty {
                    UnsavedHint { dirty }
                }
                Button {
                    disabled,
                    on_press: move |_| {
                        spawn(async move {
                            save(()).await;
                        });
                    },
                    {t!("Save location")}
                }
            }
        }
    }
}

/// What removing a location does (`StorageService::remove_keep_files`), and
/// moving its games first.
#[component]
fn RemoveRoot(
    root: RootSummary,
    destinations: Vec<RootSummary>,
    destination: Option<RootSummary>,
    working: bool,
    on_target: EventHandler<String>,
    on_cancel: EventHandler<()>,
    on_keep_files: EventHandler<()>,
    on_move: EventHandler<()>,
) -> Element {
    let games = root.games.len();
    let options: Vec<SelectOption> = destinations
        .iter()
        .map(|to| {
            SelectOption::new(
                to.root.id.clone(),
                t!(
                    note = "A storage location to move games to, and its free space",
                    "{location} · {free} free",
                    location = location_name(to),
                    free = size(to.capabilities.free_bytes)
                ),
            )
        })
        .collect();
    let blocked = root.games.iter().any(|game| game.move_blocked.is_some());
    rsx! {
        section {
            "data-part": "remove-root",
            class: "my-3 flex flex-col gap-2 rounded-panel border border-danger/40 p-3",
            if games == 0 {
                p { class: "text-sm",
                    {t!("Remove this location from the library? Files on disk are kept.")}
                }
            } else {
                p { class: "text-sm",
                    {tn!(
                        "Remove this location from the library? Its game leaves your library, with its play time, favourite and collections. If you also have that game another way, it stays, without this copy's play time.",
                        "Remove this location from the library? Its # games leave your library, with their play time, favourites and collections. A game you also have another way stays, without this copy's play time.",
                        games
                    )}
                }
                p { class: "text-sm",
                    {t!("Files on disk are kept. Add this folder again and Caskade finds them, as new games: what left the library doesn't come back.")}
                }
            }
            Button {
                variant: ButtonVariant::Danger,
                disabled: working,
                on_press: move |_| on_keep_files.call(()),
                {t!("Keep files and remove from library")}
            }
            if games > 0 && !destinations.is_empty() {
                div { class: "flex flex-wrap items-end gap-2",
                    Select {
                        label: t!("Move games to"),
                        options,
                        value: destination.as_ref().map(|to| to.root.id.clone()),
                        disabled: working,
                        on_change: move |id: String| on_target.call(id),
                    }
                    Button {
                        disabled: working || blocked,
                        on_press: move |_| on_move.call(()),
                        {tn!("Move its game there, then remove…", "Move its # games there, then remove…", games)}
                    }
                }
            }
            Button { on_press: move |_| on_cancel.call(()), {t!("Cancel")} }
        }
    }
}

/// Asks once before a location's games move and it goes (#64).
#[component]
fn MoveAndRemoveDialog(
    root: RootSummary,
    to: RootSummary,
    on_confirm: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let from = location_name(&root);
    let target = location_name(&to);
    rsx! {
        ConfirmDialog {
            title: tn!(
                "Move its game to {target}?",
                "Move its # games to {target}?",
                root.games.len(),
                target = target.clone()
            ),
            confirm_label: t!("Move and remove"),
            on_confirm: move |_| on_confirm.call(()),
            on_close: move |_| on_close.call(()),
            p {
                {t!(
                    "{size} to move, as one job you can follow or cancel in Activity.",
                    size = size(Some(root.games_bytes))
                )}
            }
            p {
                {t!(
                    "{from} is removed once every game has moved. If you cancel, or a game can't move, it stays and the games already moved stay on {target}.",
                    from = from.clone(),
                    target = target.clone()
                )}
            }
        }
    }
}

//! Storage flows, as the webview's `components/storage`: choosing where an
//! install goes (`InstallRootPicker`), moving a game's files to another
//! location (`MoveGameAction`/`MoveGameDialog`) and the default location's
//! one-line summary (`StorageSummary`). The rules that block a location are
//! `api/storage.ts`'s, word for word; the core checks them again.

use caskade_core::storage::{RootStatus, RootSummary, StorageGame};
use caskade_ui_data::storage::{MoveError, Storage};
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::prelude::*;

use crate::data::change;
use crate::ui::{Button, Dialog, RadioGroup, RadioOption, Select, SelectOption};

use super::paths::format_path;

/// What an install needs of its location.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Needs {
    pub estimated_bytes: Option<u64>,
    pub executable: bool,
    pub symlinks: bool,
}

/// Why `root` can't take an install with `needs`; `None` when it can.
pub fn block_reason(root: &RootSummary, needs: &Needs) -> Option<String> {
    if root.status != RootStatus::Online {
        return Some(if root.status == RootStatus::ReadOnly {
            t!("Read-only location")
        } else {
            t!("Location unavailable")
        });
    }
    if !root.capabilities.writable {
        return Some(t!("Read-only location"));
    }
    if needs.executable && !root.capabilities.executable {
        return Some(t!("Cannot run executables here"));
    }
    if needs.symlinks && !root.capabilities.symlinks {
        return Some(t!("This game needs symbolic links"));
    }
    if let (Some(wanted), Some(free)) = (needs.estimated_bytes, root.capabilities.free_bytes)
        && free < wanted
    {
        return Some(t!("Not enough free space"));
    }
    None
}

/// The location an install starts on: the default, else the first that
/// can take it.
pub fn initial_root(roots: &[RootSummary], needs: &Needs) -> Option<String> {
    let usable: Vec<&RootSummary> = roots
        .iter()
        .filter(|root| block_reason(root, needs).is_none())
        .collect();
    usable
        .iter()
        .find(|root| root.root.is_default)
        .or(usable.first())
        .map(|root| root.root.id.clone())
}

/// The location to use without asking: the only one, or the default when
/// installs always go there; `None` when the user chooses.
pub fn skip_picker(
    roots: &[RootSummary],
    always_use_default: bool,
    needs: &Needs,
) -> Option<String> {
    let selected = if roots.len() == 1 {
        roots.first()
    } else if always_use_default {
        roots.iter().find(|root| root.root.is_default)
    } else {
        None
    };
    selected
        .filter(|root| block_reason(root, needs).is_none())
        .map(|root| root.root.id.clone())
}

/// The location's name: its label, else its path.
fn location_name(root: &RootSummary) -> String {
    root.root
        .label
        .clone()
        .unwrap_or_else(|| format_path(&root.root.path.to_string_lossy()))
}

/// The app's storage for a flow: a test's or a host's, else the running
/// app's; `None` where it has none.
pub fn use_storage() -> Option<Storage> {
    try_use_context::<Storage>().or_else(|| crate::shell::services().and_then(Storage::of))
}

/// Chooses where an install goes, as the webview's `InstallRootPicker`.
/// It chooses by itself when there's one location, or installs always go
/// to the default.
#[component]
pub fn InstallRootPicker(
    storage: Storage,
    #[props(default)] needs: Needs,
    on_select: EventHandler<String>,
    on_cancel: EventHandler<()>,
    /// The "always use the default" preference; read from settings
    /// without it.
    #[props(default)]
    always_use_default: Option<bool>,
    /// Its own "Install location" heading; off inside a dialog titled so.
    #[props(default = true)]
    heading: bool,
) -> Element {
    let loaded = {
        let storage = storage.clone();
        use_resource(move || {
            let storage = storage.clone();
            async move {
                change(async move {
                    let roots = storage.roots().await?;
                    let preference = match always_use_default {
                        Some(preference) => preference,
                        None => storage.always_use_default().await?,
                    };
                    Ok((roots, preference))
                })
                .await
            }
        })
    };
    let mut selected = use_signal(|| None::<String>);
    let mut auto_selected = use_signal(|| false);
    let state = loaded.read().clone();
    if let Some(Ok((roots, preference))) = &state
        && !auto_selected()
        && let Some(skip) = skip_picker(roots, *preference, &needs)
    {
        auto_selected.set(true);
        on_select.call(skip);
    }
    let mut retry = loaded;
    match state {
        Some(Err(_)) => rsx! {
            section {
                "data-cs": "install-root-picker",
                role: "alert",
                class: "rounded-panel border border-danger/40 p-4",
                p { {t!("Storage locations could not be loaded.")} }
                Button { id: "root-retry", on_press: move |_| retry.restart(), {t!("Try again")} }
                Button { id: "root-cancel", on_press: move |_| on_cancel.call(()), {t!("Cancel")} }
            }
        },
        None => rsx! {
            p { role: "status", {t!("Checking storage locations…")} }
        },
        Some(Ok((roots, _))) => {
            let value = selected().or_else(|| initial_root(&roots, &needs));
            let current = roots
                .iter()
                .find(|root| Some(&root.root.id) == value.as_ref());
            let blocked = match current {
                Some(root) => block_reason(root, &needs),
                None => Some(t!("Add a storage location in Settings → Storage.")),
            };
            let estimate = match needs.estimated_bytes {
                None => t!(
                    "Estimated space: Unknown — free space will be checked again before installation."
                ),
                Some(bytes) => t!("Estimated space: {size}", size = format::bytes(Some(bytes))),
            };
            let options: Vec<RadioOption> = roots
                .iter()
                .map(|root| {
                    let reason = block_reason(root, &needs);
                    let mut label = location_name(root);
                    if root.root.is_default {
                        label = format!("{label} · {}", tp!("storage location", "Default"));
                    }
                    let mut free = t!(
                        "{free} free",
                        free = format::bytes(root.capabilities.free_bytes)
                    );
                    if let Some(reason) = &reason {
                        free = format!("{free} · {reason}");
                    }
                    RadioOption {
                        description: Some(format!(
                            "{} · {free}",
                            format_path(&root.root.path.to_string_lossy())
                        )),
                        disabled: reason.is_some(),
                        ..RadioOption::new(root.root.id.clone(), label)
                    }
                })
                .collect();
            let chosen = value.clone();
            rsx! {
                section {
                    "data-cs": "install-root-picker",
                    class: "flex flex-col gap-4 rounded-panel border border-border p-4",
                    div {
                        if heading {
                            h2 { class: "font-display text-lg font-semibold", {t!("Install location")} }
                        }
                        p { class: "text-sm text-fg-muted", "{estimate}" }
                    }
                    RadioGroup {
                        id: "root-options",
                        label: t!("Storage location"),
                        options,
                        value: value.clone(),
                        on_change: move |id| selected.set(Some(id)),
                    }
                    if let Some(blocked) = blocked.clone() {
                        p { role: "alert", class: "text-sm text-warning", "{blocked}" }
                    }
                    div { class: "flex justify-end gap-2",
                        Button { id: "root-cancel", on_press: move |_| on_cancel.call(()), {t!("Cancel")} }
                        Button {
                            id: "root-install",
                            disabled: blocked.is_some() || chosen.is_none(),
                            on_press: move |_| {
                                if let (Some(id), None) = (chosen.clone(), blocked.as_ref()) {
                                    on_select.call(id);
                                }
                            },
                            {t!("Install here")}
                        }
                    }
                }
            }
        }
    }
}

/// The game page's button for [`MoveGameDialog`].
#[component]
pub fn MoveGameAction(
    game_id: String,
    #[props(default)] entry_id: Option<String>,
    #[props(default)] is_running: bool,
) -> Element {
    let storage = use_storage();
    let mut open = use_signal(|| false);
    rsx! {
        Button {
            id: "move-game",
            disabled: storage.is_none() || is_running,
            on_press: move |_| open.set(true),
            {t!("Move installed files…")}
        }
        if let (true, Some(storage)) = (open(), storage) {
            MoveGameDialog {
                storage,
                game_id,
                entry_id,
                is_running,
                on_close: move |_| open.set(false),
            }
        }
    }
}

/// Moves a game's installed files to another location, as the webview's
/// `MoveGameDialog`. Games owned by other launchers, delegated or running
/// get the reason they can't.
#[component]
pub fn MoveGameDialog(
    storage: Storage,
    game_id: String,
    #[props(default)] entry_id: Option<String>,
    #[props(default)] is_running: bool,
    on_close: EventHandler<()>,
) -> Element {
    let roots = {
        let storage = storage.clone();
        use_resource(move || {
            let storage = storage.clone();
            async move { change(async move { storage.roots().await }).await }
        })
    };
    let mut entry = use_signal(String::new);
    let mut target = use_signal(String::new);
    let mut moving = use_signal(|| false);
    let mut failure = use_signal(|| None::<String>);

    let state = roots.read().clone();
    let all = match &state {
        Some(Ok(roots)) => roots.clone(),
        _ => Vec::new(),
    };
    let candidates: Vec<(RootSummary, StorageGame)> = all
        .iter()
        .flat_map(|root| {
            root.games
                .iter()
                .filter(|game| {
                    game.game_id == game_id
                        && entry_id
                            .as_ref()
                            .is_none_or(|entry| &game.entry_id == entry)
                })
                .map(|game| (root.clone(), game.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    let wanted = entry_id.clone().unwrap_or_else(|| entry.read().clone());
    let selected = candidates
        .iter()
        .find(|(_, game)| game.entry_id == wanted)
        .or(candidates.first())
        .cloned();
    let needs = Needs {
        estimated_bytes: selected.as_ref().map(|(_, game)| game.size_bytes),
        executable: true,
        symlinks: false,
    };
    let targets: Vec<RootSummary> = all
        .iter()
        .filter(|root| {
            selected
                .as_ref()
                .is_none_or(|(from, _)| from.root.id != root.root.id)
        })
        .cloned()
        .collect();
    let destination = targets
        .iter()
        .find(|root| root.root.id == target())
        .or_else(|| {
            targets
                .iter()
                .find(|root| block_reason(root, &needs).is_none())
        })
        .cloned();
    let reason = if is_running {
        Some(t!("Quit this game before moving its files."))
    } else {
        match &selected {
            None => Some(t!("Only Caskade-managed installations can be moved.")),
            Some((_, game)) if game.move_blocked.is_some() => game.move_blocked.clone(),
            Some(_) => match &destination {
                Some(root) => block_reason(root, &needs),
                None => Some(t!(
                    "Add another eligible storage location in Settings → Storage."
                )),
            },
        }
    };
    let busy = moving();
    let start = {
        let storage = storage.clone();
        let selected = selected.clone();
        let destination = destination.clone();
        let reason = reason.clone();
        move |_| {
            let (Some((_, game)), Some(root), None) = (&selected, &destination, &reason) else {
                return;
            };
            let storage = storage.clone();
            let entry = game.entry_id.clone();
            let root = root.root.id.clone();
            moving.set(true);
            failure.set(None);
            spawn(async move {
                let moved =
                    caskade_app::runtime::spawn(
                        async move { storage.move_game(entry, root).await },
                    )
                    .await
                    .unwrap_or_else(|error| Err(MoveError::Failed(error.to_string())));
                moving.set(false);
                match moved {
                    Ok(()) => on_close.call(()),
                    Err(MoveError::Unavailable) => {
                        failure.set(Some(t!("Storage service is unavailable")))
                    }
                    Err(MoveError::NeedsUser) => failure.set(Some(t!(
                        "This job needs a response in Activity before it can continue."
                    ))),
                    Err(MoveError::Failed(message)) => failure.set(Some(message)),
                }
            });
        }
    };
    let close = move |_| {
        if !busy {
            on_close.call(());
        }
    };
    let pending = state.is_none();
    let errored = matches!(state, Some(Err(_)));
    rsx! {
        Dialog {
            title: t!("Move installed files"),
            on_close: (!busy).then_some(EventHandler::new(close)),
            class: "max-w-lg",
            if pending {
                p { role: "status", {t!("Checking locations…")} }
            } else if errored {
                p { role: "alert", {t!("Locations could not be loaded.")} }
            } else {
                if candidates.len() > 1 {
                    Select {
                        id: "move-installation",
                        label: t!("Installation"),
                        options: candidates
                            .iter()
                            .map(|(root, game)| SelectOption::new(
                                game.entry_id.clone(),
                                format!("{} · {}", location_name(root), game.relative_dir.display()),
                            ))
                            .collect::<Vec<_>>(),
                        value: selected.as_ref().map(|(_, game)| game.entry_id.clone()),
                        on_change: move |id| entry.set(id),
                    }
                }
                p { class: "text-sm text-fg-muted",
                    match &selected {
                        Some((_, game)) => t!(
                            "{title} · {size}. Playtime and library identity are kept.",
                            title = game.title.clone(),
                            size = format::bytes(Some(game.size_bytes))
                        ),
                        None => t!("Games owned by other launchers stay in their launcher locations."),
                    }
                }
                Select {
                    id: "move-destination",
                    label: t!("Destination"),
                    options: targets
                        .iter()
                        .map(|root| SelectOption::new(
                            root.root.id.clone(),
                            t!(
                                "{location} · {free} free",
                                location = location_name(root),
                                free = format::bytes(root.capabilities.free_bytes)
                            ),
                        ))
                        .collect::<Vec<_>>(),
                    value: destination.as_ref().map(|root| root.root.id.clone()),
                    on_change: move |id| target.set(id),
                    disabled: targets.is_empty() || busy,
                }
                if let Some(reason) = reason.clone() {
                    p { role: "alert", "data-part": "reason", class: "text-sm text-warning", "{reason}" }
                }
                p { class: "text-xs text-fg-muted",
                    {t!("The move appears in Activity, where it can be cancelled.")}
                }
            }
            if let Some(message) = failure() {
                p { role: "alert", "data-part": "error", class: "text-sm text-danger", "{message}" }
            }
            div { class: "flex justify-end gap-2",
                Button { id: "move-cancel", disabled: busy, on_press: close, {t!("Cancel")} }
                Button {
                    id: "move-confirm",
                    disabled: pending || errored || reason.is_some() || busy,
                    on_press: start,
                    if busy { {t!("Moving…")} } else { {t!("Move game")} }
                }
            }
        }
    }
}

/// The default location and its state, in a line (the status bar's), as
/// the webview's `StorageSummary`.
#[component]
pub fn StorageSummary() -> Element {
    let storage = use_storage();
    let roots = use_resource(move || {
        let storage = storage.clone();
        async move {
            match storage {
                Some(storage) => Some(change(async move { storage.roots().await }).await),
                None => None,
            }
        }
    });
    let text = match roots.read().clone() {
        Some(Some(Err(_))) => t!("Locations unavailable"),
        Some(Some(Ok(roots))) => match roots.iter().find(|root| root.root.is_default) {
            Some(root) => format!(
                "{} · {}",
                format_path(&root.root.path.to_string_lossy()),
                match root.status {
                    RootStatus::Online => tp!("storage location status", "online"),
                    RootStatus::Offline => tp!("storage location status", "offline"),
                    RootStatus::ReadOnly => tp!("storage location status", "read only"),
                    RootStatus::IdentityMismatch => {
                        tp!("storage location status", "identity mismatch")
                    }
                }
            ),
            None => t!("No default location configured"),
        },
        None => t!("Checking locations…"),
        Some(None) => t!("No default location configured"),
    };
    rsx! {
        span { class: "truncate font-mono text-xs text-fg-subtle", "{text}" }
    }
}

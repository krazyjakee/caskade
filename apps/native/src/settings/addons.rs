//! Settings → Plugins' catalogue (#104, moved from #108), as the webview's
//! `AddonCatalogue`, `AddonDetails` and `DeviceOffers`: curated plugins and
//! themes with what each means here, searchable and by kind, read offline
//! from the last fetch; each opens its details, where it installs, updates,
//! is skipped or removed through the shared package service. A first
//! plugin install goes through the plugin review; an installed theme can
//! be put to use. The catalogue is the window's mode's: console mode's
//! releases while it's on (#107).

use caskade_app::commands::addon_offers::DeviceAddon;
use caskade_app::commands::addon_packages::{AddonOutcome, AddonPhase, AddonProgress};
use caskade_app::events::Event;
use caskade_ui_data::ErrorReport;
use caskade_ui_data::addons::{
    AddonKind, AddonRecord, AppMode, CatalogueItem, ItemState, UpdatePreference,
};
use caskade_ui_data::plugins::{LibraryAccess, PluginSummary};
use caskade_ui_i18n::{format, t, tn, tp};
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;
use serde_json::Value;

use super::plugins::ReviewDialog;
use super::state::use_settings_section;
use crate::data::{DataSource, change, use_addons, use_addons_query};
use crate::ui::icons::{CLOUD_OFF, IMAGE_OFF, MONITOR_SMARTPHONE, PACKAGE, REFRESH_CW, SEARCH};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Checkbox, Dialog, Segment, SegmentedControl,
    SettingsSection, Switch, TextField,
};

pub const ANATOMY: [Entry; 2] = [
    Entry {
        parts: &[
            (
                "refresh",
                "Check for updates: fetches the catalogue again (a `button`).",
            ),
            (
                "status",
                "When the catalogue was fetched, offline or why the last check failed.",
            ),
            ("updates", "How many updates are waiting."),
            (
                "update-preference",
                "Whether to check for updates when Settings opens (a `switch`).",
            ),
            ("item", "One add-on (`data-kind`: `plugin` or `theme`)."),
            (
                "state",
                "What the add-on means here (`data-state`: `available`, `installed`, `updateAvailable` or `incompatible`).",
            ),
            ("details", "Opens the add-on's details (a `button`)."),
        ],
        contains: &["button", "switch", "text-field", "segmented"],
        ..Entry::new(
            "addon-catalogue",
            "AddonCatalogue (apps/native/src/settings/addons.rs)",
            Source::App,
            "Settings → Plugins' catalogue of curated plugins and themes: what each means here, searchable and by kind, from the last fetch when offline.",
        )
    },
    Entry {
        parts: &[
            ("screenshots", "The catalogue's screenshots."),
            (
                "screenshot",
                "One screenshot, a thumbnail the app re-encoded.",
            ),
            (
                "screenshot-missing",
                "In place of a screenshot that couldn't load.",
            ),
            ("description", "The description, always shown as text."),
            (
                "reasons",
                "Why the add-on doesn't fit here, or what it lacks.",
            ),
            (
                "release",
                "The release an install gets: version, date, OSes, modes and permissions.",
            ),
            (
                "state",
                "What the add-on means here (`data-state`: `available`, `installed`, `updateAvailable` or `incompatible`).",
            ),
            ("progress", "Downloading, checking, installing or starting."),
            (
                "outcome",
                "What the last install did (`data-outcome`: `installed` or `failed`), or a release refused before.",
            ),
            (
                "remove-confirm",
                "Asks before removing; for a plugin, whether to keep its settings and data.",
            ),
            ("remove-confirmed", "Removes it (a `button`)."),
            ("actions", "The add-on's buttons."),
            ("remove", "Uninstall… (a `button`)."),
            (
                "skip",
                "Skips this update, or offers it again (a `button`).",
            ),
            (
                "apply",
                "Makes an installed theme the one in use (a `button`).",
            ),
            ("install", "Install, Update or Try again (a `button`)."),
        ],
        states: &[
            ("data-kind", &["plugin", "theme"]),
            (
                "data-state",
                &["available", "installed", "updateAvailable", "incompatible"],
            ),
        ],
        contains: &["button", "checkbox"],
        ..Entry::new(
            "addon-details",
            "AddonDetails (apps/native/src/settings/addons.rs)",
            Source::App,
            "A catalogue add-on's details: screenshots, description as text, compatibility, the release an install gets, and installing, updating, applying and removing it.",
        )
    },
];

/// The catalogue is fetched again on opening when older than this.
const STALE_MS: i64 = 24 * 3_600_000;

/// The mode the catalogue's releases are for: the window's (#107).
fn mode() -> AppMode {
    if crate::console::active() {
        AppMode::Console
    } else {
        AppMode::Desktop
    }
}

fn when(ms: i64) -> String {
    format::date_time(crate::library::local_date_time(ms))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn kind_name(kind: AddonKind) -> String {
    match kind {
        AddonKind::Plugin => t!("Plugin"),
        AddonKind::Theme => t!("Theme"),
    }
}

fn state_name(state: ItemState) -> &'static str {
    match state {
        ItemState::Available => "available",
        ItemState::Installed => "installed",
        ItemState::UpdateAvailable => "updateAvailable",
        ItemState::Incompatible => "incompatible",
    }
}

/// A badge for what an entry means here.
#[component]
pub fn StateBadge(item: CatalogueItem) -> Element {
    let compatibility = &item.compatibility;
    let (tone, label) = match compatibility.state {
        ItemState::Available => ("border-border-strong text-fg-muted", t!("Available")),
        ItemState::Installed => (
            "border-success/50 text-success",
            match &compatibility.installed_version {
                Some(version) => t!(
                    "Installed {installedVersion}",
                    installedVersion = version.clone()
                ),
                None => t!("Installed"),
            },
        ),
        ItemState::UpdateAvailable => (
            "border-accent-alt/60 text-accent-alt",
            t!(
                "Update to {0}",
                0 = compatibility
                    .release
                    .as_ref()
                    .map(|release| release.version.clone())
                    .unwrap_or_default()
            ),
        ),
        ItemState::Incompatible => ("border-warning/50 text-warning", t!("Not compatible")),
    };
    rsx! {
        span {
            "data-part": "state",
            "data-state": state_name(compatibility.state),
            class: "shrink-0 rounded-control border px-2 py-0.5 text-xs {tone}",
            "{label}"
        }
    }
}

#[component]
pub fn AddonCatalogue() -> Element {
    let addons = use_addons();
    let mut catalogue = use_addons_query(|addons| async move { addons.catalogue(mode()).await });
    let mut records = use_addons_query(|addons| async move { addons.records().await });
    let mut query = use_signal(String::new);
    let mut kind = use_signal(|| "all".to_owned());
    let mut open = use_signal(|| None::<String>);
    let mut refreshing = use_signal(|| false);
    let mut refresh_error = use_signal(|| None::<ErrorReport>);
    let mut preference_error = use_signal(|| None::<ErrorReport>);
    let mut preference_pending = use_signal(|| None::<bool>);
    let refresh = use_callback({
        let addons = addons.clone();
        move |()| {
            let addons = addons.clone();
            refreshing.set(true);
            refresh_error.set(None);
            spawn(async move {
                let outcome = change(async move { addons.refresh(mode()).await }).await;
                refreshing.set(false);
                if let Err(error) = outcome {
                    refresh_error.set(Some(error));
                }
                catalogue.restart();
            });
        }
    });
    let set_preference = {
        let addons = addons.clone();
        move |notify: bool| {
            let addons = addons.clone();
            preference_pending.set(Some(notify));
            preference_error.set(None);
            spawn(async move {
                let preference = if notify {
                    UpdatePreference::Notify
                } else {
                    UpdatePreference::Manual
                };
                let outcome =
                    change(async move { addons.set_update_preference(preference).await }).await;
                preference_pending.set(None);
                if let Err(error) = outcome {
                    preference_error.set(Some(error));
                }
                records.restart();
            });
        }
    };
    let view = match &*catalogue.read() {
        Some(Ok(view)) => Some(view.clone()),
        _ => None,
    };
    let load_error = match &*catalogue.read() {
        Some(Err(error)) => Some(error.clone()),
        _ => None,
    };
    let records_now = match &*records.read() {
        Some(Ok(records)) => Some(records.clone()),
        _ => None,
    };
    // Checks for updates once when Settings opens, unless the user said not
    // to.
    let mut checked = use_signal(|| false);
    let due = match (&view, &records_now) {
        (Some(view), Some(records)) => {
            records.updates == UpdatePreference::Notify
                && view
                    .status
                    .fetched_at_ms
                    .is_none_or(|fetched| now_ms() - fetched > STALE_MS)
        }
        _ => false,
    };
    let ready = view.is_some() && records_now.is_some();
    use_effect(use_reactive!(|ready, due| {
        if ready && !*checked.peek() {
            checked.set(true);
            if due {
                refresh(());
            }
        }
    }));
    let words = query().trim().to_lowercase();
    let all_items = view
        .as_ref()
        .map(|view| view.items.clone())
        .unwrap_or_default();
    let items: Vec<CatalogueItem> = all_items
        .iter()
        .filter(|item| {
            let entry = &item.entry;
            let kind_ok = match kind().as_str() {
                "plugin" => entry.kind == AddonKind::Plugin,
                "theme" => entry.kind == AddonKind::Theme,
                _ => true,
            };
            kind_ok
                && (words.is_empty()
                    || [&entry.name, &entry.summary, &entry.author]
                        .into_iter()
                        .chain(entry.categories.iter())
                        .any(|text| text.to_lowercase().contains(&words)))
        })
        .cloned()
        .collect();
    let updates = all_items
        .iter()
        .filter(|item| {
            item.compatibility.state == ItemState::UpdateAvailable
                && records_now
                    .as_ref()
                    .and_then(|records| records.addons.get(&item.entry.id))
                    .and_then(|record| record.skipped.clone())
                    != item
                        .compatibility
                        .release
                        .as_ref()
                        .map(|release| release.version.clone())
        })
        .count();
    let status_text = match view.as_ref().map(|view| &view.status) {
        None => t!("Loading the catalogue…"),
        Some(status) => match (status.fetched_at_ms, &status.error, status.offline) {
            (None, Some(error), _) => {
                t!("The catalogue couldn't be fetched: {0}", 0 = error.clone())
            }
            (None, None, _) => t!("The catalogue hasn't been fetched yet."),
            (Some(at), _, true) => t!(
                "Offline: showing the catalogue from {0}. Installing needs a connection.",
                0 = when(at)
            ),
            (Some(at), Some(error), false) => t!(
                "Showing the catalogue from {0}; the last check failed: {1}",
                0 = when(at),
                1 = error.clone()
            ),
            (Some(at), None, false) => t!("Updated {0}.", 0 = when(at)),
        },
    };
    let offline = view.as_ref().is_some_and(|view| view.status.offline);
    let selected = view
        .as_ref()
        .and_then(|view| {
            view.items
                .iter()
                .find(|item| Some(&item.entry.id) == open().as_ref())
        })
        .cloned();
    let kinds = vec![
        Segment::new("all", tp!("add-on kind", "All")),
        Segment::new("plugin", t!("Plugins")),
        Segment::new("theme", t!("Themes")),
    ];
    let error = load_error.or(refresh_error());
    rsx! {
        SettingsSection {
            icon: Some(PACKAGE),
            title: t!("Plugin and theme catalogue"),
            description: t!("Curated plugins and themes. Each says whether it fits this device."),
            div { "data-cs": "addon-catalogue", class: "mt-3 flex flex-col gap-3",
                div { class: "flex flex-wrap items-center justify-between gap-3",
                    p {
                        "data-part": "status",
                        role: "status",
                        class: "flex min-w-0 flex-[1_1_16rem] items-center gap-2 text-sm text-fg-muted",
                        if offline {
                            crate::ui::Svg { icon: CLOUD_OFF, class: "size-4" }
                        }
                        "{status_text}"
                        if updates > 0 {
                            span { "data-part": "updates", class: "text-accent-alt",
                                " "
                                {tn!("# update", "# updates", updates)}
                            }
                        }
                    }
                    Button {
                        "data-part": "refresh",
                        size: ButtonSize::Sm,
                        icon: Some(REFRESH_CW),
                        disabled: refreshing(),
                        on_press: move |_| refresh(()),
                        if refreshing() {
                            {t!("Checking…")}
                        } else {
                            {t!("Check for updates")}
                        }
                    }
                }
                if let Some(records) = records_now.clone() {
                    div { "data-part": "update-preference", class: "flex items-center gap-3",
                        Switch {
                            "aria-label": t!("Check for add-on updates when Settings opens"),
                            checked: preference_pending().unwrap_or(records.updates == UpdatePreference::Notify),
                            disabled: preference_pending().is_some(),
                            on_change: set_preference,
                        }
                        span { class: "text-sm", {t!("Check for add-on updates when Settings opens")} }
                    }
                }
                if let Some(error) = preference_error() {
                    crate::ui::FailureNotice { error, variant: crate::ui::NoticeVariant::Inline }
                }
                DeviceOffers { items: all_items.clone(), on_open: move |id: String| open.set(Some(id)) }
                div { class: "flex flex-wrap items-center gap-3",
                    div { class: "min-w-56 flex-1",
                        TextField {
                            "aria-label": t!("Search the catalogue"),
                            placeholder: t!("Search plugins and themes"),
                            icon: Some(SEARCH),
                            value: query(),
                            on_input: move |next: String| query.set(next),
                        }
                    }
                    SegmentedControl {
                        "aria-label": t!("Show"),
                        options: kinds,
                        value: kind(),
                        on_change: move |next: String| kind.set(next),
                    }
                }
                if let Some(error) = error {
                    p { role: "alert", class: "text-sm text-danger", "{error.summary}" }
                }
                if view.is_some() && items.is_empty() {
                    p { class: "rounded-control border border-dashed border-border p-5 text-sm text-fg-muted",
                        if all_items.is_empty() {
                            {t!("The catalogue is empty.")}
                        } else {
                            {t!("Nothing in the catalogue matches.")}
                        }
                    }
                }
                if !items.is_empty() {
                    ul { "aria-label": t!("Catalogue"), class: "grid gap-3 md:grid-cols-2",
                        for item in items {
                            Row {
                                key: "{item.entry.id}",
                                on_open: {
                                    let id = item.entry.id.clone();
                                    move |_| open.set(Some(id.clone()))
                                },
                                item,
                            }
                        }
                    }
                }
                if let Some(item) = selected {
                    AddonDetails {
                        key: "{item.entry.id}",
                        record: records_now
                            .as_ref()
                            .and_then(|records| records.addons.get(&item.entry.id).cloned()),
                        item,
                        offline,
                        on_changed: move |_| {
                            catalogue.restart();
                            records.restart();
                        },
                        on_close: move |_| open.set(None),
                    }
                }
            }
        }
    }
}

#[component]
fn Row(item: CatalogueItem, on_open: EventHandler<()>) -> Element {
    let entry = item.entry.clone();
    rsx! {
        li {
            "data-part": "item",
            "data-kind": if entry.kind == AddonKind::Plugin { "plugin" } else { "theme" },
            class: "flex flex-col gap-2 rounded-control border border-border bg-surface-base/60 p-4",
            div { class: "flex items-start gap-2",
                div { class: "min-w-0 flex-1",
                    p { class: "truncate font-semibold", "{entry.name}" }
                    p { class: "text-xs text-fg-muted", "{kind_name(entry.kind)} · {entry.author}" }
                }
                StateBadge { item: item.clone() }
            }
            p { class: "text-sm text-fg-muted", "{entry.summary}" }
            div {
                Button {
                    "data-part": "details",
                    "data-spatial": "focusable",
                    size: ButtonSize::Sm,
                    "aria-label": t!("Details of {0}", 0 = entry.name.clone()),
                    on_press: move |_| on_open.call(()),
                    {t!("Details…")}
                }
            }
        }
    }
}

/// Add-ons on the user's other devices (#245): what this device could have
/// too. Nothing installs by itself; each opens its details.
#[component]
fn DeviceOffers(items: Vec<CatalogueItem>, on_open: EventHandler<String>) -> Element {
    let offers = use_addons_query(|addons| async move { addons.from_devices().await });
    let read: Vec<DeviceAddon> = match &*offers.read() {
        Some(Ok(offers)) => offers.clone(),
        _ => Vec::new(),
    };
    // One row per add-on, with every device that has it.
    let mut by_addon: Vec<(String, Vec<DeviceAddon>)> = Vec::new();
    for offer in read {
        match by_addon.iter_mut().find(|(id, _)| *id == offer.addon_id) {
            Some((_, from)) => from.push(offer),
            None => by_addon.push((offer.addon_id.clone(), vec![offer])),
        }
    }
    let rows: Vec<(String, Vec<DeviceAddon>, Option<CatalogueItem>)> = by_addon
        .into_iter()
        .map(|(id, from)| {
            let item = items.iter().find(|item| item.entry.id == id).cloned();
            (id, from, item)
        })
        // Installed here since the devices last synced: nothing to offer.
        .filter(|(_, _, item)| {
            item.as_ref().is_none_or(|item| {
                !matches!(
                    item.compatibility.state,
                    ItemState::Installed | ItemState::UpdateAvailable
                )
            })
        })
        .collect();
    if rows.is_empty() {
        return rsx! {};
    }
    rsx! {
        section {
            "data-part": "from-devices",
            "aria-labelledby": "addons-from-devices",
            class: "flex flex-col gap-2 rounded-control border border-border bg-surface-base/60 p-4",
            h3 { id: "addons-from-devices", class: "flex items-center gap-2 font-semibold",
                crate::ui::Svg { icon: MONITOR_SMARTPHONE, class: "size-4 text-fg-muted" }
                {t!("On your other devices")}
            }
            p { class: "text-sm text-fg-muted",
                {t!("Your other devices have these. Nothing installs here until you choose to.")}
            }
            ul { class: "flex flex-col gap-2",
                for (id, from, item) in rows {
                    {
                        let devices = from.iter().map(|offer| offer.device_name.clone()).collect::<Vec<_>>().join(", ");
                        let name = item.as_ref().map(|item| item.entry.name.clone()).unwrap_or_else(|| id.clone());
                        rsx! {
                            li {
                                key: "{id}",
                                "data-part": "device-addon",
                                class: "flex flex-wrap items-center justify-between gap-3",
                                div { class: "min-w-0",
                                    p { class: "text-sm font-medium", "{name}" }
                                    p { class: "text-xs text-fg-muted",
                                        if item.is_some() {
                                            {t!("On {devices}", devices = devices.clone())}
                                        } else {
                                            {t!("On {devices}. It isn't in the catalogue, so install it there the way it came.", devices = devices.clone())}
                                        }
                                    }
                                }
                                if let Some(item) = item.clone() {
                                    div { class: "flex items-center gap-2",
                                        StateBadge { item }
                                        Button {
                                            "data-spatial": "focusable",
                                            size: ButtonSize::Sm,
                                            "aria-label": t!("See {name}", name = name.clone()),
                                            on_press: {
                                                let id = id.clone();
                                                move |_| on_open.call(id.clone())
                                            },
                                            {t!("See it…")}
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

/// The catalogue's permission words in the review's plain words; a word
/// this version doesn't know stays as written.
pub fn permission_lines(words: &[String]) -> Vec<String> {
    let mut network = Vec::new();
    let mut fs = Vec::new();
    let mut process = false;
    let mut library = LibraryAccess::None;
    let mut unknown = Vec::new();
    for word in words {
        match word.as_str() {
            "library.read" => library = LibraryAccess::Read,
            "library.write" => library = LibraryAccess::Write,
            "process" => process = true,
            other => match (other.strip_prefix("network:"), other.strip_prefix("fs:")) {
                (Some(host), _) => network.push(host.to_owned()),
                (_, Some(folder)) => fs.push(folder.to_owned()),
                _ => unknown.push(other.to_owned()),
            },
        }
    }
    let plugin: PluginSummary = serde_json::from_value(serde_json::json!({
        "id": "", "name": "", "version": null, "description": null, "author": null,
        "folder": "", "status": "disabled", "errors": [],
        "permissions": {"network": network, "fs": fs, "process": process, "library": library},
        "contributes": null, "hasUi": false, "hasBackend": false, "failures": 0,
        "lastError": null, "settingsError": null, "dev": false
    }))
    .expect("a plugin summary from its own fields");
    let mut lines = super::plugins::capability_lines(&plugin);
    lines.extend(unknown);
    lines
}

/// One catalogue add-on: its description (as text), screenshots,
/// releases, what it asks for and why it may not fit, and installing,
/// updating, skipping and removing it.
#[component]
fn AddonDetails(
    item: CatalogueItem,
    offline: bool,
    record: Option<AddonRecord>,
    on_changed: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let addons = use_addons();
    let source = use_context::<DataSource>();
    let entry = item.entry.clone();
    let compatibility = item.compatibility.clone();
    let release = compatibility.release.clone();
    let mut progress = use_signal(|| None::<AddonProgress>);
    let mut outcome = use_signal(|| None::<AddonOutcome>);
    let mut keep_data = use_signal(|| true);
    let mut confirm_remove = use_signal(|| false);
    let mut installing = use_signal(|| false);
    let mut removing = use_signal(|| false);
    let mut install_error = use_signal(|| None::<String>);
    let mut remove_error = use_signal(|| None::<String>);
    // Install progress for this add-on, from the app's events.
    let id = entry.id.clone();
    use_hook(move || {
        let mut events = source.events.subscribe();
        spawn(async move {
            while let Ok(event) = events.recv().await {
                if let Event::AddonProgress(next) = event
                    && next.id == id
                {
                    progress.set(Some(next));
                }
            }
        });
    });
    let install = use_callback({
        let addons = addons.clone();
        let id = entry.id.clone();
        move |(accept, retry): (bool, bool)| {
            let addons = addons.clone();
            let id = id.clone();
            installing.set(true);
            install_error.set(None);
            spawn(async move {
                let result =
                    change(async move { addons.install(&id, mode(), accept, retry).await }).await;
                installing.set(false);
                progress.set(None);
                match result {
                    Ok(result) => {
                        let review = matches!(result, AddonOutcome::NeedsReview { .. });
                        outcome.set(Some(result));
                        // A failure may be remembered as a rejected release.
                        if !review {
                            on_changed.call(());
                        }
                    }
                    Err(error) => install_error.set(Some(error.summary)),
                }
            });
        }
    });
    let remove = {
        let addons = addons.clone();
        let id = entry.id.clone();
        move |_| {
            let addons = addons.clone();
            let id = id.clone();
            let keep = *keep_data.peek();
            removing.set(true);
            remove_error.set(None);
            spawn(async move {
                let result = change(async move { addons.uninstall(&id, keep).await }).await;
                removing.set(false);
                match result {
                    Ok(()) => {
                        confirm_remove.set(false);
                        on_changed.call(());
                    }
                    Err(error) => remove_error.set(Some(error.summary)),
                }
            });
        }
    };
    let installed = matches!(
        compatibility.state,
        ItemState::Installed | ItemState::UpdateAvailable
    );
    let rejected = record
        .as_ref()
        .and_then(|record| record.rejected.clone())
        .filter(|rejected| {
            Some(&rejected.version) == release.as_ref().map(|release| &release.version)
        });
    let skipped = compatibility.state == ItemState::UpdateAvailable
        && record.as_ref().and_then(|record| record.skipped.clone())
            == release.as_ref().map(|release| release.version.clone());
    let busy = installing() || removing();
    let skip = {
        let addons = addons.clone();
        let id = entry.id.clone();
        let version = release.as_ref().map(|release| release.version.clone());
        move |_| {
            let addons = addons.clone();
            let id = id.clone();
            let next = if skipped { None } else { version.clone() };
            spawn(async move {
                if change(async move { addons.skip_update(&id, next).await })
                    .await
                    .is_ok()
                {
                    on_changed.call(());
                }
            });
        }
    };
    let review = match outcome() {
        Some(AddonOutcome::NeedsReview { preview, .. }) => Some(preview),
        _ => None,
    };
    let install_label = match (&release, compatibility.state) {
        (Some(release), ItemState::UpdateAvailable) => {
            t!("Update to {0}", 0 = release.version.clone())
        }
        _ if rejected.is_some() => t!("Try again"),
        _ => t!("Install"),
    };
    let progress_text = progress().map(|progress| match progress.phase {
        AddonPhase::Downloading => t!(
            "Downloading… {0}%",
            0 = (progress.received * 100 / progress.total.max(1))
        ),
        AddonPhase::Checking => t!("Checking the download…"),
        AddonPhase::Installing => t!("Installing…"),
        #[allow(unreachable_patterns)]
        _ => t!("Starting…"),
    });
    let modes = release.as_ref().map(|release| {
        if release.modes.len() == 1 {
            if release.modes[0] == AppMode::Desktop {
                t!("Desktop mode")
            } else {
                t!("Console mode")
            }
        } else {
            t!("Desktop and console modes")
        }
    });
    let runs_on = release.as_ref().map(|release| {
        if release.os.is_empty() {
            t!("Linux, Windows and macOS")
        } else {
            release
                .os
                .iter()
                .map(|os| {
                    match serde_json::to_value(os)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .as_deref()
                    {
                        Some("linux") => "Linux".to_owned(),
                        Some("windows") => "Windows".to_owned(),
                        Some("macos") => "macOS".to_owned(),
                        other => other.unwrap_or_default().to_owned(),
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    });
    let asks = release
        .as_ref()
        .map(|release| permission_lines(&release.permissions))
        .unwrap_or_default();
    rsx! {
        Dialog {
            title: entry.name.clone(),
            "data-cs": "addon-details",
            "data-kind": if entry.kind == AddonKind::Plugin { "plugin" } else { "theme" },
            "data-state": state_name(compatibility.state),
            class: "max-w-2xl",
            on_close: move |_| {
                if !busy {
                    on_close.call(());
                }
            },
            div { class: "flex flex-col gap-4",
                div { class: "flex flex-wrap items-start gap-3",
                    div { class: "min-w-0 flex-1",
                        p { class: "text-sm text-fg-muted", "{kind_name(entry.kind)} · {entry.author} · {entry.license}" }
                        p { class: "font-mono text-xs text-fg-muted", "{entry.id}" }
                    }
                    StateBadge { item: item.clone() }
                }
                if !entry.screenshots.is_empty() {
                    ul { "data-part": "screenshots", "aria-label": t!("Screenshots"), class: "flex gap-3 overflow-x-auto",
                        for (index, shot) in entry.screenshots.iter().enumerate() {
                            li { key: "{shot.url}", class: "shrink-0",
                                Screenshot { id: entry.id.clone(), index: index as u32, alt: shot.alt.clone() }
                            }
                        }
                    }
                }
                // Text, never markup.
                p { "data-part": "description", class: "text-sm whitespace-pre-wrap",
                    {if entry.description.is_empty() { entry.summary.clone() } else { entry.description.clone() }}
                }
                if let Some(deprecated) = &entry.deprecated {
                    p { role: "note", class: "text-sm text-warning",
                        {t!("Deprecated: {0}", 0 = deprecated.clone())}
                    }
                }
                if !compatibility.reasons.is_empty() {
                    ul { "data-part": "reasons", "aria-label": t!("Compatibility"), class: "flex flex-col gap-1 text-sm",
                        for reason in compatibility.reasons.clone() {
                            li {
                                key: "{reason.message}",
                                class: if reason.blocking { "text-danger" } else { "text-warning" },
                                "{reason.message}"
                            }
                        }
                    }
                }
                if let Some(release) = release.clone() {
                    dl { "data-part": "release", class: "grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm",
                        dt { class: "text-fg-muted", {t!("Version")} }
                        dd {
                            "{release.version}"
                            if let Some(published) = &release.published {
                                span { class: "text-fg-subtle", " · {published}" }
                            }
                        }
                        dt { class: "text-fg-muted", {t!("Runs on")} }
                        dd { {runs_on.clone().unwrap_or_default()} }
                        if entry.kind == AddonKind::Plugin {
                            dt { class: "text-fg-muted", {t!("Modes")} }
                            dd { {modes.clone().unwrap_or_default()} }
                            dt { class: "text-fg-muted", {t!("Asks for")} }
                            dd {
                                if asks.is_empty() {
                                    {t!("Nothing beyond its own storage")}
                                }
                                for line in asks.clone() {
                                    span { key: "{line}", class: "block", "{line}" }
                                }
                            }
                        }
                        dt { class: "text-fg-muted", {t!("Download")} }
                        dd {
                            {t!("{0} KB", 0 = ((release.artifact.size as f64 / 1024.0).round() as u64).max(1))}
                        }
                        if !release.notes.is_empty() {
                            dt { class: "text-fg-muted", {t!("What's new")} }
                            dd { class: "whitespace-pre-wrap", "{release.notes}" }
                        }
                    }
                }
                if let Some(text) = progress_text {
                    div { "data-part": "progress", role: "status", class: "text-sm text-fg-muted", "{text}" }
                }
                Outcome { outcome: outcome(), rejected: rejected.clone() }
                if offline && !installed {
                    p { class: "text-sm text-fg-muted", {t!("Installing needs a connection.")} }
                }
                if confirm_remove() {
                    div { "data-part": "remove-confirm", class: "flex flex-col gap-2 rounded-panel border border-danger/40 p-3",
                        p { class: "text-sm",
                            if entry.kind == AddonKind::Plugin {
                                {t!("Uninstall {0}?", 0 = entry.name.clone())}
                            } else {
                                {t!("Uninstall {0}? If it's the theme in use, the default theme takes over.", 0 = entry.name.clone())}
                            }
                        }
                        if entry.kind == AddonKind::Plugin {
                            Checkbox {
                                "data-spatial": "focusable",
                                label: t!("Keep its settings and data for a reinstall"),
                                checked: keep_data(),
                                on_change: move |on: bool| keep_data.set(on),
                            }
                        }
                        div { class: "flex gap-2",
                            Button { "data-spatial": "focusable", size: ButtonSize::Sm, on_press: move |_| confirm_remove.set(false), {t!("Keep it")} }
                            Button {
                                "data-part": "remove-confirmed",
                                "data-spatial": "focusable",
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Danger,
                                disabled: removing(),
                                on_press: remove,
                                {t!("Uninstall")}
                            }
                        }
                        if let Some(error) = remove_error() {
                            p { role: "alert", class: "text-sm text-danger", "{error}" }
                        }
                    }
                }
                div { "data-part": "actions", class: "flex flex-wrap justify-end gap-2",
                    if installing() {
                        Button {
                            "data-spatial": "focusable",
                            on_press: {
                                let addons = addons.clone();
                                let id = entry.id.clone();
                                move |_| {
                                    addons.cancel(&id);
                                }
                            },
                            {t!("Cancel download")}
                        }
                    }
                    if installed && !confirm_remove() {
                        Button {
                            "data-part": "remove",
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Ghost,
                            disabled: busy,
                            on_press: move |_| confirm_remove.set(true),
                            {t!("Uninstall…")}
                        }
                    }
                    if compatibility.state == ItemState::UpdateAvailable && release.is_some() {
                        Button {
                            "data-part": "skip",
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Ghost,
                            disabled: busy,
                            on_press: skip,
                            if skipped {
                                {t!("Offer this version again")}
                            } else {
                                {t!("Skip this version")}
                            }
                        }
                    }
                    if entry.kind == AddonKind::Theme && installed {
                        ApplyTheme { id: entry.id.clone() }
                    }
                    if release.is_some() && compatibility.state != ItemState::Installed {
                        Button {
                            "data-part": "install",
                            "data-spatial": "focusable",
                            "data-focus-default": "true",
                            variant: ButtonVariant::Primary,
                            disabled: busy,
                            on_press: {
                                let retry = rejected.is_some();
                                move |_| install((false, retry))
                            },
                            "{install_label}"
                        }
                    }
                    Button { "data-spatial": "focusable", disabled: busy, on_press: move |_| on_close.call(()), {t!("Close")} }
                }
                if let Some(error) = install_error() {
                    p { role: "alert", class: "text-sm text-danger", "{error}" }
                }
            }
        }
        if let Some(preview) = review {
            ReviewDialog {
                plugin: preview.plugin.clone(),
                installed_version: preview.installed_version.clone(),
                action: t!("Install and allow"),
                busy: installing(),
                error: install_error(),
                on_accept: move |_| install((true, false)),
                on_close: move |_| outcome.set(None),
            }
        }
    }
}

#[component]
fn Screenshot(id: String, index: u32, alt: String) -> Element {
    let media = use_addons_query({
        let id = id.clone();
        move |addons| {
            let id = id.clone();
            async move { addons.media(&id, index).await }
        }
    });
    let url = match &*media.read() {
        Some(Ok(Some(cached))) => Some(format!(
            "{}://localhost/{}",
            crate::library::ART_SCHEME,
            cached.cached_path.trim_start_matches('/')
        )),
        _ => None,
    };
    match url {
        Some(url) => rsx! {
            div { class: "relative h-36 w-64 overflow-hidden rounded-control",
                img { "data-part": "screenshot", src: "{url}", alt: "{alt}", class: "absolute inset-0 size-full object-cover" }
            }
        },
        None => rsx! {
            div {
                "data-part": "screenshot-missing",
                role: "img",
                "aria-label": if alt.is_empty() { t!("Screenshot") } else { alt.clone() },
                class: "grid h-36 w-64 place-items-center rounded-control border border-border bg-surface-base/60 text-fg-subtle",
                crate::ui::Svg { icon: IMAGE_OFF, class: "size-5" }
            }
        },
    }
}

#[component]
fn Outcome(
    outcome: Option<AddonOutcome>,
    rejected: Option<caskade_ui_data::addons::RejectedRelease>,
) -> Element {
    match (outcome, rejected) {
        (
            Some(AddonOutcome::Installed {
                version,
                needs_consent,
                restart_required,
                ..
            }),
            _,
        ) => {
            let text = if needs_consent {
                t!(
                    "{0} is installed. It asks for more than before, so it's off until you allow it under Installed plugins.",
                    0 = version
                )
            } else if restart_required {
                t!(
                    "{0} is installed. Its pages change when Caskade restarts.",
                    0 = version
                )
            } else {
                t!("{0} is installed.", 0 = version)
            };
            rsx! {
                p { "data-part": "outcome", "data-outcome": "installed", role: "status", class: "text-sm text-success", "{text}" }
            }
        }
        (outcome, rejected) => {
            let failure = match (outcome, rejected) {
                (
                    Some(AddonOutcome::Failed {
                        kind,
                        message,
                        rolled_back,
                        ..
                    }),
                    _,
                ) => Some((kind, message, rolled_back)),
                (None, Some(rejected)) => Some((rejected.kind, rejected.message, false)),
                _ => None,
            };
            let Some((kind, message, rolled_back)) = failure else {
                return rsx! {};
            };
            let transient = matches!(
                serde_json::to_value(kind)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .as_deref(),
                Some("network" | "storage")
            );
            let text = if rolled_back {
                t!(
                    "It didn't install, and the previous version is back: {0}",
                    0 = message
                )
            } else if transient {
                t!("It didn't install; try again later: {0}", 0 = message)
            } else {
                t!("It didn't install: {0}", 0 = message)
            };
            rsx! {
                p { "data-part": "outcome", "data-outcome": "failed", role: "alert", class: "text-sm text-danger", "{text}" }
            }
        }
    }
}

/// Makes an installed theme the one in use, as Appearance does.
#[component]
fn ApplyTheme(id: String) -> Element {
    let section = use_settings_section("core.appearance");
    let active = section
        .value()
        .is_some_and(|value| value.get("theme").and_then(Value::as_str) == Some(id.as_str()));
    rsx! {
        Button {
            "data-part": "apply",
            "data-spatial": "focusable",
            disabled: active || section.saving() || section.value().is_none(),
            on_press: move |_| section.set_field("theme", Value::String(id.clone())),
            if active {
                {t!("In use")}
            } else {
                {t!("Use this theme")}
            }
        }
    }
}

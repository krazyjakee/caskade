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
    AddonKind, AddonOs, AddonRecord, AppMode, CatalogueItem, ItemState, UpdatePreference,
};
use caskade_ui_data::plugins::{LibraryAccess, PluginSummary};
use caskade_ui_i18n::{t, tn, tp};
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

mod addon_details;
use addon_details::AddonDetails;
#[cfg(test)]
pub(super) use addon_details::permission_lines;

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
                    .is_none_or(|fetched| caskade_core::now_ms() - fetched > STALE_MS)
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
                0 = crate::library::date_time(at)
            ),
            (Some(at), Some(error), false) => t!(
                "Showing the catalogue from {0}; the last check failed: {1}",
                0 = crate::library::date_time(at),
                1 = error.clone()
            ),
            (Some(at), None, false) => t!("Updated {0}.", 0 = crate::library::date_time(at)),
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

//! A game's saves on its page (#100, the webview's
//! `views/game/SavesPanel.tsx`): the versions Caskade kept (newest first,
//! the one in use marked), Restore, choosing between devices' saves when
//! both played, where the saves are on this device with Show in folder, and
//! sharing a version with someone (#108's `ShareGameDialog`).

use super::data::{use_answer, use_page_data};
use super::launch::{SaveConflictDialog, use_save_device_name};
use crate::ui::{Button, ButtonSize, ButtonVariant, icons};
use caskade_core::saves::versions::{SaveHistory, VersionReason};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::prelude::*;

fn reason_label(reason: VersionReason) -> String {
    match reason {
        VersionReason::Session => tp!("save version", "After playing"),
        VersionReason::Found => tp!("save version", "Changed outside Caskade"),
        VersionReason::BeforeRestore => tp!("save version", "Before a restore"),
        VersionReason::Restored => tp!("save version", "Restored"),
        VersionReason::Chosen => tp!("save version", "Chosen between devices"),
        VersionReason::Shared => tp!("save version", "Shared by someone"),
    }
}

#[component]
pub fn SavesPanel(
    game_id: String,
    title: String,
    /// The release Play starts, whose saves these are.
    entry_id: Option<String>,
    running: bool,
) -> Element {
    let data = use_page_data();
    let device_name = use_save_device_name();
    let history = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.save_history)(game_id.clone())
        })
    };
    let mut locations_entry = use_signal(|| None::<String>);
    if *locations_entry.peek() != entry_id {
        locations_entry.set(entry_id.clone());
    }
    let locations = {
        let data = data.clone();
        use_answer(DataKey::Game(game_id.clone()), move || {
            let (data, entry) = (data.clone(), locations_entry());
            Box::pin(async move {
                match entry {
                    Some(entry) => (data.save_locations)(entry).await,
                    None => Ok(Vec::new()),
                }
            })
        })
    };
    let accounts = {
        let data = data.clone();
        use_answer(DataKey::Settings("core.accounts".to_owned()), move || {
            (data.accounts)()
        })
    };
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut done = use_signal(|| None::<String>);
    let mut choosing = use_signal(|| false);
    let mut sharing = use_signal(|| None::<String>);
    let read = history.read();
    let Some(Ok(Some(history))) = read.as_ref().map(|result| result.clone()) else {
        return rsx! {};
    };
    drop(read);
    let paths: Vec<String> = locations
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok().cloned())
        .unwrap_or_default();
    if history.versions.is_empty() && paths.is_empty() {
        return rsx! {};
    }
    let account_names: Vec<(String, String)> = accounts
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok().cloned())
        .unwrap_or_default();
    let account_name = |id: &str| {
        if id.is_empty() {
            return String::new();
        }
        account_names
            .iter()
            .find(|(each, _)| each == id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| tp!("save version", "Another account"))
    };
    let SaveHistory {
        versions,
        conflict,
        syncing,
        ..
    } = history;
    let conflicting: Vec<_> = versions
        .iter()
        .filter(|each| conflict.contains(&each.version.id))
        .map(|each| each.version.clone())
        .collect();
    let restore = {
        let (data, entry_id) = (data.clone(), entry_id.clone());
        move |version: String| {
            let Some(entry) = entry_id.clone() else {
                return;
            };
            let data = data.clone();
            busy.set(true);
            error.set(None);
            done.set(None);
            spawn(async move {
                match (data.save_restore)(entry, version).await {
                    Ok(()) => done.set(Some(t!(
                        "Restored. The save that was in use is kept as a version."
                    ))),
                    Err(report) => error.set(Some(report.summary)),
                }
                busy.set(false);
            });
        }
    };
    let choose = {
        let (data, entry_id) = (data.clone(), entry_id.clone());
        move |version: String| {
            let Some(entry) = entry_id.clone() else {
                return;
            };
            let data = data.clone();
            busy.set(true);
            error.set(None);
            spawn(async move {
                match (data.save_choose)(entry, version).await {
                    Ok(()) => choosing.set(false),
                    Err(report) => error.set(Some(report.summary)),
                }
                busy.set(false);
            });
        }
    };
    let reveal = {
        let (data, entry_id) = (data.clone(), entry_id.clone());
        move |index: u32| {
            let Some(entry) = entry_id.clone() else {
                return;
            };
            let data = data.clone();
            spawn(async move {
                if let Err(report) = (data.save_reveal)(entry, index).await {
                    error.set(Some(report.summary));
                }
            });
        }
    };
    let shares = crate::flows::receive_share::use_shares();
    rsx! {
        section { "data-cs": "saves-panel", class: "mt-8", "aria-labelledby": "saves-heading",
            h2 { id: "saves-heading", class: "font-display text-xl font-semibold", {t!("Saves")} }
            if !syncing {
                p { class: "mt-1 text-sm text-fg-muted", {t!("Saves stay on this device: syncing them is off in Settings → Devices.")} }
            }
            if conflicting.len() > 1 {
                div { "data-part": "conflict", class: "mt-4 flex flex-wrap items-center justify-between gap-3 rounded-panel border border-warning/50 bg-warning/10 p-4 text-sm",
                    p { {t!("This game was played on more than one device since they last shared saves. Choose which save to play with; the other is kept.")} }
                    Button {
                        "data-spatial": "focusable",
                        "data-part": "choose",
                        disabled: entry_id.is_none() || running || busy(),
                        on_press: move |_| choosing.set(true),
                        {t!("Choose a save…")}
                    }
                }
            }
            if !paths.is_empty() && entry_id.is_some() {
                div { "data-part": "locations", class: "mt-4",
                    h3 { class: "text-sm font-semibold", {t!("Where they are on this device")} }
                    ul { class: "mt-2 flex flex-col gap-1",
                        for (index, path) in paths.iter().enumerate() {
                            li { key: "{path}", class: "flex items-center justify-between gap-3",
                                span { class: "min-w-0 break-all font-mono text-xs text-fg-muted", "{path}" }
                                // A file manager would open behind console
                                // mode's fullscreen window (#107).
                                if !crate::console::is_console() {
                                Button {
                                    "data-spatial": "focusable",
                                    "data-part": "reveal",
                                    size: ButtonSize::Sm,
                                    variant: ButtonVariant::Ghost,
                                    icon: Some(icons::FOLDER_OPEN),
                                    "aria-label": t!("Show in folder: {path}", path = path.clone()),
                                    on_press: {
                                        let reveal = reveal.clone();
                                        move |_| reveal(index as u32)
                                    },
                                    {t!("Show in folder")}
                                }
                                }
                            }
                        }
                    }
                }
            }
            if versions.is_empty() {
                p { class: "mt-4 text-sm text-fg-muted", {t!("No saves kept yet. A version is kept each time you finish playing.")} }
            } else {
                ul { "data-part": "versions", class: "mt-4 flex flex-col gap-2",
                    for each in versions.clone() {
                        li {
                            key: "{each.version.id}",
                            "data-part": "version",
                            "data-status": if each.current { "current" } else if each.complete { "kept" } else { "arriving" },
                            class: "flex flex-wrap items-center justify-between gap-3 rounded-panel border border-border bg-surface-raised p-3",
                            div { class: "min-w-0",
                                p { class: "text-sm font-medium",
                                    {crate::library::date_time(each.version.created_at_ms)}
                                    if each.current {
                                        span { class: "ml-2 rounded-full border border-accent px-2 py-0.5 text-xs font-normal text-accent-alt",
                                            {tp!("save version", "In use")}
                                        }
                                    }
                                }
                                p { class: "text-xs text-fg-muted",
                                    {
                                        let who = [device_name(&each.version), account_name(&each.version.account_id)]
                                            .into_iter()
                                            .filter(|part| !part.is_empty())
                                            .collect::<Vec<_>>()
                                            .join(" · ");
                                        let mut line = format!(
                                            "{who} · {} · {}",
                                            reason_label(each.version.reason),
                                            format::bytes(Some(each.version.size_bytes))
                                        );
                                        if !each.complete {
                                            line.push_str(&format!(" · {}", t!("Not on this device yet")));
                                        }
                                        line
                                    }
                                }
                            }
                            div { class: "flex gap-2",
                                if each.complete && shares.is_some() {
                                    Button {
                                        "data-spatial": "focusable",
                                        "data-part": "share",
                                        size: ButtonSize::Sm,
                                        variant: ButtonVariant::Ghost,
                                        "aria-label": t!("Share this save with someone…"),
                                        on_press: {
                                            let id = each.version.id.clone();
                                            move |_| sharing.set(Some(id.clone()))
                                        },
                                        {t!("Share…")}
                                    }
                                }
                                if !each.current {
                                    Button {
                                        "data-spatial": "focusable",
                                        "data-part": "restore",
                                        size: ButtonSize::Sm,
                                        disabled: entry_id.is_none() || running || busy() || !each.complete,
                                        on_press: {
                                            let mut restore = restore.clone();
                                            let id = each.version.id.clone();
                                            move |_| restore(id.clone())
                                        },
                                        {t!("Restore")}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if running && !versions.is_empty() {
                p { class: "mt-2 text-xs text-fg-subtle", {t!("Restoring waits until the game has closed.")} }
            }
            if let Some(error) = error() {
                p { role: "alert", class: "mt-2 text-sm text-danger", "{error}" }
            }
            if let Some(done) = done() {
                p { role: "status", class: "mt-2 text-sm text-fg-muted", "{done}" }
            }
            if choosing() && entry_id.is_some() {
                SaveConflictDialog {
                    versions: conflicting.clone(),
                    busy: busy(),
                    error: error(),
                    action: t!("Use this save"),
                    on_close: move |_| choosing.set(false),
                    on_choose: {
                        let mut choose = choose.clone();
                        move |version: String| choose(version)
                    },
                }
            }
            if let (Some(version), Some(shares)) = (sharing(), shares.clone()) {
                crate::flows::share_game::ShareGameDialog {
                    shares,
                    target: crate::flows::share_game::ShareTarget::Item {
                        item: caskade_ui_data::shares::ShareItem::Save { version_id: version },
                        title: t!("a save of {title}", title = title.clone()),
                    },
                    on_close: move |_| sharing.set(None),
                }
            }
        }
    }
}

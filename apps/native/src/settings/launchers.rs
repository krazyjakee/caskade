//! Settings → Launchers (#104), as the webview's `LaunchersSettings` and
//! `launchers.ts`: the launchers discovery looks for on this device,
//! whether it found each, and a switch for scanning it (#48). The one
//! place `core.launchers` changes; Library Discovery links here.

use caskade_app::discovery::DiscoverySnapshot;
use caskade_ui_data::discovery::LauncherStatus;
use caskade_ui_data::discovery::scanning;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use serde_json::{Map, Value};

use super::reset::{ResetDefaults, ResetField, ResetPart};
use super::state::{SettingsFeedback, SettingsLoading, use_settings_section};
use crate::data::{change, try_use_discovery, use_discovery_query};
use crate::router::{Route, RouteLink};
use crate::ui::icons::{GAMEPAD_2, REFRESH_CW};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, DotTone, FailureNotice, NoticeVariant, SettingsSection,
    StatusDot, Switch, SwitchSize,
};

/// A launcher as people know it, and the discovery sources that read its
/// games (#48). The switch keys in `core.launchers.enabled` are those
/// source ids: one switch turns all of a launcher's sources off.
#[derive(Clone, Debug, PartialEq)]
pub struct Launcher {
    pub id: String,
    /// Its own name: a product's, not translated.
    pub name: String,
    pub sources: Vec<String>,
}

const KNOWN: &[(&str, &str, &[&str])] = &[
    ("steam", "Steam", &["steam"]),
    ("heroic", "Heroic Games Launcher", &["heroic"]),
    ("lutris", "Lutris", &["lutris"]),
    ("epic", "Epic Games Launcher", &["epic", "epic.app"]),
    // Rare keeps its games in Legendary's records.
    ("legendary", "Legendary / Rare", &["legendary", "rare"]),
    ("gog", "GOG Galaxy", &["gog"]),
    ("minigalaxy", "Minigalaxy", &["minigalaxy"]),
    ("ea", "EA app", &["ea"]),
    ("ubisoft", "Ubisoft Connect", &["ubisoft"]),
    ("battlenet", "Battle.net", &["battlenet"]),
    ("itch", "itch", &["itch"]),
    ("amazon", "Amazon Games", &["amazon"]),
    ("riot", "Riot Client", &["riot"]),
    ("rockstar", "Rockstar Games Launcher", &["rockstar"]),
    ("bottles", "Bottles", &["bottles"]),
    ("faugus", "Faugus Launcher", &["faugus"]),
];

fn known(source: &str) -> Option<(usize, Launcher)> {
    KNOWN
        .iter()
        .enumerate()
        .find(|(_, (_, _, sources))| sources.contains(&source))
        .map(|(at, (id, name, sources))| {
            (
                at,
                Launcher {
                    id: (*id).to_owned(),
                    name: (*name).to_owned(),
                    sources: sources.iter().map(|source| (*source).to_owned()).collect(),
                },
            )
        })
}

/// A discovery source's name for people: its launcher's, or the system
/// record's. An unknown id shows as it is.
pub fn source_name(source: &str) -> String {
    let system = match source {
        "desktop" => Some(tp!("discovery source", "Desktop entries")),
        "flatpak" => Some(tp!("discovery source", "Flatpak apps")),
        "snap" => Some(tp!("discovery source", "Snap apps")),
        "prefixes" => Some(tp!("discovery source", "Wine and Proton prefixes")),
        "startmenu" => Some(tp!("discovery source", "Start Menu shortcuts")),
        "uninstall" => Some(tp!("discovery source", "Installed programs")),
        "gameux" => Some(tp!("discovery source", "Game Explorer")),
        "xbox" => Some(tp!("discovery source", "Xbox and packaged apps")),
        "macos.apps" => Some(tp!("discovery source", "Applications")),
        "steam.shortcuts" => Some(tp!("discovery source", "Steam shortcuts")),
        "deep" => Some(tp!("discovery source", "Deep scans")),
        _ => None,
    };
    system
        .or_else(|| known(source).map(|(_, launcher)| launcher.name))
        .unwrap_or_else(|| source.to_owned())
}

/// A launcher and where discovery found it on this device (empty when it
/// didn't).
#[derive(Clone, Debug, PartialEq)]
pub struct DetectedLauncher {
    pub launcher: Launcher,
    pub found: Vec<LauncherStatus>,
}

/// The launchers discovery looks for on this device, found ones first, each
/// once however many of its sources reported. A source the list doesn't
/// know shows under its own id.
pub fn detected_launchers(detected: &[LauncherStatus]) -> Vec<DetectedLauncher> {
    let mut launchers: Vec<(usize, DetectedLauncher)> = Vec::new();
    for row in detected {
        let source = row.source.to_string();
        let (order, launcher) = known(&source).unwrap_or((
            usize::MAX,
            Launcher {
                id: source.clone(),
                name: source.clone(),
                sources: vec![source.clone()],
            },
        ));
        let at = match launchers
            .iter()
            .position(|(_, entry)| entry.launcher.id == launcher.id)
        {
            Some(at) => at,
            None => {
                launchers.push((
                    order,
                    DetectedLauncher {
                        launcher,
                        found: Vec::new(),
                    },
                ));
                launchers.len() - 1
            }
        };
        if row.found {
            launchers[at].1.found.push(row.clone());
        }
    }
    launchers.sort_by_key(|(order, entry)| (entry.found.is_empty(), *order));
    launchers.into_iter().map(|(_, entry)| entry).collect()
}

/// On unless one of its sources was turned off.
pub fn launcher_enabled(launcher: &Launcher, enabled: &Map<String, Value>) -> bool {
    launcher
        .sources
        .iter()
        .all(|source| enabled.get(source) != Some(&Value::Bool(false)))
}

/// `enabled` with every source of `launcher` turned on or off.
pub fn with_launcher(
    enabled: &Map<String, Value>,
    launcher: &Launcher,
    on: bool,
) -> Map<String, Value> {
    let mut next = enabled.clone();
    for source in &launcher.sources {
        next.insert(source.clone(), Value::Bool(on));
    }
    next
}

#[component]
pub fn LaunchersSettings() -> Element {
    let section = use_settings_section("core.launchers");
    let discovery = try_use_discovery();
    let mut snapshot = use_discovery_query(|discovery| async move { discovery.status().await });
    let mut action_error = use_signal(|| None);
    let mut looking = use_signal(|| false);
    let detect = use_callback(move |()| {
        let Some(discovery) = discovery.clone() else {
            return;
        };
        looking.set(true);
        action_error.set(None);
        spawn(async move {
            let outcome = change(async move { discovery.launchers().await }).await;
            looking.set(false);
            match outcome {
                Ok(_) => snapshot.restart(),
                Err(error) => action_error.set(Some(error)),
            }
        });
    });
    let current: Option<DiscoverySnapshot> = match &*snapshot.read() {
        Some(Ok(current)) => Some(current.clone()),
        _ => None,
    };
    let load_error = match &*snapshot.read() {
        Some(Err(error)) => Some(error.clone()),
        _ => None,
    };
    // Detection comes with each scan; a device that hasn't scanned since
    // Caskade started looks once when the page opens.
    let mut asked = use_signal(|| false);
    let unseen = current
        .as_ref()
        .is_some_and(|current| current.launchers.is_empty() && !scanning(Some(current)));
    use_effect(use_reactive!(|unseen| {
        if unseen && !*asked.peek() {
            asked.set(true);
            detect(());
        }
    }));
    let Some(settings) = section.value() else {
        return rsx! {
            SettingsLoading { error: section.load_error(), on_reload: move |_| section.reload() }
        };
    };
    let enabled = settings
        .get("enabled")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let launchers = detected_launchers(
        current
            .as_ref()
            .map(|current| current.launchers.as_slice())
            .unwrap_or_default(),
    );
    let busy = looking() || scanning(current.as_ref());
    let error = load_error.or(action_error());
    let reset_parts = vec![ResetPart {
        namespace: "core.launchers".into(),
        whole: true,
        fields: vec![ResetField {
            key: "enabled".into(),
            label: t!("Which launchers are scanned"),
        }],
    }];
    rsx! {
        div { "data-settings": "launchers", class: "flex flex-col gap-4",
            div { class: "flex items-center justify-between gap-3",
                SettingsFeedback { error: section.error(), saving: section.saving() }
                ResetDefaults {
                    group: tp!("settings tab", "Launchers"),
                    label: t!("Reset launcher settings"),
                    parts: reset_parts,
                    disabled: section.saving(),
                }
            }
            SettingsSection {
                icon: Some(GAMEPAD_2),
                title: t!("Launchers"),
                description: t!("Games from the launchers turned on here join your library. Turning one off removes the games only it found at the next scan."),
                aside: rsx! {
                    Button {
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        icon: Some(REFRESH_CW),
                        disabled: busy,
                        on_press: move |_| detect(()),
                        {t!("Look again")}
                    }
                },
                DiscoveryLink {}
                if let Some(error) = error {
                    FailureNotice {
                        error,
                        variant: NoticeVariant::Inline,
                        class: "mt-3",
                        on_retry: move |_| {
                            if action_error.peek().is_some() {
                                detect(());
                            } else {
                                snapshot.restart();
                            }
                        },
                    }
                } else if launchers.is_empty() {
                    p { role: "status", class: "mt-3 text-sm text-fg-muted",
                        if busy || current.is_none() {
                            {t!("Looking for launchers on this device…")}
                        } else {
                            {t!("Caskade knows no launchers for this device's system.")}
                        }
                    }
                } else {
                    ul { class: "mt-1 flex flex-col",
                        for detected in launchers {
                            LauncherRow {
                                key: "{detected.launcher.id}",
                                on: launcher_enabled(&detected.launcher, &enabled),
                                detected: detected.clone(),
                                on_change: {
                                    let enabled = enabled.clone();
                                    let launcher = detected.launcher.clone();
                                    move |on: bool| {
                                        section.set_field(
                                            "enabled",
                                            Value::Object(with_launcher(&enabled, &launcher, on)),
                                        )
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LauncherRow(detected: DetectedLauncher, on: bool, on_change: EventHandler<bool>) -> Element {
    let found = !detected.found.is_empty();
    rsx! {
        li {
            "data-part": "launcher",
            "data-status": if found { "detected" } else { "not-found" },
            class: "flex items-center gap-3 border-t border-border py-2 first:border-t-0",
            div { class: "min-w-0 flex-1",
                p { class: "text-sm font-medium", "{detected.launcher.name}" }
                if found {
                    for (index, row) in detected.found.iter().enumerate() {
                        p { key: "{index}", class: "break-all font-mono text-xs text-fg-muted",
                            {
                                let place = row
                                    .path
                                    .as_ref()
                                    .map(|path| crate::flows::paths::format_path(&path.to_string_lossy()))
                                    .unwrap_or_else(|| t!("Location unknown"));
                                match &row.version {
                                    Some(version) => format!("{place} · {version}"),
                                    None => place,
                                }
                            }
                        }
                    }
                } else {
                    p { class: "text-xs text-fg-muted", {t!("Not found on this device")} }
                }
            }
            StatusDot { tone: if on && found { DotTone::Ok } else { DotTone::Off },
                if on {
                    {t!("Scanned")}
                } else {
                    {t!("Not scanned")}
                }
            }
            Switch {
                size: SwitchSize::Sm,
                "aria-label": "{detected.launcher.name}",
                checked: on,
                on_change: move |next: bool| on_change.call(next),
            }
        }
    }
}

/// Scans, folders and the system's checks are Library Discovery's (#46); it
/// links back here.
#[component]
fn DiscoveryLink() -> Element {
    let href = Route::Settings {
        tab: "library-discovery",
        section: None,
    }
    .href();
    rsx! {
        p { class: "mt-1 text-sm text-fg-muted",
            {crate::trans!("How and when games are scanned: <0>Library Discovery</0>";
                0 => |inner| rsx! {
                    RouteLink { to: href.clone(), current: false, class: "text-accent-alt", {inner} }
                })}
        }
    }
}

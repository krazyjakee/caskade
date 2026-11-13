//! What Play asks before a game starts (#100, the webview's
//! `views/game/launch.tsx`, `UpdateBeforePlay.tsx`, `PlayingElsewhere.tsx`,
//! `SavesBeforePlay.tsx`, `components/saves/SaveConflictDialog.tsx`): a
//! newer release's offer, a warning when it's running on another device,
//! the saves (a newer one from another device, or which to use when both
//! played), then the launch. `game_menu::play::PlayRequest` runs them in
//! that order for every Play: the game page's, a game's menu and card, and
//! another device's request. [`LaunchSteps`] is what they ask the app,
//! which a test replaces.

use crate::data::change;
use crate::data::status::use_devices;
use crate::ui::{Button, ButtonVariant, Dialog, RadioGroup, RadioOption};
use caskade_app::commands::launch::LaunchOutcome;
use caskade_app::commands::presence::PlayingElsewhere;
use caskade_core::error::ErrorReport;
use caskade_core::install::JobStatus;
use caskade_core::launch::LaunchChoice;
use caskade_core::saves::versions::{BeforePlay, SaveCheck, SaveVersion};
use caskade_core::updates::GameUpdate;
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::prelude::*;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

/// One of the app's answers.
pub type Answer<T> = Pin<Box<dyn Future<Output = Result<T, ErrorReport>>>>;

/// What the launch asks the app, step by step: the running app's services,
/// or a test's fakes (`use_context_provider`).
#[derive(Clone)]
pub struct LaunchSteps {
    /// The release to offer before entry `0` plays; `None` to play now.
    pub update_before_play: Rc<dyn Fn(String) -> Answer<Option<GameUpdate>>>,
    /// Stops offering release `1` for entry `0`.
    pub skip_update: Rc<dyn Fn(String, String) -> Answer<()>>,
    /// Installs entry `0`'s newest release: the job's id.
    pub install_update: Rc<dyn Fn(String) -> Answer<String>>,
    /// What's running on other devices.
    pub playing_elsewhere: Rc<dyn Fn() -> Answer<Vec<PlayingElsewhere>>>,
    /// Entry `0`'s saves before it plays.
    pub saves_before_play: Rc<dyn Fn(String) -> Answer<BeforePlay>>,
    /// Plays entry `0` with save version `1`.
    pub choose_save: Rc<dyn Fn(String, String) -> Answer<()>>,
    /// Starts entry `0`.
    pub launch: Rc<dyn Fn(String, LaunchChoice) -> Answer<LaunchOutcome>>,
    /// Waits a moment (looking again for a save on its way).
    pub pause: Rc<dyn Fn(std::time::Duration) -> Answer<()>>,
}

impl PartialEq for LaunchSteps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.launch, &other.launch)
    }
}

fn unavailable() -> ErrorReport {
    ErrorReport::from(t!("That isn't available in this copy of Caskade."))
}

impl LaunchSteps {
    /// The running app's. Without its services (fixtures mode) there's
    /// nothing to offer, nobody else playing and no saves to check, and a
    /// launch says it isn't available.
    pub fn of_app() -> Self {
        Self {
            update_before_play: Rc::new(|entry_id| {
                Box::pin(async move {
                    let Some(updates) =
                        crate::shell::services().and_then(|services| services.updates.as_ref())
                    else {
                        return Ok(None);
                    };
                    change(async move {
                        caskade_app::commands::updates::game_update_before_play(updates, &entry_id)
                            .await
                    })
                    .await
                })
            }),
            skip_update: Rc::new(|entry_id, version| {
                Box::pin(async move {
                    let updates = crate::shell::services()
                        .and_then(|services| services.updates.as_ref())
                        .ok_or_else(unavailable)?;
                    change(async move {
                        caskade_app::commands::updates::game_update_skip(
                            updates,
                            &entry_id,
                            Some(version),
                        )
                        .await
                        .map(drop)
                    })
                    .await
                })
            }),
            install_update: Rc::new(|entry_id| {
                Box::pin(async move {
                    let services = crate::shell::services().ok_or_else(unavailable)?;
                    let updates = services.updates.as_ref().ok_or_else(unavailable)?;
                    change(async move {
                        caskade_app::commands::updates::game_update_install(
                            updates,
                            &services.launch,
                            &entry_id,
                        )
                        .await
                        .map(|job| job.id)
                    })
                    .await
                })
            }),
            playing_elsewhere: Rc::new(|| {
                Box::pin(async move {
                    let Some(services) = crate::shell::services() else {
                        return Ok(Vec::new());
                    };
                    change(async move {
                        caskade_app::commands::presence::devices_playing(
                            &services.presence,
                            &services.devices,
                            &services.store,
                        )
                        .await
                    })
                    .await
                })
            }),
            saves_before_play: Rc::new(|entry_id| {
                Box::pin(async move {
                    let Some(services) = crate::shell::services() else {
                        return Ok(BeforePlay {
                            check: SaveCheck::NoSaves,
                            checked: true,
                            warnings: Vec::new(),
                        });
                    };
                    change(async move {
                        caskade_app::commands::saves::game_saves_before_play(
                            &services.save_versions,
                            entry_id,
                        )
                        .await
                    })
                    .await
                })
            }),
            choose_save: Rc::new(|entry_id, version_id| {
                Box::pin(async move {
                    let services = crate::shell::services().ok_or_else(unavailable)?;
                    change(async move {
                        caskade_app::commands::saves::game_saves_choose(
                            &services.save_versions,
                            entry_id,
                            version_id,
                        )
                        .await
                        .map(drop)
                    })
                    .await
                })
            }),
            launch: Rc::new(|entry_id, choice| {
                Box::pin(crate::game_menu::actions::launch(entry_id, Some(choice)))
            }),
            pause: Rc::new(|duration| {
                Box::pin(async move {
                    change(async move {
                        tokio::time::sleep(duration).await;
                        Ok(())
                    })
                    .await
                })
            }),
        }
    }
}

/// The launch's steps: a test's, else the app's.
pub fn use_launch_steps() -> LaunchSteps {
    let provided = try_use_context::<LaunchSteps>();
    use_hook(move || provided.unwrap_or_else(LaunchSteps::of_app))
}

/// A version, as the webview's `formatVersion`: `v1.2` for `1.2`, a tag as
/// it is.
pub fn version_label(version: &str) -> String {
    if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("v{version}")
    } else {
        version.to_owned()
    }
}

/// Update before playing? Update and play (then plays once the update's job
/// completes), Play now, or Skip this version.
#[component]
pub fn UpdateOffer(
    update: GameUpdate,
    on_close: EventHandler<()>,
    on_play: EventHandler<()>,
) -> Element {
    let steps = use_launch_steps();
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut job = use_signal(|| None::<String>);
    let jobs = crate::data::use_jobs_query();
    let latest = update.latest.clone().unwrap_or_default();
    let latest_version = version_label(&latest);
    let body = match &update.installed {
        Some(installed) => t!(
            note = "Versions, e.g. v1.2",
            "{latestVersion} is out, and you have {installedVersion}. Updating keeps your saves and settings.",
            latestVersion = latest_version.clone(),
            installedVersion = version_label(installed)
        ),
        None => t!(
            note = "A version, e.g. v1.2",
            "{latestVersion} is out. Updating keeps your saves and settings.",
            latestVersion = latest_version.clone()
        ),
    };
    // Plays once the update's job has completed; says so if it failed.
    let job_status = job().and_then(|id| {
        jobs.read()
            .as_ref()
            .and_then(|jobs| jobs.as_ref().ok().cloned())
            .and_then(|jobs| jobs.into_iter().find(|each| each.id == id))
    });
    let mut played = use_signal(|| false);
    if let Some(snapshot) = &job_status
        && snapshot.status == JobStatus::Completed
        && !*played.peek()
    {
        played.set(true);
        on_play.call(());
    }
    let progress = job_status.as_ref().map(|snapshot| match snapshot.status {
        JobStatus::Completed => t!("Updated."),
        JobStatus::Failed => snapshot
            .error
            .clone()
            .unwrap_or_else(|| t!("The update didn't finish.")),
        _ => t!("Updating…"),
    });
    let skip = {
        let steps = steps.clone();
        let entry = update.entry_id.clone();
        let latest = latest.clone();
        move |_| {
            let (steps, entry, latest) = (steps.clone(), entry.clone(), latest.clone());
            busy.set(true);
            spawn(async move {
                match (steps.skip_update)(entry, latest).await {
                    Ok(()) => on_play.call(()),
                    Err(report) => error.set(Some(report.summary)),
                }
                busy.set(false);
            });
        }
    };
    let install = {
        let entry = update.entry_id.clone();
        move |_| {
            let (steps, entry) = (steps.clone(), entry.clone());
            busy.set(true);
            spawn(async move {
                match (steps.install_update)(entry).await {
                    Ok(id) => job.set(Some(id)),
                    Err(report) => error.set(Some(report.summary)),
                }
                busy.set(false);
            });
        }
    };
    rsx! {
        Dialog {
            title: t!("Update before playing?"),
            "data-cs": "update-offer",
            on_close: move |_| {
                if !busy() {
                    on_close.call(());
                }
            },
            p { class: "text-sm text-fg-muted", "{body}" }
            if let Some(url) = update.latest_url.clone() {
                super::ExternalLink {
                    href: url,
                    class: "mt-2 inline-flex items-center gap-1 text-sm text-accent-alt",
                    {t!("What's new")}
                }
            }
            if let Some(progress) = progress {
                p { role: "status", "data-part": "progress", class: "mt-4 text-sm", "{progress}" }
            }
            if let Some(error) = error() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            div { class: "mt-6 flex flex-wrap justify-end gap-2",
                if job().is_some() {
                    Button { "data-spatial": "focusable", on_press: move |_| on_close.call(()), {t!("Close")} }
                } else {
                    Button {
                        "data-spatial": "focusable",
                        "data-part": "skip",
                        variant: ButtonVariant::Ghost,
                        disabled: busy(),
                        on_press: skip,
                        {t!("Skip this version")}
                    }
                    Button {
                        "data-spatial": "focusable",
                        "data-part": "play-now",
                        disabled: busy(),
                        on_press: move |_| on_play.call(()),
                        {t!("Play now")}
                    }
                    Button {
                        "data-spatial": "focusable",
                        "data-part": "update",
                        "data-focus-default": "true",
                        variant: ButtonVariant::Primary,
                        disabled: busy(),
                        on_press: install,
                        if busy() { {t!("Starting…")} } else { {t!("Update and play")} }
                    }
                }
            }
        }
    }
}

fn another_device() -> String {
    tp!("playing on", "another device")
}

/// "{title} is running on {device}": each device keeps its own save if it's
/// played here too. Cancel, or Play anyway.
#[component]
pub fn PlayingElsewhereDialog(
    playing: Vec<PlayingElsewhere>,
    on_close: EventHandler<()>,
    on_play: EventHandler<()>,
) -> Element {
    let first = playing.first().cloned();
    let device = first
        .as_ref()
        .map(|each| each.device_name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(another_device);
    let title = first.map(|each| each.title).unwrap_or_default();
    rsx! {
        Dialog {
            title: t!("{title} is running on {device}", title = title, device = device),
            "data-cs": "playing-elsewhere",
            on_close: move |_| on_close.call(()),
            p { class: "text-sm text-fg-muted",
                {t!("If you play here too, each device keeps its own save, and the next time you play you choose which one to carry on with.")}
            }
            ul { class: "mt-4 space-y-1 text-sm",
                for each in playing {
                    li { key: "{each.device_id}:{each.started_at_ms}",
                        {
                            let name = if each.device_name.is_empty() { another_device() } else { each.device_name.clone() };
                            let since = crate::library::date_time(each.started_at_ms);
                            t!("On {name} since {since}", name = name, since = since)
                        }
                    }
                }
            }
            div { class: "mt-6 flex justify-end gap-2",
                Button {
                    "data-spatial": "focusable",
                    "data-focus-default": "true",
                    on_press: move |_| on_close.call(()),
                    {t!("Cancel")}
                }
                Button {
                    "data-spatial": "focusable",
                    "data-part": "play-anyway",
                    variant: ButtonVariant::Primary,
                    on_press: move |_| on_play.call(()),
                    {t!("Play anyway")}
                }
            }
        }
    }
}

/// Names the device that made a save version: this device, a paired device
/// by its name, or another device once it's no longer paired.
pub fn use_save_device_name() -> impl Fn(&SaveVersion) -> String + Clone {
    let devices = use_devices();
    let names: Vec<(String, String)> = devices
        .read()
        .as_ref()
        .and_then(|status| status.as_ref().ok().cloned().flatten())
        .map(|status| {
            status
                .devices
                .into_iter()
                .map(|device| (device.id, device.name))
                .collect()
        })
        .unwrap_or_default();
    move |version: &SaveVersion| {
        if version.local {
            return tp!("save version", "This device");
        }
        names
            .iter()
            .find(|(id, _)| id == &version.device_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| tp!("save version", "Another device"))
    }
}

/// Both devices played since they last agreed: which save to play with
/// (at Play, or from the game's Saves panel). The others stay as versions.
/// Newest first; this device's own is chosen to begin with.
#[component]
pub fn SaveConflictDialog(
    versions: Vec<SaveVersion>,
    busy: bool,
    #[props(default)] error: Option<String>,
    /// The confirming button's label; Play's question plays with the save.
    #[props(default)]
    action: Option<String>,
    on_close: EventHandler<()>,
    on_choose: EventHandler<String>,
) -> Element {
    let device_name = use_save_device_name();
    let mut chosen = use_signal(|| {
        versions
            .iter()
            .find(|each| each.local)
            .or(versions.first())
            .map(|each| each.id.clone())
    });
    let options: Vec<RadioOption> = versions
        .iter()
        .map(|version| RadioOption {
            description: Some(format!(
                "{} · {}",
                crate::library::date_time(version.created_at_ms),
                format::size(Some(version.size_bytes))
            )),
            disabled: busy,
            ..RadioOption::new(version.id.clone(), device_name(version))
        })
        .collect();
    let label = action.unwrap_or_else(|| {
        if busy {
            t!("Starting…")
        } else {
            t!("Play with this save")
        }
    });
    rsx! {
        Dialog {
            title: t!("Choose which save to play with"),
            "data-cs": "save-conflict",
            on_close: move |_| {
                if !busy {
                    on_close.call(());
                }
            },
            p { class: "text-sm text-fg-muted",
                {t!("This game was played on more than one device since they last shared saves. The save you don't choose is kept as a version.")}
            }
            div { "data-part": "versions", class: "mt-5",
                RadioGroup {
                    label: t!("Saves"),
                    options,
                    value: chosen(),
                    on_change: move |id: String| chosen.set(Some(id)),
                }
            }
            if let Some(error) = error {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            div { class: "mt-6 flex justify-end gap-2",
                Button { "data-spatial": "focusable", disabled: busy, on_press: move |_| on_close.call(()), {t!("Cancel")} }
                Button {
                    "data-spatial": "focusable",
                    "data-part": "choose",
                    variant: ButtonVariant::Primary,
                    disabled: chosen().is_none() || busy,
                    on_press: move |_| {
                        if let Some(id) = chosen() {
                            on_choose.call(id);
                        }
                    },
                    "{label}"
                }
            }
        }
    }
}

/// Another device's newer save is on its way: Skip plays with the save
/// here (the caller looks again meanwhile).
#[component]
pub fn WaitForSave(
    version: SaveVersion,
    on_skip: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let device_name = use_save_device_name();
    let device = device_name(&version);
    rsx! {
        Dialog {
            title: t!("Waiting for the save from {device}", device = device),
            "data-cs": "save-arriving",
            on_close: move |_| on_close.call(()),
            p { role: "status", class: "text-sm text-fg-muted",
                {t!("It was just played there, and its save is on its way. Skip to play with the save on this device.")}
            }
            div { class: "mt-6 flex justify-end gap-2",
                Button { "data-spatial": "focusable", on_press: move |_| on_close.call(()), {t!("Cancel")} }
                Button {
                    "data-spatial": "focusable",
                    "data-focus-default": "true",
                    "data-part": "skip",
                    variant: ButtonVariant::Primary,
                    on_press: move |_| on_skip.call(()),
                    {t!("Skip")}
                }
            }
        }
    }
}

/// What the check before the launch did, when it's worth knowing; empty
/// when not.
pub fn save_note(result: &BeforePlay, device_name: impl Fn(&SaveVersion) -> String) -> Vec<String> {
    let mut lines = Vec::new();
    match &result.check {
        SaveCheck::Restored { version } => lines.push(t!(
            "Started with the save from {device}.",
            device = device_name(version)
        )),
        SaveCheck::NotArrived { version } => lines.push(t!(
            "A newer save from {device} hasn't arrived yet, so the game started with the save on this device.",
            device = device_name(version)
        )),
        _ => {}
    }
    if !result.checked {
        lines.push(t!(
            "Not every paired device could be reached, so one of them may have a newer save."
        ));
    }
    lines.extend(result.warnings.iter().cloned());
    lines
}

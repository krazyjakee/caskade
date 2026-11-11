//! One launch action's fields (the webview's `gameActions/ActionForm.tsx`):
//! its name, whether it plays the game or is a tool, what it starts, its
//! arguments, working folder and environment, the Proton build a Windows
//! program runs with, and how a play action's game is tracked. Nothing is
//! typed as a command line: each argument is its own field.

use caskade_core::install::ProtonBuilds;
use caskade_core::model::{ActionKind, ActionTracking, GameAction, LaunchSpec};
use caskade_core::tracking::TrackingMode;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::flows::fields::{ListField, PathField};
use crate::flows::paths::{PathRequest, PickKind};
use crate::ui::icons::{PLUS, X};
use crate::ui::{Button, Segment, SegmentedControl, Select, SelectOption, TextField};

/// The Proton select's value for the automatic choice.
const AUTOMATIC: &str = "";

/// Which Proton build a Windows program runs with: the best one installed
/// when it starts, or one the user keeps it on.
#[component]
fn ProtonField(
    value: Option<String>,
    builds: ProtonBuilds,
    on_change: EventHandler<Option<String>>,
    disabled: bool,
) -> Element {
    let mut options = vec![SelectOption::new(
        AUTOMATIC,
        match &builds.automatic {
            Some(automatic) => t!("Automatic ({automatic})", automatic = automatic.clone()),
            None => tp!("Proton build", "Automatic"),
        },
    )];
    options.extend(
        builds
            .installed
            .iter()
            .map(|name| SelectOption::new(name.clone(), name.clone())),
    );
    // A build another device chose, or one since removed, stays shown.
    let missing = value
        .as_ref()
        .filter(|value| !builds.installed.contains(value));
    if let Some(value) = missing {
        options.push(SelectOption::new(
            value.clone(),
            t!("{value} (not installed here)", value = value.clone()),
        ));
    }
    rsx! {
        div { "data-part": "proton", class: "flex flex-col gap-1.5",
            Select {
                label: t!("Proton build"),
                options,
                value: Some(value.clone().unwrap_or_default()),
                disabled,
                on_change: move |next: String| on_change.call((next != AUTOMATIC).then_some(next)),
            }
            p { class: "text-xs text-fg-muted",
                if missing.is_some() {
                    {t!("This device doesn't have it, so the best build installed runs instead.")}
                } else {
                    {t!("Automatic runs the best Proton build installed each time the game starts. Choose a build to keep this action on it.")}
                }
            }
        }
    }
}

/// Environment variables as name and value rows. The rows keep their
/// order while a name is typed; the action holds them by name.
#[component]
fn EnvField(
    env: BTreeMap<String, String>,
    on_change: EventHandler<BTreeMap<String, String>>,
    disabled: bool,
) -> Element {
    let as_rows = |env: &BTreeMap<String, String>| -> Vec<(String, String)> {
        env.iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    };
    let to_map =
        |rows: &[(String, String)]| -> BTreeMap<String, String> { rows.iter().cloned().collect() };
    let mut rows = use_signal(|| as_rows(&env));
    // A change from outside (Revert, another action) starts again.
    if to_map(&rows.peek()) != env {
        rows.set(as_rows(&env));
    }
    let mut set = move |next: Vec<(String, String)>| {
        let map = to_map(&next);
        rows.set(next);
        on_change.call(map);
    };
    let current = rows();
    let adding = current.iter().any(|(name, _)| name.is_empty());
    rsx! {
        fieldset { "data-part": "env", class: "flex flex-col gap-2",
            legend { class: "mb-1.5 text-sm font-medium", {t!("Environment variables")} }
            for (index, (name, value)) in current.iter().cloned().enumerate() {
                div { key: "{index}", class: "flex items-end gap-2",
                    TextField {
                        "aria-label": t!("Variable {0} name", 0 = index + 1),
                        value: name,
                        mono: true,
                        disabled,
                        class: "flex-1",
                        on_input: move |next: String| {
                            let mut rows = rows.peek().clone();
                            rows[index].0 = next;
                            set(rows);
                        },
                    }
                    TextField {
                        "aria-label": t!("Variable {0} value", 0 = index + 1),
                        value,
                        mono: true,
                        disabled,
                        class: "flex-1",
                        on_input: move |next: String| {
                            let mut rows = rows.peek().clone();
                            rows[index].1 = next;
                            set(rows);
                        },
                    }
                    Button {
                        "data-spatial": "focusable",
                        "data-part": "remove",
                        "aria-label": t!("Remove variable {0}", 0 = index + 1),
                        icon: X,
                        disabled,
                        on_press: move |_| {
                            let mut rows = rows.peek().clone();
                            rows.remove(index);
                            set(rows);
                        },
                    }
                }
            }
            Button {
                "data-spatial": "focusable",
                icon: PLUS,
                class: "self-start",
                // An empty name is a new row being filled in.
                disabled: disabled || adding,
                on_press: move |_| {
                    let mut rows = rows.peek().clone();
                    rows.push((String::new(), String::new()));
                    set(rows);
                },
                {t!("Add variable")}
            }
        }
    }
}

/// What kind of target the form edits.
fn kind_of(target: &LaunchSpec) -> Option<&'static str> {
    match target {
        LaunchSpec::Executable { .. } => Some("executable"),
        LaunchSpec::Uri { .. } => Some("uri"),
        LaunchSpec::Flatpak { .. } => Some("flatpak"),
        _ => None,
    }
}

/// `target` as a `kind` target, keeping its arguments.
fn retarget(target: &LaunchSpec, kind: &str) -> LaunchSpec {
    if kind_of(target) == Some(kind) {
        return target.clone();
    }
    let args = match target {
        LaunchSpec::Executable { args, .. } | LaunchSpec::Flatpak { args, .. } => args.clone(),
        _ => Vec::new(),
    };
    match kind {
        "uri" => LaunchSpec::Uri { uri: String::new() },
        "flatpak" => LaunchSpec::Flatpak {
            app_id: String::new(),
            args,
        },
        _ => LaunchSpec::Executable {
            path: PathBuf::new(),
            args,
            cwd: None,
            env: BTreeMap::new(),
        },
    }
}

fn mode_id(mode: &TrackingMode) -> &'static str {
    match mode {
        TrackingMode::Automatic => "automatic",
        TrackingMode::ProcessTree => "process_tree",
        TrackingMode::OriginalProcess => "original_process",
        TrackingMode::Folder => "folder",
        TrackingMode::ProcessName => "process_name",
    }
}

fn mode_of(id: &str) -> TrackingMode {
    match id {
        "process_tree" => TrackingMode::ProcessTree,
        "original_process" => TrackingMode::OriginalProcess,
        "folder" => TrackingMode::Folder,
        "process_name" => TrackingMode::ProcessName,
        _ => TrackingMode::Automatic,
    }
}

fn path_text(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

#[component]
pub fn ActionForm(
    action: GameAction,
    on_change: EventHandler<GameAction>,
    /// The target is the release's source's: only name and tracking change.
    target_locked: bool,
    /// The Proton builds to choose from, when the action runs a Windows
    /// program through Proton.
    #[props(default)]
    proton: Option<ProtonBuilds>,
    #[props(default)] disabled: bool,
) -> Element {
    let locked = disabled || target_locked;
    // `action` with a change.
    let with = {
        let action = action.clone();
        move |change: &dyn Fn(&mut GameAction)| {
            let mut next = action.clone();
            change(&mut next);
            on_change.call(next);
        }
    };
    let set_target = {
        let with = with.clone();
        move |target: LaunchSpec| with(&|action| action.target = target.clone())
    };
    let tracking = action.tracking.clone();
    let set_tracking = {
        let with = with.clone();
        move |tracking: ActionTracking| with(&|action| action.tracking = tracking.clone())
    };
    let modes = vec![
        SelectOption::new("automatic", tp!("game tracking", "Automatic")),
        SelectOption::new("process_tree", t!("Everything it starts")),
        SelectOption::new("original_process", t!("Only the program it starts")),
        SelectOption::new("folder", t!("Programs in a folder")),
        SelectOption::new("process_name", t!("Programs by name")),
    ];
    let mode_note = match tracking.mode {
        TrackingMode::Automatic => {
            t!("Caskade finds the game the usual way: its install folder and what the store says.")
        }
        TrackingMode::ProcessTree => {
            t!("Playtime counts while the program or anything it started runs.")
        }
        TrackingMode::OriginalProcess => {
            t!("Playtime counts only while the program Caskade started runs.")
        }
        TrackingMode::Folder => t!(
            "Playtime counts while a program from this folder runs: for a mod manager, the game's folder."
        ),
        TrackingMode::ProcessName => t!(
            "Playtime counts while a program with one of these names runs: for a mod manager, the game's own program."
        ),
    };
    let target = action.target.clone();
    let argument = tp!("a program's command-line argument", "Argument");
    rsx! {
        div { "data-part": "form", class: "flex flex-col gap-4",
            TextField {
                label: tp!("launch action", "Name"),
                value: action.name.clone(),
                disabled,
                on_input: {
                    let with = with.clone();
                    move |name: String| with(&|action| action.name = name.clone())
                },
            }
            SegmentedControl {
                "aria-label": t!("What it does"),
                options: vec![
                    Segment::new("play", tp!("launch action kind", "Plays the game")),
                    Segment::new("utility", tp!("launch action kind", "Tool")),
                ],
                value: if action.kind == ActionKind::Play { "play" } else { "utility" },
                disabled: locked,
                on_change: {
                    let with = with.clone();
                    move |kind: String| {
                        let kind = if kind == "utility" { ActionKind::Utility } else { ActionKind::Play };
                        with(&|action| action.kind = kind)
                    }
                },
            }
            if action.kind == ActionKind::Utility {
                p { class: "text-xs text-fg-muted",
                    {t!("A tool runs from the game's menu. It counts no playtime and doesn't change when the game was last played.")}
                }
            }
            if target_locked {
                p { "data-part": "locked", class: "text-xs text-fg-muted",
                    {t!("This launch comes from where the game was found, which keeps it up to date. Duplicate it to change what it starts.")}
                }
            }
            if let Some(kind) = kind_of(&target) {
                Select {
                    label: t!("Starts"),
                    options: vec![
                        SelectOption::new("executable", t!("A program")),
                        SelectOption::new("uri", t!("A launcher link")),
                        SelectOption::new("flatpak", t!("A Flatpak app")),
                    ],
                    value: Some(kind.to_owned()),
                    disabled: locked,
                    on_change: {
                        let (set_target, target) = (set_target.clone(), target.clone());
                        move |kind: String| set_target(retarget(&target, &kind))
                    },
                }
            } else {
                p { "data-part": "target-kind", class: "text-sm text-fg-muted",
                    match &target {
                        LaunchSpec::Emulator { .. } => t!("Starts the game in its emulator."),
                        LaunchSpec::Web { .. } => t!("Opens the web game in its game window."),
                        _ => t!("Starts the game through a plugin."),
                    }
                }
            }
            match target.clone() {
                LaunchSpec::Executable { path, args, cwd, env } => {
                    let (on_path, on_args, on_cwd, on_env) = {
                        let make = || (set_target.clone(), target.clone());
                        (make(), make(), make(), make())
                    };
                    rsx! {
                        PathField {
                            label: t!("Program"),
                            description: t!("A program or script file. Inside a Caskade install, a path relative to its folder moves with it."),
                            value: path_text(&path),
                            request: PathRequest::new(PickKind::File, t!("Choose the program")),
                            disabled: locked,
                            on_change: move |next: String| {
                                let (set, target) = &on_path;
                                if let LaunchSpec::Executable { args, cwd, env, .. } = target.clone() {
                                    set(LaunchSpec::Executable { path: next.into(), args, cwd, env });
                                }
                            },
                        }
                        ListField {
                            label: argument.clone(),
                            values: args,
                            add_label: t!("Add argument"),
                            disabled: locked,
                            on_change: move |next: Vec<String>| {
                                let (set, target) = &on_args;
                                if let LaunchSpec::Executable { path, cwd, env, .. } = target.clone() {
                                    set(LaunchSpec::Executable { path, args: next, cwd, env });
                                }
                            },
                        }
                        PathField {
                            label: t!("Working folder"),
                            description: t!("Empty: the game's install folder."),
                            value: cwd.as_deref().map(path_text).unwrap_or_default(),
                            request: PathRequest::new(PickKind::Folder, t!("Choose the working folder")),
                            disabled: locked,
                            on_change: move |next: String| {
                                let (set, target) = &on_cwd;
                                if let LaunchSpec::Executable { path, args, env, .. } = target.clone() {
                                    let cwd = (!next.is_empty()).then(|| PathBuf::from(next));
                                    set(LaunchSpec::Executable { path, args, cwd, env });
                                }
                            },
                        }
                        EnvField {
                            env,
                            disabled: locked,
                            on_change: move |next: BTreeMap<String, String>| {
                                let (set, target) = &on_env;
                                if let LaunchSpec::Executable { path, args, cwd, .. } = target.clone() {
                                    set(LaunchSpec::Executable { path, args, cwd, env: next });
                                }
                            },
                        }
                    }
                }
                LaunchSpec::Uri { uri } => {
                    let set = set_target.clone();
                    rsx! {
                        TextField {
                            label: t!("Link"),
                            description: t!("A game launcher's link, like steam://rungameid/620."),
                            value: uri,
                            mono: true,
                            disabled: locked,
                            on_input: move |uri: String| set(LaunchSpec::Uri { uri }),
                        }
                    }
                }
                LaunchSpec::Flatpak { app_id, args } => {
                    let (on_id, on_args) = ((set_target.clone(), args.clone()), (set_target.clone(), app_id.clone()));
                    rsx! {
                        TextField {
                            label: t!("Flatpak app id"),
                            description: t!("Like org.example.Game."),
                            value: app_id,
                            mono: true,
                            disabled: locked,
                            on_input: move |app_id: String| {
                                let (set, args) = &on_id;
                                set(LaunchSpec::Flatpak { app_id, args: args.clone() });
                            },
                        }
                        ListField {
                            label: argument.clone(),
                            values: args,
                            add_label: t!("Add argument"),
                            disabled: locked,
                            on_change: move |args: Vec<String>| {
                                let (set, app_id) = &on_args;
                                set(LaunchSpec::Flatpak { app_id: app_id.clone(), args });
                            },
                        }
                    }
                }
                _ => rsx! {},
            }
            if let Some(builds) = proton {
                ProtonField {
                    value: action.proton.clone(),
                    builds,
                    disabled,
                    on_change: {
                        let with = with.clone();
                        move |build: Option<String>| with(&|action| action.proton = build.clone())
                    },
                }
            }
            if action.kind == ActionKind::Play {
                fieldset { "data-part": "tracking", class: "flex flex-col gap-3",
                    Select {
                        label: t!("Which programs are the game"),
                        options: modes,
                        value: Some(mode_id(&tracking.mode).to_owned()),
                        disabled,
                        on_change: {
                            let (set, tracking) = (set_tracking.clone(), tracking.clone());
                            move |id: String| set(ActionTracking { mode: mode_of(&id), ..tracking.clone() })
                        },
                    }
                    p { class: "text-xs text-fg-muted", "{mode_note}" }
                    if tracking.mode == TrackingMode::Folder {
                        PathField {
                            label: t!("Folder"),
                            description: t!("Empty: the game's install folder."),
                            value: tracking.folder.as_deref().map(path_text).unwrap_or_default(),
                            request: PathRequest::new(PickKind::Folder, t!("Choose the game's folder")),
                            disabled,
                            on_change: {
                                let (set, tracking) = (set_tracking.clone(), tracking.clone());
                                move |folder: String| set(ActionTracking {
                                    folder: (!folder.is_empty()).then(|| PathBuf::from(folder)),
                                    ..tracking.clone()
                                })
                            },
                        }
                    }
                    if tracking.mode == TrackingMode::ProcessName {
                        ListField {
                            label: t!("Program name"),
                            values: tracking.names.clone(),
                            add_label: t!("Add program name"),
                            disabled,
                            on_change: {
                                let (set, tracking) = (set_tracking.clone(), tracking.clone());
                                move |names: Vec<String>| set(ActionTracking { names, ..tracking.clone() })
                            },
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_kind_of_target_keeps_the_arguments() {
        let flatpak = LaunchSpec::Flatpak {
            app_id: "org.example.Game".into(),
            args: vec!["--fast".into()],
        };
        match retarget(&flatpak, "executable") {
            LaunchSpec::Executable { args, path, .. } => {
                assert_eq!(args, vec!["--fast".to_owned()]);
                assert_eq!(path, PathBuf::new());
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(retarget(&flatpak, "flatpak"), flatpak);
    }
}

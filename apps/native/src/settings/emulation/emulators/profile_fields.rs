//! A custom profile's own fields: its platforms and file extensions.

use super::*;

/// "nes, .SFC, , <none>" → `["nes", "sfc", "<none>"]`, once each.
pub(super) fn parse_extensions(text: &str) -> Vec<String> {
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
pub(super) fn CustomFields(
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

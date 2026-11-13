//! Adding an emulator by hand, or changing one's install (the webview's
//! `ConfiguredEmulators` form).

use super::*;

pub(super) fn empty_install(kind: &str) -> EmulatorInstall {
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

pub(super) fn install_kind(install: &EmulatorInstall) -> &'static str {
    match install {
        EmulatorInstall::Folder { .. } => "folder",
        EmulatorInstall::Path { .. } => "path",
        EmulatorInstall::Flatpak { .. } => "flatpak",
        EmulatorInstall::AppImage { .. } => "app_image",
        EmulatorInstall::AppBundle { .. } => "app_bundle",
    }
}

pub(super) fn install_kinds() -> Vec<SelectOption> {
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

/// An emulator's name, which emulator it is and its install; `emulator`
/// absent adds one.
#[component]
pub(super) fn EmulatorForm(
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

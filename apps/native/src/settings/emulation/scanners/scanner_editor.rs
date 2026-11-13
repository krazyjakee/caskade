//! Adding or editing a ROM scanner: its folder, platforms, profile and
//! what it reads.

use super::*;

/// Saves an editor's draft; whether it saved.
pub(super) type Save = Rc<dyn Fn() -> Pin<Box<dyn Future<Output = bool>>>>;

/// Extensions of playlists, which absorb the files they list.
pub(super) const PLAYLIST_TYPES: [&str; 3] = ["m3u", "cue", "gdi"];

/// A scanner's switches.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Toggle {
    Recursive,
    ScanArchives,
    SkipCloudPlaceholders,
    InLibraryUpdate,
}

pub(super) const TOGGLES: [Toggle; 4] = [
    Toggle::Recursive,
    Toggle::ScanArchives,
    Toggle::SkipCloudPlaceholders,
    Toggle::InLibraryUpdate,
];

impl Toggle {
    fn id(self) -> &'static str {
        match self {
            Self::Recursive => "scanner-recursive",
            Self::ScanArchives => "scanner-archives",
            Self::SkipCloudPlaceholders => "scanner-skip-cloud",
            Self::InLibraryUpdate => "scanner-library-update",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Recursive => FOLDER_TREE,
            Self::ScanArchives => ARCHIVE,
            Self::SkipCloudPlaceholders => CLOUD_OFF,
            Self::InLibraryUpdate => REFRESH_CW,
        }
    }

    fn label(self) -> String {
        match self {
            Self::Recursive => t!("Scan subfolders"),
            Self::ScanArchives => t!("Look inside archives"),
            Self::SkipCloudPlaceholders => t!("Skip cloud-only files"),
            Self::InLibraryUpdate => t!("Include in library update"),
        }
    }

    fn description(self) -> String {
        match self {
            Self::Recursive => t!("Look in every folder below the scan folder."),
            Self::ScanArchives => {
                t!("Find ROMs inside zip and 7z files the profile can't open directly.")
            }
            Self::SkipCloudPlaceholders => {
                t!("Don't download OneDrive, Dropbox or iCloud placeholders to scan them.")
            }
            Self::InLibraryUpdate => {
                t!("Import new ROMs whenever the library updates, without a review.")
            }
        }
    }

    fn get(self, scanner: &RomScanner) -> bool {
        match self {
            Self::Recursive => scanner.recursive,
            Self::ScanArchives => scanner.scan_archives,
            Self::SkipCloudPlaceholders => scanner.skip_cloud_placeholders,
            Self::InLibraryUpdate => scanner.in_library_update,
        }
    }

    fn set(self, scanner: &mut RomScanner, on: bool) {
        match self {
            Self::Recursive => scanner.recursive = on,
            Self::ScanArchives => scanner.scan_archives = on,
            Self::SkipCloudPlaceholders => scanner.skip_cloud_placeholders = on,
            Self::InLibraryUpdate => scanner.in_library_update = on,
        }
    }
}

/// What a scanner's games launch with, by its `type`.
pub(super) const LAUNCH_MODES: [&str; 4] =
    ["platform_default", "profile", "ask_profile", "ask_emulator"];

/// One scanner's settings, saved with its Save button.
#[component]
pub(super) fn ScannerEditor(
    initial: RomScanner,
    /// The saved scanner's row; none for a new one.
    #[props(default)]
    row: Option<ScannerView>,
    snapshot: Snapshot,
    on_saved: EventHandler<RomScanner>,
    on_removed: EventHandler<()>,
) -> Element {
    let emulation = use_emulation();
    let mut draft = use_signal(|| initial.clone());
    let mut base = use_signal(|| initial.clone());
    // A review or the include switch changes the saved scanner; follow it
    // unless the draft has edits of its own.
    use_effect(use_reactive!(|initial| {
        if *base.peek() != initial {
            if *draft.peek() == *base.peek() {
                draft.set(initial.clone());
            }
            base.set(initial);
        }
    }));
    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| None::<ErrorReport>);
    let mut confirming = use_signal(|| false);
    let mut removing = use_signal(|| false);
    let mut remove_error = use_signal(|| None::<ErrorReport>);
    let created = initial.id.is_empty();
    let now = draft();
    let options = profile_options(&snapshot);
    let profile = options
        .iter()
        .find(|option| option.id == profile_ref(&now.profile.emulator_id, &now.profile.profile_id))
        .cloned();
    let title = if created {
        t!("New ROM scanner")
    } else if now.name.is_empty() {
        t!("Unnamed scanner")
    } else {
        now.name.clone()
    };
    let dirty = now != base();
    let save: Save = {
        let emulation = emulation.clone();
        Rc::new(move || {
            let emulation = emulation.clone();
            let next = draft.peek().clone();
            Box::pin(async move {
                saving.set(true);
                save_error.set(None);
                let saved = change(async move { emulation.save_scanner(next).await }).await;
                saving.set(false);
                match saved {
                    Ok(scanner) => {
                        on_saved.call(scanner);
                        true
                    }
                    Err(report) => {
                        save_error.set(Some(report));
                        false
                    }
                }
            }) as Pin<Box<dyn Future<Output = bool>>>
        })
    };
    // The scanner waits for its Save button (#43): leaving the section asks
    // first.
    let has_profile = profile.is_some();
    use_unsaved_changes(
        dirty,
        UnsavedForm {
            label: title.clone(),
            save: {
                let save = save.clone();
                Rc::new(move || {
                    let save = save.clone();
                    Box::pin(async move { has_profile && save().await })
                        as Pin<Box<dyn Future<Output = bool>>>
                })
            },
            discard: Rc::new(move || {
                let mut draft = draft;
                draft.set(base.peek().clone());
            }),
        },
    );
    let remove = {
        let emulation = emulation.clone();
        let id = initial.id.clone();
        move |_| {
            removing.set(true);
            remove_error.set(None);
            let emulation = emulation.clone();
            let id = id.clone();
            spawn(async move {
                let removed = change(async move { emulation.delete_scanner(&id).await }).await;
                removing.set(false);
                match removed {
                    Ok(()) => {
                        confirming.set(false);
                        on_removed.call(());
                    }
                    Err(report) => remove_error.set(Some(report)),
                }
            });
        }
    };
    let profile_choices: Vec<SelectOption> = options
        .iter()
        .map(|option| SelectOption::new(option.id.clone(), option.label.clone()))
        .collect();
    let from_profile = match &profile {
        Some(profile) if !profile.platforms.is_empty() => t!(
            "From profile ({platforms})",
            platforms = profile
                .platforms
                .iter()
                .map(|id| snapshot.platform_short(id))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => t!("From profile"),
    };
    let platform_choices: Vec<SelectOption> =
        std::iter::once(SelectOption::new("profile", from_profile))
            .chain(
                profile
                    .iter()
                    .flat_map(|profile| profile.platforms.iter())
                    .map(|id| SelectOption::new(id.clone(), snapshot.platform_name(id))),
            )
            .collect();
    let launch_choices: Vec<SelectOption> = LAUNCH_MODES
        .iter()
        .map(|mode| SelectOption::new(*mode, launch_mode_label(mode)))
        .collect();
    let playlists = profile.as_ref().is_some_and(|profile| {
        profile
            .extensions
            .iter()
            .any(|extension| PLAYLIST_TYPES.contains(&extension.as_str()))
    });
    let flatpak_hint = row.as_ref().and_then(|row| row.flatpak_hint.clone());
    let problem = row.as_ref().and_then(|row| row.problem.clone());
    let submit = {
        let save = save.clone();
        move |_| {
            let save = save.clone();
            spawn(async move {
                save().await;
            });
        }
    };
    rsx! {
        SettingsSection {
            icon: FOLDER_SEARCH,
            title: title.clone(),
            description: t!("The profile decides which files match. Imported games launch with the choice below."),
            aside: (!created).then(|| rsx! {
                Button {
                    id: "scanner-remove",
                    variant: ButtonVariant::Danger,
                    size: ButtonSize::Sm,
                    icon: TRASH,
                    disabled: removing(),
                    on_press: move |_| {
                        remove_error.set(None);
                        confirming.set(true);
                    },
                    {t!(note = "Remove the ROM scanner", "Remove…")}
                }
            }),
            div { class: "mt-3 grid gap-4",
                TextField {
                    id: "scanner-name",
                    label: t!("Scanner name"),
                    value: now.name.clone(),
                    on_input: move |name: String| draft.write().name = name,
                }
                PathField {
                    id: "scanner-folder",
                    label: t!("ROM folder"),
                    value: now.folder.to_string_lossy().into_owned(),
                    on_change: move |folder: String| draft.write().folder = PathBuf::from(folder),
                }
                Select {
                    id: "scanner-profile",
                    label: t!("Emulator profile"),
                    placeholder: t!("Choose a profile"),
                    options: profile_choices,
                    value: profile.as_ref().map(|profile| profile.id.clone()),
                    disabled: options.is_empty(),
                    on_change: {
                        let options = options.clone();
                        move |id: String| {
                            let Some(next) = options.iter().find(|option| option.id == id) else {
                                return;
                            };
                            let mut scanner = draft.peek().clone();
                            let mode = launch_mode(&scanner.launch_with);
                            scanner.platform = None;
                            scanner.profile = ProfileRef {
                                emulator_id: next.emulator_id.clone(),
                                profile_id: next.profile_id.clone(),
                            };
                            scanner.launch_with = launch_with(mode, &scanner);
                            draft.set(scanner);
                        }
                    },
                }
                Select {
                    id: "scanner-platform",
                    label: t!(note = "A ROM scanner's platform", "Platform"),
                    options: platform_choices,
                    value: Some(now.platform.clone().unwrap_or_else(|| "profile".to_owned())),
                    on_change: move |platform: String| {
                        draft.write().platform = (platform != "profile").then_some(platform);
                    },
                }
                Select {
                    id: "scanner-launch",
                    label: t!("Games launch with"),
                    options: launch_choices,
                    value: Some(launch_mode(&now.launch_with).to_owned()),
                    on_change: move |mode: String| {
                        let next = launch_with(&mode, &draft.peek());
                        draft.write().launch_with = next;
                    },
                }
            }
            if options.is_empty() {
                p { class: "mt-3 text-sm text-fg-muted",
                    {t!("Add an emulator with a profile first; its file types decide what a scanner finds.")}
                }
            }
            if let Some(profile) = profile.clone() {
                div { class: "mt-3 flex flex-wrap items-center gap-1.5 text-xs text-fg-muted",
                    {t!(note = "Followed by the file extensions the scanner's profile matches", "Matches")}
                    for extension in profile.extensions.iter() {
                        span {
                            key: "{extension}",
                            "data-part": "extension",
                            class: "rounded-control border border-border px-1.5 py-0.5 font-mono",
                            {format!(".{extension}")}
                        }
                    }
                    if playlists {
                        span {
                            {format!("· {}", t!("Playlists absorb the files they list, so disc tracks aren't separate games."))}
                        }
                    }
                }
            }
            if let Some(hint) = flatpak_hint {
                p {
                    "data-part": "flatpak-hint",
                    class: "mt-3 flex gap-2 rounded-control border border-accent-alt/30 bg-accent-alt/5 p-3 text-sm text-fg-muted",
                    Svg { icon: INFO, class: "mt-0.5 size-4 shrink-0 text-accent-alt" }
                    "{hint}"
                }
            }
            if let Some(problem) = problem {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{problem}" }
            }
            div { class: "mt-4",
                for (id, toggle) in TOGGLES.map(|toggle| (toggle.id(), toggle)) {
                    SettingRow {
                        key: "{id}",
                        icon: toggle.icon(),
                        label: toggle.label(),
                        description: toggle.description(),
                        Switch {
                            id: "{id}",
                            checked: toggle.get(&now),
                            on_change: move |on: bool| toggle.set(&mut draft.write(), on),
                        }
                    }
                }
            }
            div { class: "mt-4 grid gap-4",
                div {
                    h3 { class: "text-sm font-medium", {t!("Exclusions")} }
                    p { class: "text-xs text-fg-muted",
                        {t!("Files and folders this scanner always skips. Unticking a game in a review adds it here.")}
                    }
                    ExclusionList {
                        patterns: now.exclusions.clone(),
                        on_change: move |patterns: Vec<String>| draft.write().exclusions = patterns,
                    }
                }
                ExclusionSyntax {}
            }
            if let Some(report) = save_error() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
            }
            if confirming() {
                ConfirmDialog {
                    title: t!("Remove {scanner}?", scanner = title.clone()),
                    confirm_label: t!(note = "Confirms removing a ROM scanner", "Remove"),
                    pending_label: t!("Removing…"),
                    pending: removing(),
                    on_confirm: remove,
                    on_close: move |_| confirming.set(false),
                    p {
                        {t!("Its games stay in your library as not installed, because Caskade no longer looks for their ROMs. Your ROM files aren't touched.")}
                    }
                    if let Some(report) = remove_error() {
                        FailureNotice { error: report, variant: NoticeVariant::Inline }
                    }
                }
            }
            div { class: "mt-4 flex items-center justify-end gap-2",
                div { class: "mr-auto",
                    UnsavedHint { dirty }
                }
                Button {
                    id: "scanner-save",
                    variant: ButtonVariant::Primary,
                    disabled: saving() || profile.is_none(),
                    on_press: submit,
                    if saving() {
                        {t!("Saving…")}
                    } else if created {
                        {t!("Save and scan")}
                    } else {
                        {t!("Save scanner")}
                    }
                }
            }
        }
    }
}

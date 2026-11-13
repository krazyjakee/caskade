//! Adding or editing a profile, with a live preview of the command it
//! runs (the webview's `ProfileEditor`).

use super::*;

/// Display and edit quoting only, for tokens that need it; the core splits
/// and runs them.
pub fn argv_text(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if !arg.is_empty()
                && !arg
                    .chars()
                    .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '\\')
            {
                arg.clone()
            } else {
                format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A profile as its editor holds it: the profile, and its arguments and
/// hooks as typed. The core splits the text when it's saved or previewed.
#[derive(Clone, PartialEq)]
pub struct ProfileEdit {
    pub profile: EmulatorProfile,
    pub args_text: String,
    pub before_text: String,
    pub after_exit_text: String,
}

impl ProfileEdit {
    pub fn of(profile: &EmulatorProfile) -> Self {
        let (args, hooks) = match profile {
            EmulatorProfile::Custom { args, hooks, .. } => (args.clone(), hooks),
            EmulatorProfile::BuiltIn { args, hooks, .. } => (args.args().to_vec(), hooks),
        };
        Self {
            profile: profile.clone(),
            args_text: argv_text(&args),
            before_text: argv_text(hooks.before.as_deref().unwrap_or_default()),
            after_exit_text: argv_text(hooks.after_exit.as_deref().unwrap_or_default()),
        }
    }

    pub(super) fn built_in(name: &str) -> Self {
        Self {
            profile: EmulatorProfile::built_in(name),
            args_text: String::new(),
            before_text: String::new(),
            after_exit_text: String::new(),
        }
    }

    pub(super) fn custom() -> Self {
        Self {
            profile: EmulatorProfile::Custom {
                id: String::new(),
                name: t!("Custom profile"),
                executable: None,
                args: Vec::new(),
                working_dir: None,
                platforms: Vec::new(),
                extensions: Vec::new(),
                env: Vec::new(),
                tracking: Tracking::Default,
                hooks: ProfileHooks::default(),
            },
            args_text: "\"{RomPath}\"".to_owned(),
            before_text: String::new(),
            after_exit_text: String::new(),
        }
    }

    /// The profile with its typed arguments and hooks split, as the core
    /// will save it.
    fn parsed(&self) -> Result<EmulatorProfile, ErrorReport> {
        let args = Emulation::split_args(&self.args_text)?;
        let before = Emulation::split_args(&self.before_text)?;
        let after_exit = Emulation::split_args(&self.after_exit_text)?;
        let hooks = ProfileHooks {
            before: (!before.is_empty()).then_some(before),
            after_exit: (!after_exit.is_empty()).then_some(after_exit),
        };
        Ok(match self.profile.clone() {
            EmulatorProfile::Custom {
                id,
                name,
                executable,
                working_dir,
                platforms,
                extensions,
                env,
                tracking,
                ..
            } => EmulatorProfile::Custom {
                id,
                name,
                executable,
                args,
                working_dir,
                platforms,
                extensions,
                env,
                tracking,
                hooks,
            },
            EmulatorProfile::BuiltIn {
                id,
                definition_profile,
                args: old,
                ..
            } => EmulatorProfile::BuiltIn {
                id,
                definition_profile,
                hooks,
                args: match old {
                    ArgsOverride::Default => ArgsOverride::Default,
                    ArgsOverride::Append { .. } => ArgsOverride::Append { args },
                    // An empty `replaced` keeps the stored one; the editor
                    // sets it when the user reviews a drift.
                    ArgsOverride::Replace { replaced, .. } => {
                        ArgsOverride::Replace { args, replaced }
                    }
                },
            },
        })
    }
}

/// How playtime is tracked, by id.
pub(super) fn tracking_modes() -> Vec<SelectOption> {
    vec![
        SelectOption::new("default", tp!("playtime tracking", "Default")),
        SelectOption::new("process_tree", tp!("playtime tracking", "Process tree")),
        SelectOption::new(
            "original_process",
            tp!("playtime tracking", "Original process only"),
        ),
        SelectOption::new("folder", tp!("playtime tracking", "Any process in folder")),
        SelectOption::new("process_name", tp!("playtime tracking", "Process name")),
    ]
}

pub(super) fn tracking_id(tracking: &Tracking) -> &'static str {
    match tracking {
        Tracking::Default => "default",
        Tracking::ProcessTree => "process_tree",
        Tracking::OriginalProcess => "original_process",
        Tracking::Folder { .. } => "folder",
        Tracking::ProcessName { .. } => "process_name",
    }
}

pub(super) fn args_mode(args: &ArgsOverride) -> &'static str {
    match args {
        ArgsOverride::Default => "default",
        ArgsOverride::Replace { .. } => "replace",
        ArgsOverride::Append { .. } => "append",
    }
}

/// Adds or changes one of `emulator`'s profiles.
#[component]
pub(super) fn ProfileEditor(
    emulator: EmulatorView,
    initial: ProfileEdit,
    snapshot: Snapshot,
    on_close: EventHandler<()>,
) -> Element {
    let emulation = use_emulation();
    let mut draft = use_signal(|| initial.clone());
    let mut extensions = use_signal(|| match &initial.profile {
        EmulatorProfile::Custom { extensions, .. } => extensions.join(", "),
        _ => String::new(),
    });
    let mut saving = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let mut preview =
        use_signal(|| None::<Result<caskade_core::emulation::ArgvPreview, ErrorReport>>);
    let mut previewing = use_signal(|| false);
    let dirty = draft() != initial;
    let current = draft();
    let profile = current.profile.clone();
    let title = match &profile {
        EmulatorProfile::Custom { .. } => t!("Custom profile"),
        EmulatorProfile::BuiltIn {
            definition_profile, ..
        } => definition_profile.clone(),
    };
    // The command line, again 200 ms after the last change.
    let mut generation = use_signal(|| 0u64);
    {
        let emulation = emulation.clone();
        let (install, definition) = (
            emulator.emulator.install.clone(),
            emulator.emulator.definition.clone(),
        );
        use_effect(use_reactive!(|current| {
            *generation.write() += 1;
            let mine = *generation.peek();
            let emulation = emulation.clone();
            let (install, definition) = (install.clone(), definition.clone());
            spawn(async move {
                sleep(Duration::from_millis(200)).await;
                if *generation.peek() != mine {
                    return;
                }
                previewing.set(true);
                let shown = current.parsed().map(|profile| {
                    emulation.preview(&ProfileDraft {
                        install,
                        definition,
                        profile,
                    })
                });
                previewing.set(false);
                preview.set(Some(shown));
            });
        }));
    }
    let save = {
        let emulation = emulation.clone();
        let emulator_id = emulator.emulator.id.clone();
        std::rc::Rc::new(move || {
            let emulation = emulation.clone();
            let emulator_id = emulator_id.clone();
            let edit = draft.peek().clone();
            Box::pin(async move {
                saving.set(true);
                failure.set(None);
                let saved = change(async move {
                    let parsed = edit.parsed()?;
                    let mut stored = emulation
                        .emulators()
                        .await?
                        .into_iter()
                        .find(|row| row.emulator.id == emulator_id)
                        .map(|row| row.emulator)
                        .ok_or_else(gone)?;
                    match stored
                        .profiles
                        .iter_mut()
                        .find(|profile| !parsed.id().is_empty() && profile.id() == parsed.id())
                    {
                        Some(profile) => *profile = parsed,
                        None => stored.profiles.push(parsed),
                    }
                    emulation.save_emulator(stored).await
                })
                .await;
                saving.set(false);
                match saved {
                    Ok(_) => {
                        on_close.call(());
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
    // The profile waits for Save profile (#43): leaving the section asks
    // first.
    use_unsaved_changes(
        dirty,
        UnsavedForm {
            label: format!("{}: {title}", emulator.emulator.name),
            save: save.clone(),
            discard: std::rc::Rc::new({
                let initial = initial.clone();
                move || {
                    let (mut draft, mut extensions) = (draft, extensions);
                    extensions.set(match &initial.profile {
                        EmulatorProfile::Custom { extensions, .. } => extensions.join(", "),
                        _ => String::new(),
                    });
                    draft.set(initial.clone());
                }
            }),
        },
    );
    let state = emulator
        .profiles
        .iter()
        .find(|row| !profile.id().is_empty() && row.id == profile.id())
        .cloned();
    // Cleared once the user picks how to resolve it; saving then records
    // the reviewed arguments.
    let drift = state.as_ref().and_then(|row| row.drift.clone()).filter(|drift| {
        !matches!(&profile, EmulatorProfile::BuiltIn { args: ArgsOverride::Replace { replaced, .. }, .. } if *replaced == drift.definition_args)
    });
    let definition = snapshot
        .catalog
        .definitions
        .iter()
        .find(|row| Some(&row.id) == emulator.emulator.definition.as_ref())
        .cloned();
    let definition_profile = match &profile {
        EmulatorProfile::BuiltIn {
            definition_profile, ..
        } => definition.as_ref().and_then(|definition| {
            definition
                .profiles
                .iter()
                .find(|row| &row.name == definition_profile)
                .cloned()
        }),
        _ => None,
    };
    let mut variables: Vec<(String, String)> = snapshot
        .catalog
        .variables
        .iter()
        .flat_map(|row| {
            std::iter::once((row.name.clone(), row.description.clone())).chain(
                row.aliases
                    .iter()
                    .map(|alias| (alias.clone(), format!("Alias for {{{}}}", row.name))),
            )
        })
        .collect();
    for name in definition
        .as_ref()
        .map(|d| d.vars.clone())
        .unwrap_or_default()
    {
        if !variables.iter().any(|(known, _)| *known == name) {
            variables.push((name, t!("Set by Caskade for this emulator")));
        }
    }
    let mut set_profile = move |next: EmulatorProfile| draft.write().profile = next;
    let shows_args = match &profile {
        EmulatorProfile::Custom { .. } => true,
        EmulatorProfile::BuiltIn { args, .. } => *args != ArgsOverride::Default,
    };
    let args_label = match &profile {
        EmulatorProfile::Custom { .. } => t!("Arguments"),
        EmulatorProfile::BuiltIn {
            args: ArgsOverride::Replace { .. },
            ..
        } => t!("Replacement arguments"),
        _ => t!("Extra arguments"),
    };
    let no_args = t!(note = "No arguments", "(none)");
    let shown_preview = match preview() {
        Some(Ok(preview)) => Some(preview),
        _ => state.as_ref().map(|row| row.preview.clone()),
    };
    let preview_error = match preview() {
        Some(Err(report)) => Some(report.summary),
        _ => None,
    };
    rsx! {
        SettingsSection {
            title: title.clone(),
            description: t!("Caskade checks the profile when you save. Nothing here starts the emulator."),
            aside: rsx! {
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    icon: X,
                    "aria-label": t!("Close profile editor"),
                    disabled: saving(),
                    on_press: move |_| on_close.call(()),
                }
            },
            div { class: "mt-4 space-y-4",
                match profile.clone() {
                    EmulatorProfile::Custom { .. } => rsx! {
                        CustomFields {
                            profile: profile.clone(),
                            extensions: extensions(),
                            snapshot: snapshot.clone(),
                            on_change: move |next: EmulatorProfile| set_profile(next),
                            on_extensions: move |text: String| {
                                let parsed = parse_extensions(&text);
                                extensions.set(text);
                                if let EmulatorProfile::Custom { extensions, .. } = &mut draft.write().profile {
                                    *extensions = parsed;
                                }
                            },
                        }
                    },
                    EmulatorProfile::BuiltIn { args, .. } => rsx! {
                        p { class: "text-sm text-fg-muted",
                            {
                                let names = definition_profile
                                    .as_ref()
                                    .map(|row| {
                                        row.platforms
                                            .iter()
                                            .map(|id| snapshot.platform_name(id))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                    .unwrap_or_default();
                                t!(
                                    note = "An emulator profile's platforms",
                                    "Platforms: {platforms}",
                                    platforms = if names.is_empty() { t!("Unknown built-in profile") } else { names }
                                )
                            }
                        }
                        p { class: "font-mono text-xs text-fg-muted",
                            {
                                let extensions = definition_profile.as_ref().map(|row| row.extensions.join(", ")).unwrap_or_default();
                                t!(
                                    note = "The file extensions an emulator profile opens",
                                    "File types: {extensions}",
                                    extensions = if extensions.is_empty() { tp!("file extensions", "None") } else { extensions }
                                )
                            }
                        }
                        if let Some(row) = definition_profile.clone().filter(|row| !row.requires.is_empty()) {
                            ul { class: "space-y-1 text-xs text-fg-muted",
                                for path in row.requires {
                                    li { key: "{path}",
                                        {t!(note = "A file an emulator profile needs", "Requires: {path}", path = path.clone())}
                                    }
                                }
                            }
                        }
                        SegmentedControl {
                            "aria-label": t!("Arguments mode"),
                            options: vec![
                                Segment::new("default", tp!(note = "Use the specification's arguments", "emulator arguments", "Default")),
                                Segment::new("replace", tp!(note = "Replace the specification's arguments", "emulator arguments", "Replace")),
                                Segment::new("append", tp!(note = "Add arguments after the specification's", "emulator arguments", "Append")),
                            ],
                            value: args_mode(&args),
                            on_change: {
                                let definition_args = definition_profile.as_ref().map(|row| row.args.clone()).unwrap_or_default();
                                move |mode: String| {
                                    let mut next = draft.write();
                                    // Replace starts from the specification's
                                    // arguments, so it is an edit.
                                    if mode == "replace" && next.args_text.trim().is_empty() {
                                        next.args_text = argv_text(&definition_args);
                                    }
                                    if let EmulatorProfile::BuiltIn { args, .. } = &mut next.profile {
                                        *args = match mode.as_str() {
                                            "replace" => ArgsOverride::Replace { args: Vec::new(), replaced: Vec::new() },
                                            "append" => ArgsOverride::Append { args: Vec::new() },
                                            _ => ArgsOverride::Default,
                                        };
                                    }
                                }
                            },
                        }
                        if let Some(drift) = drift.clone().filter(|_| matches!(args, ArgsOverride::Replace { .. })) {
                            div {
                                role: "status",
                                "data-part": "drift",
                                class: "space-y-2 rounded-control border border-warning/40 p-3 text-sm text-warning",
                                p { {t!("Caskade's arguments for this emulator changed since you replaced them.")} }
                                p { class: "break-all font-mono text-xs text-fg-muted",
                                    {
                                        let then = argv_text(&drift.overridden_from);
                                        t!(note = "The arguments a Replace override was saved with", "Replaced then: {args}", args = if then.is_empty() { no_args.clone() } else { then })
                                    }
                                }
                                p { class: "break-all font-mono text-xs text-fg-muted",
                                    {
                                        let now = argv_text(&drift.definition_args);
                                        t!(note = "The arguments Caskade's definition of the emulator gives now", "Caskade's now: {args}", args = if now.is_empty() { no_args.clone() } else { now })
                                    }
                                }
                                div { class: "flex flex-wrap gap-2",
                                    Button {
                                        size: ButtonSize::Sm,
                                        on_press: {
                                            let drift = drift.clone();
                                            move |_| {
                                                let mut next = draft.write();
                                                next.args_text = argv_text(&drift.definition_args);
                                                if let EmulatorProfile::BuiltIn { args, .. } = &mut next.profile {
                                                    *args = ArgsOverride::Replace { args: Vec::new(), replaced: drift.definition_args.clone() };
                                                }
                                            }
                                        },
                                        {t!("Start from the new arguments")}
                                    }
                                    Button {
                                        size: ButtonSize::Sm,
                                        on_press: {
                                            let drift = drift.clone();
                                            move |_| {
                                                if let EmulatorProfile::BuiltIn { args: ArgsOverride::Replace { replaced, .. }, .. } = &mut draft.write().profile {
                                                    *replaced = drift.definition_args.clone();
                                                }
                                            }
                                        },
                                        {t!("Keep my arguments")}
                                    }
                                }
                            }
                        }
                    },
                }
                if shows_args {
                    TextField {
                        id: "profile-args",
                        label: args_label,
                        mono: true,
                        value: current.args_text.clone(),
                        description: t!("Split into arguments as a shell would, but no shell runs. Quote a value that has spaces."),
                        on_input: move |text: String| draft.write().args_text = text,
                    }
                    div { "data-part": "variables", class: "flex flex-wrap gap-1.5",
                        for (name, description) in variables {
                            Button {
                                key: "{name}",
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Ghost,
                                "aria-label": t!(
                                    note = "Inserts a launch variable into the arguments",
                                    "Insert {variable}: {description}",
                                    variable = format!("{{{name}}}"),
                                    description = description.clone()
                                ),
                                on_press: {
                                    let name = name.clone();
                                    move |_| {
                                        let mut next = draft.write();
                                        let trimmed = next.args_text.trim_end().to_owned();
                                        let space = if next.args_text.trim().is_empty() { "" } else { " " };
                                        next.args_text = format!("{trimmed}{space}\"{{{name}}}\"");
                                    }
                                },
                                {format!("{{{name}}}")}
                            }
                        }
                    }
                }
                if let EmulatorProfile::Custom { env, .. } = profile.clone() {
                    fieldset {
                        legend { class: "mb-2 text-sm font-medium", {t!(note = "Environment variables", "Environment")} }
                        div { class: "space-y-2",
                            for (index, (name, value)) in env.iter().cloned().enumerate() {
                                div { key: "{index}", class: "grid items-end gap-2 sm:grid-cols-[1fr_2fr_auto]",
                                    TextField {
                                        "aria-label": t!("Environment name {row}", row = index + 1),
                                        value: name,
                                        mono: true,
                                        on_input: move |next: String| {
                                            if let EmulatorProfile::Custom { env, .. } = &mut draft.write().profile && let Some(row) = env.get_mut(index) {
                                                row.0 = next;
                                            }
                                        },
                                    }
                                    TextField {
                                        "aria-label": t!("Environment value {row}", row = index + 1),
                                        value,
                                        mono: true,
                                        on_input: move |next: String| {
                                            if let EmulatorProfile::Custom { env, .. } = &mut draft.write().profile && let Some(row) = env.get_mut(index) {
                                                row.1 = next;
                                            }
                                        },
                                    }
                                    Button {
                                        "aria-label": t!("Remove environment row {row}", row = index + 1),
                                        on_press: move |_| {
                                            if let EmulatorProfile::Custom { env, .. } = &mut draft.write().profile && index < env.len() {
                                                env.remove(index);
                                            }
                                        },
                                        {tp!("environment row", "Remove")}
                                    }
                                }
                            }
                        }
                        Button {
                            class: "mt-2",
                            size: ButtonSize::Sm,
                            icon: PLUS,
                            on_press: move |_| {
                                if let EmulatorProfile::Custom { env, .. } = &mut draft.write().profile {
                                    env.push((String::new(), String::new()));
                                }
                            },
                            {t!("Add environment variable")}
                        }
                    }
                }
                div { class: "grid gap-4",
                    TextField {
                        id: "profile-before",
                        label: t!("Before launch"),
                        description: t!("Optional. A command to run first; if it fails, the game doesn't start."),
                        mono: true,
                        value: current.before_text.clone(),
                        on_input: move |text: String| draft.write().before_text = text,
                    }
                    TextField {
                        id: "profile-after",
                        label: t!("After exit"),
                        mono: true,
                        value: current.after_exit_text.clone(),
                        on_input: move |text: String| draft.write().after_exit_text = text,
                    }
                }
                PreviewTokens { preview: shown_preview, pending: previewing(), error: preview_error }
                if let Some(report) = failure() {
                    p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
                }
                div { class: "flex items-center justify-end gap-2",
                    div { class: "mr-auto",
                        UnsavedHint { dirty }
                    }
                    Button { disabled: saving(), on_press: move |_| on_close.call(()), {t!("Cancel")} }
                    Button {
                        id: "profile-save",
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
                            {t!("Save profile")}
                        }
                    }
                }
                // The user's lifecycle scripts for this profile (#180),
                // saved on their own, apart from the fields above.
                if state.is_some() {
                    section { "aria-label": t!("Profile scripts"), class: "border-t border-border pt-4",
                        h3 { class: "mb-3 font-display text-base font-semibold", {t!("Profile scripts")} }
                        ScriptsPanel {
                            scope: ScriptScope::Profile { profile_id: profile.id().to_owned() },
                            profile_id: profile.id().to_owned(),
                        }
                    }
                }
            }
        }
    }
}

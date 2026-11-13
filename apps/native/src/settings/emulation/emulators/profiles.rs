//! An emulator's profiles: whether each can play, the files it needs and
//! its command line.

use super::*;

/// A profile editor's identity: its id, or what it adds.
pub(super) fn profile_key(profile: &EmulatorProfile) -> String {
    match profile {
        EmulatorProfile::BuiltIn {
            id,
            definition_profile,
            ..
        } if id.is_empty() => format!("built_in:{definition_profile}"),
        EmulatorProfile::Custom { id, .. } if id.is_empty() => "custom:new".to_owned(),
        _ => profile.id().to_owned(),
    }
}

/// The chosen emulator's profiles, each opening to its details, and
/// adding one.
#[component]
pub(super) fn Profiles(
    view: EmulatorView,
    snapshot: Snapshot,
    writing: bool,
    on_edit: EventHandler<ProfileEdit>,
    on_remove: EventHandler<Removing>,
) -> Element {
    let mut open = use_signal(std::collections::BTreeSet::<String>::new);
    let mut menu = use_signal(|| None::<RowMenu>);
    let mut list_node = use_signal(|| None::<NodeHandle>);
    let no_platforms = t!(note = "An emulator profile's platforms", "None selected");
    let no_extensions = tp!("file extensions", "None");
    let has_definition = snapshot
        .catalog
        .definitions
        .iter()
        .any(|row| Some(&row.id) == view.emulator.definition.as_ref());
    let mut addable = view.addable.clone();
    addable.sort_by_key(|profile| !profile.available);
    let addable_items: Vec<MenuEntry> = addable
        .iter()
        .map(|profile| {
            MenuEntry::Item(MenuItem {
                description: Some(if profile.available {
                    t!(note = "An emulator profile's status", "Available")
                } else {
                    t!(note = "An emulator profile's status", "Missing files")
                }),
                ..MenuItem::new(profile.name.clone(), profile.name.clone())
            })
        })
        .collect();
    let profile_of = {
        let view = view.clone();
        move |id: &str| {
            view.emulator
                .profiles
                .iter()
                .find(|profile| profile.id() == id)
                .cloned()
        }
    };
    let on_menu_action = {
        let profile_of = profile_of.clone();
        let emulator_id = view.emulator.id.clone();
        move |action: String| {
            menu.set(None);
            let mut parts = action.splitn(3, ':');
            match (parts.next(), parts.next(), parts.next()) {
                (Some("edit"), Some(id), _) => {
                    if let Some(profile) = profile_of(id) {
                        on_edit.call(ProfileEdit::of(&profile));
                    }
                }
                (Some("remove"), Some(id), Some(name)) => on_remove.call(Removing::Profile {
                    emulator_id: emulator_id.clone(),
                    profile_id: id.to_owned(),
                    name: name.to_owned(),
                }),
                _ => {}
            }
        }
    };
    rsx! {
        SettingsSection {
            icon: LAYERS,
            title: tp!("emulator profiles", "Profiles"),
            description: t!("Whether each profile can play, the files it needs and the command it runs."),
            if view.profiles.is_empty() {
                p { class: "mt-3 rounded-control border border-dashed border-border p-5 text-sm text-fg-muted",
                    {t!("No profiles yet. Add a built-in or custom profile before launching games.")}
                }
            }
            div {
                class: "mt-3 space-y-3",
                onmounted: move |event| list_node.set(event.data().downcast::<NodeHandle>().cloned()),
                for state in view.profiles.iter().cloned() {
                    div {
                        key: "{state.id}",
                        "data-expanded": open.read().contains(&state.id).then_some("true"),
                        class: "rounded-control border border-border bg-surface-overlay/40",
                        oncontextmenu: {
                            let (id, name) = (state.id.clone(), state.name.clone());
                            move |event: MouseEvent| {
                                event.prevent_default();
                                event.stop_propagation();
                                let anchor = crate::frame::pointer_anchor(list_node.peek().as_ref(), &event);
                                let edit = MenuItem { icon: Some(PENCIL), ..MenuItem::new(format!("edit:{id}"), t!("Edit {profile}", profile = name.clone())) };
                                let remove = MenuItem {
                                    icon: Some(TRASH),
                                    danger: true,
                                    disabled: writing,
                                    ..MenuItem::new(format!("remove:{id}:{name}"), t!("Remove {profile}…", profile = name.clone()))
                                };
                                menu.set(Some(RowMenu { anchor, items: vec![MenuEntry::Item(edit), MenuEntry::Item(remove)] }));
                            }
                        },
                        h3 {
                            Button {
                                variant: ButtonVariant::Ghost,
                                "aria-expanded": if open.read().contains(&state.id) { "true" } else { "false" },
                                class: "flex w-full flex-wrap items-center justify-between gap-2 p-3 text-left",
                                on_press: {
                                    let id = state.id.clone();
                                    move |_| {
                                        let mut open = open.write();
                                        if !open.remove(&id) {
                                            open.insert(id.clone());
                                        }
                                    }
                                },
                                span { class: "font-medium", "{state.name}" }
                                {
                                    let available = state.availability == Availability::Available;
                                    let found = view.status != EmulatorStatus::NotFound;
                                    let (tone, text) = if available {
                                        (DotTone::Ok, t!(note = "An emulator's status", "Ready"))
                                    } else if found {
                                        (DotTone::Warning, t!(note = "An emulator profile's status", "Missing files"))
                                    } else {
                                        (DotTone::Danger, t!("Emulator not found"))
                                    };
                                    rsx! { StatusDot { tone, "{text}" } }
                                }
                            }
                        }
                        if open.read().contains(&state.id) {
                            div { class: "space-y-3 border-t border-border p-3",
                                p { class: "text-xs text-fg-muted",
                                    {
                                        let names = state
                                            .platforms
                                            .iter()
                                            .map(|id| snapshot.platform_short(id))
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        t!(
                                            note = "An emulator profile's platforms",
                                            "Platforms: {platforms}",
                                            platforms = if names.is_empty() { no_platforms.clone() } else { names }
                                        )
                                    }
                                }
                                p { class: "font-mono text-xs text-fg-muted",
                                    {
                                        let extensions = state.extensions.join(", ");
                                        t!(
                                            note = "The file extensions an emulator profile opens",
                                            "File types: {extensions}",
                                            extensions = if extensions.is_empty() { no_extensions.clone() } else { extensions }
                                        )
                                    }
                                }
                                if let Availability::MissingFiles { missing } = &state.availability {
                                    ul { class: "space-y-1 font-mono text-xs text-warning",
                                        for path in missing.iter() {
                                            li { key: "{path}",
                                                {t!(note = "A file an emulator profile needs", "Missing: {path}", path = path.clone())}
                                            }
                                        }
                                    }
                                }
                                if state.drift.is_some() {
                                    p { "data-part": "drift", class: "text-sm text-warning",
                                        {t!("Caskade's arguments for this emulator changed since you replaced them. Edit the profile to review them.")}
                                    }
                                }
                                BiosSummary { files: state.bios.clone() }
                                PreviewTokens { preview: Some(state.preview.clone()) }
                                div { class: "flex flex-wrap justify-end gap-2",
                                    Button {
                                        size: ButtonSize::Sm,
                                        on_press: {
                                            let profile_of = profile_of.clone();
                                            let id = state.id.clone();
                                            move |_| {
                                                if let Some(profile) = profile_of(&id) {
                                                    on_edit.call(ProfileEdit::of(&profile));
                                                }
                                            }
                                        },
                                        {t!("Edit {profile}", profile = state.name.clone())}
                                    }
                                    Button {
                                        size: ButtonSize::Sm,
                                        variant: ButtonVariant::Danger,
                                        disabled: writing,
                                        on_press: {
                                            let (emulator_id, id, name) = (view.emulator.id.clone(), state.id.clone(), state.name.clone());
                                            move |_| on_remove.call(Removing::Profile {
                                                emulator_id: emulator_id.clone(),
                                                profile_id: id.clone(),
                                                name: name.clone(),
                                            })
                                        },
                                        {t!("Remove {profile}…", profile = state.name.clone())}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "mt-3 flex flex-wrap gap-2",
                if has_definition {
                    MenuButton {
                        id: "profile-add-built-in",
                        label: t!("Add built-in profile"),
                        icon: PLUS,
                        variant: ButtonVariant::Secondary,
                        disabled: addable_items.is_empty(),
                        items: addable_items,
                        on_action: move |name: String| on_edit.call(ProfileEdit::built_in(&name)),
                    }
                }
                Button {
                    id: "profile-add-custom",
                    icon: PLUS,
                    variant: ButtonVariant::Ghost,
                    on_press: move |_| on_edit.call(ProfileEdit::custom()),
                    {t!("Add custom profile")}
                }
            }
            if let Some(open) = menu() {
                Menu {
                    anchor: open.anchor,
                    items: open.items,
                    on_action: on_menu_action,
                    on_close: move |_| menu.set(None),
                }
            }
        }
    }
}

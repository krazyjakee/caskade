//! Restoring a backup: the archive to restore, what it holds, and this
//! device's own backups (the webview's `DeviceBackups.tsx`).

use super::*;

#[component]
pub(super) fn RestoreSection(backups: Backups) -> Element {
    let shown = use_shown();
    let picker = use_path_picker();
    let mut chosen = use_signal(|| None::<(String, Inspection)>);
    let mut components = use_signal(Vec::<Component>::new);
    let busy_action = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let title = t!("Restore");
    let description = t!(
        "Restoring replaces this library and its settings with the backup's, then restarts Caskade. Nothing is merged, and your games' files aren't touched."
    );
    let status = match &*shown.status.read() {
        Some(Ok(status)) => status.clone(),
        _ => {
            return rsx! {
                Unloaded { icon: ARCHIVE_RESTORE, title, description, preferences: None, backups }
            };
        }
    };
    let open = {
        let backups = backups.clone();
        use_callback(move |path: String| {
            let backups = backups.clone();
            let mut busy = busy_action;
            chosen.set(None);
            failure.set(None);
            busy.set(true);
            spawn(async move {
                let read = change({
                    let path = path.clone();
                    async move { backups.inspect(path).await }
                })
                .await;
                busy.set(false);
                match read {
                    Ok(inspection) => {
                        components.set(
                            inspection
                                .components
                                .iter()
                                .map(|summary| summary.id)
                                .filter(|id| *id != Component::Library)
                                .collect(),
                        );
                        chosen.set(Some((path, inspection)));
                    }
                    Err(report) => failure.set(Some(report)),
                }
            });
        })
    };
    let restart = {
        let backups = backups.clone();
        use_callback(move |()| {
            let backups = backups.clone();
            spawn(async move {
                match change(async move { backups.restart_ready().await }).await {
                    Ok(()) => {
                        if let Err(error) = crate::updater::restart() {
                            failure.set(Some(ErrorReport::from(error)));
                        }
                    }
                    Err(report) => failure.set(Some(report)),
                }
            });
        })
    };
    let staged = status.staged_restore.clone();
    let outcome = status.last_restore.clone();
    let busy = status.running.is_some() || busy_action();
    let default_destination = status.default_destination.to_string_lossy().into_owned();
    rsx! {
        SettingsSection { icon: ARCHIVE_RESTORE, title, description,
            if let Some(outcome) = outcome {
                p { role: "status", class: if outcome.error.is_some() { "mb-3 text-sm text-danger" } else { "mb-3 text-sm text-fg" },
                    match outcome.error.clone() {
                        Some(error) => t!("The restore of {0} was undone, and the library from before is back: {1}", 0 = outcome.journal.archive.clone(), 1 = error),
                        None => t!("Restored from {0}.", 0 = outcome.journal.archive.clone()),
                    }
                }
            }
            if let Some(staged) = staged.clone() {
                div { "data-part": "staged", class: "flex flex-col gap-3",
                    p { class: "text-sm",
                        {t!("{0} is ready to restore. Caskade applies it when it restarts.", 0 = staged.archive.clone())}
                    }
                    div { class: "flex gap-3",
                        // The same words as the restore it finishes (#57).
                        Button { variant: ButtonVariant::Primary, disabled: busy, on_press: move |_| restart.call(()), {t!("Restore and restart")} }
                        Button {
                            disabled: busy,
                            on_press: {
                                let backups = backups.clone();
                                move |_| {
                                    let backups = backups.clone();
                                    spawn(async move {
                                        let read = backups.clone();
                                        if let Err(report) = change(async move { read.discard_restore().await }).await {
                                            failure.set(Some(report));
                                        }
                                        refresh(&backups, shown);
                                    });
                                }
                            },
                            {t!("Don't restore")}
                        }
                    }
                }
            } else {
                div {
                    Button {
                        id: "backup-choose",
                        icon: FOLDER_OPEN,
                        disabled: busy,
                        on_press: move |_| {
                            let mut request = PathRequest::new(PickKind::File, t!("Choose a Caskade backup"));
                            request.start_at = Some(default_destination.clone());
                            request.extensions = vec!["zip".to_owned()];
                            spawn(async move {
                                if let Some(path) = picker.pick(request).await {
                                    open.call(path);
                                }
                            });
                        },
                        {t!("Choose a backup…")}
                    }
                    DeviceBackupList { backups: backups.clone(), busy, on_open: move |path: String| open.call(path) }
                }
            }
            if status.running.as_ref().is_some_and(|running| matches!(running.kind, BackupJobKind::Inspect | BackupJobKind::Restore)) {
                JobProgress { status: status.clone() }
            }
            if let (Some((path, inspection)), None) = (chosen(), staged) {
                RestorePreview {
                    inspection,
                    components: components(),
                    busy,
                    on_components: move |list| components.set(list),
                    on_cancel: move |_| chosen.set(None),
                    on_restore: {
                        let backups = backups.clone();
                        move |_| {
                            let (backups, path, list) = (backups.clone(), path.clone(), components());
                            let mut busy = busy_action;
                            busy.set(true);
                            spawn(async move {
                                let read = backups.clone();
                                let result = change(async move { read.restore(path, list).await }).await;
                                busy.set(false);
                                match result {
                                    Ok(_) => restart.call(()),
                                    Err(report) => failure.set(Some(report)),
                                }
                                refresh(&backups, shown);
                            });
                        }
                    },
                }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline, class: "mt-3" }
            }
        }
    }
}

#[component]
pub(super) fn RestorePreview(
    inspection: Inspection,
    components: Vec<Component>,
    busy: bool,
    on_components: EventHandler<Vec<Component>>,
    on_restore: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    let manifest = &inspection.manifest;
    let games = manifest
        .tables
        .iter()
        .find(|table| table.name == "games")
        .map_or(0, |table| table.rows);
    let made = crate::library::date_time(manifest.created_at_ms);
    let version = manifest.app_version.clone();
    let game_count = tn!("# game", "# games", games);
    let missing_roots = inspection
        .missing_roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(", ");
    rsx! {
        section { "data-part": "preview", "aria-label": t!("Backup to restore"), class: "mt-4 flex flex-col gap-3 rounded-control border border-border bg-surface-base/30 p-4",
            p { class: "text-sm",
                {t!("Made {made} by Caskade {version}, with {gameCount}.", made = made, version = version, gameCount = game_count)}
            }
            fieldset { class: "flex flex-col gap-2",
                legend { class: "mb-1 text-sm font-medium", {t!("Restore")} }
                for summary in inspection.components.clone() {
                    div { key: "{component_id(summary.id)}", class: "flex items-center gap-2",
                        Checkbox {
                            checked: summary.id == Component::Library || components.contains(&summary.id),
                            disabled: summary.id == Component::Library,
                            label: component_label(summary.id),
                            on_change: {
                                let components = components.clone();
                                move |on: bool| {
                                    let mut next: Vec<Component> = components.iter().copied().filter(|id| *id != summary.id).collect();
                                    if on {
                                        next.push(summary.id);
                                    }
                                    on_components.call(next);
                                }
                            },
                        }
                        span { class: "font-mono text-xs text-fg-muted", {format::size(Some(summary.bytes))} }
                    }
                }
            }
            ul { class: "flex list-disc flex-col gap-1 pl-5 text-xs text-fg-muted",
                li { {t!("Parts you leave unticked stay as they are now. Plugins restored without their files ask for their permissions again.")} }
                li { {t!("Passwords and sign-ins aren't in backups: sign in to RomM and your stores again afterwards.")} }
                if inspection.needs_migration {
                    li { {t!("It was made by an older Caskade: its library is updated as it's restored.")} }
                }
                if inspection.other_os {
                    li { class: "text-warning",
                        {t!("It was made on another operating system: games and storage locations whose folders aren't on this computer show as missing until you find them again.")}
                    }
                }
                if !inspection.missing_roots.is_empty() {
                    li { class: "text-warning",
                        {t!("Storage locations not found here: {0}. They show as offline until their drives are connected.", 0 = missing_roots)}
                    }
                }
                if inspection.missing_games > 0 {
                    li { class: "text-warning",
                        {tn!(
                            "# game's folder isn't on this computer; the next scan finds it if it's elsewhere.",
                            "# games' folders aren't on this computer; the next scan finds those that are elsewhere.",
                            u64::from(inspection.missing_games)
                        )}
                    }
                }
            }
            div { class: "flex gap-3",
                Button { id: "restore-now", variant: ButtonVariant::Danger, icon: ARCHIVE_RESTORE, disabled: busy, on_press: move |_| on_restore.call(()), {t!("Restore and restart")} }
                Button { disabled: busy, on_press: move |_| on_cancel.call(()), {t!("Cancel")} }
            }
        }
    }
}

/// Backups paired devices keep, each restorable here.
#[component]
pub(super) fn DeviceBackupList(
    backups: Backups,
    busy: bool,
    on_open: EventHandler<String>,
) -> Element {
    let held = {
        let backups = backups.clone();
        use_resource(move || {
            let backups = backups.clone();
            async move {
                change(async move { backups.on_devices().await })
                    .await
                    .unwrap_or_default()
            }
        })
    };
    let mut fetching = use_signal(|| None::<String>);
    let mut failure = use_signal(|| None::<String>);
    let devices: Vec<DeviceBackups> = held.read().clone().unwrap_or_default();
    if devices.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { "data-part": "device-backups", class: "mt-4 flex flex-col gap-3",
            h3 { class: "flex items-center gap-2 text-sm font-semibold",
                Svg { icon: MONITOR_SMARTPHONE, class: "size-4 text-fg-muted" }
                {t!("On your other devices")}
            }
            for device in devices {
                section { key: "{device.device_id}", "aria-label": t!("Kept on {holder}", holder = device.device_name.clone()),
                    p { class: "text-xs text-fg-muted", {t!("Kept on {holder}", holder = device.device_name.clone())} }
                    ul { class: "mt-1 flex flex-col gap-1",
                        for backup in device.backups.clone() {
                            {
                                let from = if backup.origin_name.is_empty() { tp!("backup of", "Another device") } else { backup.origin_name.clone() };
                                let when = crate::library::date_time(backup.created_ms);
                                let hash = backup.hash.clone();
                                let holder = device.device_name.clone();
                                rsx! {
                                    li { key: "{backup.hash}", class: "flex flex-wrap items-center justify-between gap-3 text-sm",
                                        span {
                                            {t!("{from}, {when}", from = from.clone(), when = when.clone())}
                                            span { class: "ml-2 text-xs text-fg-muted", {format::bytes(Some(backup.size))} }
                                        }
                                        Button {
                                            "data-spatial": "focusable",
                                            size: ButtonSize::Sm,
                                            disabled: busy || fetching().is_some(),
                                            "aria-label": t!("Restore the backup of {from} from {when}", from = from.clone(), when = when.clone()),
                                            on_press: {
                                                let (backups, hash) = (backups.clone(), hash.clone());
                                                move |_| {
                                                    let (backups, hash) = (backups.clone(), hash.clone());
                                                    fetching.set(Some(hash.clone()));
                                                    failure.set(None);
                                                    spawn(async move {
                                                        let got = change(async move { backups.fetch_from_device(&hash).await }).await;
                                                        fetching.set(None);
                                                        match got {
                                                            Ok(path) => on_open.call(path),
                                                            Err(report) => failure.set(Some(report.summary)),
                                                        }
                                                    });
                                                }
                                            },
                                            if fetching().as_deref() == Some(hash.as_str()) {
                                                {t!("Getting it from {holder}…", holder = holder)}
                                            } else {
                                                {t!("Restore…")}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(message) = failure() {
                p { role: "alert", class: "text-sm text-danger", "{message}" }
            }
        }
    }
}

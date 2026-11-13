//! One add-on's details: what it may do, its screenshots, installing,
//! updating or removing it, and how that went.

use super::*;

/// The catalogue's permission words in the review's plain words; a word
/// this version doesn't know stays as written.
pub fn permission_lines(words: &[String]) -> Vec<String> {
    let mut network = Vec::new();
    let mut fs = Vec::new();
    let mut process = false;
    let mut library = LibraryAccess::None;
    let mut unknown = Vec::new();
    for word in words {
        match word.as_str() {
            "library.read" => library = LibraryAccess::Read,
            "library.write" => library = LibraryAccess::Write,
            "process" => process = true,
            other => match (other.strip_prefix("network:"), other.strip_prefix("fs:")) {
                (Some(host), _) => network.push(host.to_owned()),
                (_, Some(folder)) => fs.push(folder.to_owned()),
                _ => unknown.push(other.to_owned()),
            },
        }
    }
    let plugin: PluginSummary = serde_json::from_value(serde_json::json!({
        "id": "", "name": "", "version": null, "description": null, "author": null,
        "folder": "", "status": "disabled", "errors": [],
        "permissions": {"network": network, "fs": fs, "process": process, "library": library},
        "contributes": null, "hasUi": false, "hasBackend": false, "failures": 0,
        "lastError": null, "settingsError": null, "dev": false
    }))
    .expect("a plugin summary from its own fields");
    let mut lines = crate::settings::plugins::capability_lines(&plugin);
    lines.extend(unknown);
    lines
}

/// One catalogue add-on: its description (as text), screenshots,
/// releases, what it asks for and why it may not fit, and installing,
/// updating, skipping and removing it.
#[component]
pub(super) fn AddonDetails(
    item: CatalogueItem,
    offline: bool,
    record: Option<AddonRecord>,
    on_changed: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let addons = use_addons();
    let source = use_context::<DataSource>();
    let entry = item.entry.clone();
    let compatibility = item.compatibility.clone();
    let release = compatibility.release.clone();
    let mut progress = use_signal(|| None::<AddonProgress>);
    let mut outcome = use_signal(|| None::<AddonOutcome>);
    let mut keep_data = use_signal(|| true);
    let mut confirm_remove = use_signal(|| false);
    let mut installing = use_signal(|| false);
    let mut removing = use_signal(|| false);
    let mut install_error = use_signal(|| None::<String>);
    let mut remove_error = use_signal(|| None::<String>);
    // Install progress for this add-on, from the app's events.
    let id = entry.id.clone();
    use_hook(move || {
        let mut events = source.events.subscribe();
        spawn(async move {
            while let Ok(event) = events.recv().await {
                if let Event::AddonProgress(next) = event
                    && next.id == id
                {
                    progress.set(Some(next));
                }
            }
        });
    });
    let install = use_callback({
        let addons = addons.clone();
        let id = entry.id.clone();
        move |(accept, retry): (bool, bool)| {
            let addons = addons.clone();
            let id = id.clone();
            installing.set(true);
            install_error.set(None);
            spawn(async move {
                let result =
                    change(async move { addons.install(&id, mode(), accept, retry).await }).await;
                installing.set(false);
                progress.set(None);
                match result {
                    Ok(result) => {
                        let review = matches!(result, AddonOutcome::NeedsReview { .. });
                        outcome.set(Some(result));
                        // A failure may be remembered as a rejected release.
                        if !review {
                            on_changed.call(());
                        }
                    }
                    Err(error) => install_error.set(Some(error.summary)),
                }
            });
        }
    });
    let remove = {
        let addons = addons.clone();
        let id = entry.id.clone();
        move |_| {
            let addons = addons.clone();
            let id = id.clone();
            let keep = *keep_data.peek();
            removing.set(true);
            remove_error.set(None);
            spawn(async move {
                let result = change(async move { addons.uninstall(&id, keep).await }).await;
                removing.set(false);
                match result {
                    Ok(()) => {
                        confirm_remove.set(false);
                        on_changed.call(());
                    }
                    Err(error) => remove_error.set(Some(error.summary)),
                }
            });
        }
    };
    let installed = matches!(
        compatibility.state,
        ItemState::Installed | ItemState::UpdateAvailable
    );
    let rejected = record
        .as_ref()
        .and_then(|record| record.rejected.clone())
        .filter(|rejected| {
            Some(&rejected.version) == release.as_ref().map(|release| &release.version)
        });
    let skipped = compatibility.state == ItemState::UpdateAvailable
        && record.as_ref().and_then(|record| record.skipped.clone())
            == release.as_ref().map(|release| release.version.clone());
    let busy = installing() || removing();
    let skip = {
        let addons = addons.clone();
        let id = entry.id.clone();
        let version = release.as_ref().map(|release| release.version.clone());
        move |_| {
            let addons = addons.clone();
            let id = id.clone();
            let next = if skipped { None } else { version.clone() };
            spawn(async move {
                if change(async move { addons.skip_update(&id, next).await })
                    .await
                    .is_ok()
                {
                    on_changed.call(());
                }
            });
        }
    };
    let review = match outcome() {
        Some(AddonOutcome::NeedsReview { preview, .. }) => Some(preview),
        _ => None,
    };
    let install_label = match (&release, compatibility.state) {
        (Some(release), ItemState::UpdateAvailable) => {
            t!("Update to {0}", 0 = release.version.clone())
        }
        _ if rejected.is_some() => t!("Try again"),
        _ => t!("Install"),
    };
    let progress_text = progress().map(|progress| match progress.phase {
        AddonPhase::Downloading => t!(
            "Downloading… {0}%",
            0 = (progress.received * 100 / progress.total.max(1))
        ),
        AddonPhase::Checking => t!("Checking the download…"),
        AddonPhase::Installing => t!("Installing…"),
        #[allow(unreachable_patterns)]
        _ => t!("Starting…"),
    });
    let modes = release.as_ref().map(|release| {
        if release.modes.len() == 1 {
            if release.modes[0] == AppMode::Desktop {
                t!("Desktop mode")
            } else {
                t!("Console mode")
            }
        } else {
            t!("Desktop and console modes")
        }
    });
    let runs_on = release.as_ref().map(|release| {
        if release.os.is_empty() {
            t!("Linux, Windows and macOS")
        } else {
            release
                .os
                .iter()
                .map(|os| match os {
                    AddonOs::Linux => "Linux",
                    AddonOs::Windows => "Windows",
                    AddonOs::Macos => "macOS",
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    });
    let asks = release
        .as_ref()
        .map(|release| permission_lines(&release.permissions))
        .unwrap_or_default();
    rsx! {
        Dialog {
            title: entry.name.clone(),
            "data-cs": "addon-details",
            "data-kind": if entry.kind == AddonKind::Plugin { "plugin" } else { "theme" },
            "data-state": state_name(compatibility.state),
            class: "max-w-2xl",
            on_close: move |_| {
                if !busy {
                    on_close.call(());
                }
            },
            div { class: "flex flex-col gap-4",
                div { class: "flex flex-wrap items-start gap-3",
                    div { class: "min-w-0 flex-1",
                        p { class: "text-sm text-fg-muted", "{kind_name(entry.kind)} · {entry.author} · {entry.license}" }
                        p { class: "font-mono text-xs text-fg-muted", "{entry.id}" }
                    }
                    StateBadge { item: item.clone() }
                }
                if !entry.screenshots.is_empty() {
                    ul { "data-part": "screenshots", "aria-label": t!("Screenshots"), class: "flex gap-3 overflow-x-auto",
                        for (index, shot) in entry.screenshots.iter().enumerate() {
                            li { key: "{shot.url}", class: "shrink-0",
                                Screenshot { id: entry.id.clone(), index: index as u32, alt: shot.alt.clone() }
                            }
                        }
                    }
                }
                // Text, never markup.
                p { "data-part": "description", class: "text-sm whitespace-pre-wrap",
                    {if entry.description.is_empty() { entry.summary.clone() } else { entry.description.clone() }}
                }
                if let Some(deprecated) = &entry.deprecated {
                    p { role: "note", class: "text-sm text-warning",
                        {t!("Deprecated: {0}", 0 = deprecated.clone())}
                    }
                }
                if !compatibility.reasons.is_empty() {
                    ul { "data-part": "reasons", "aria-label": t!("Compatibility"), class: "flex flex-col gap-1 text-sm",
                        for reason in compatibility.reasons.clone() {
                            li {
                                key: "{reason.message}",
                                class: if reason.blocking { "text-danger" } else { "text-warning" },
                                "{reason.message}"
                            }
                        }
                    }
                }
                if let Some(release) = release.clone() {
                    dl { "data-part": "release", class: "grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm",
                        dt { class: "text-fg-muted", {t!("Version")} }
                        dd {
                            "{release.version}"
                            if let Some(published) = &release.published {
                                span { class: "text-fg-subtle", " · {published}" }
                            }
                        }
                        dt { class: "text-fg-muted", {t!("Runs on")} }
                        dd { {runs_on.clone().unwrap_or_default()} }
                        if entry.kind == AddonKind::Plugin {
                            dt { class: "text-fg-muted", {t!("Modes")} }
                            dd { {modes.clone().unwrap_or_default()} }
                            dt { class: "text-fg-muted", {t!("Asks for")} }
                            dd {
                                if asks.is_empty() {
                                    {t!("Nothing beyond its own storage")}
                                }
                                for line in asks.clone() {
                                    span { key: "{line}", class: "block", "{line}" }
                                }
                            }
                        }
                        dt { class: "text-fg-muted", {t!("Download")} }
                        dd {
                            {t!("{0} KB", 0 = ((release.artifact.size as f64 / 1024.0).round() as u64).max(1))}
                        }
                        if !release.notes.is_empty() {
                            dt { class: "text-fg-muted", {t!("What's new")} }
                            dd { class: "whitespace-pre-wrap", "{release.notes}" }
                        }
                    }
                }
                if let Some(text) = progress_text {
                    div { "data-part": "progress", role: "status", class: "text-sm text-fg-muted", "{text}" }
                }
                Outcome { outcome: outcome(), rejected: rejected.clone() }
                if offline && !installed {
                    p { class: "text-sm text-fg-muted", {t!("Installing needs a connection.")} }
                }
                if confirm_remove() {
                    div { "data-part": "remove-confirm", class: "flex flex-col gap-2 rounded-panel border border-danger/40 p-3",
                        p { class: "text-sm",
                            if entry.kind == AddonKind::Plugin {
                                {t!("Uninstall {0}?", 0 = entry.name.clone())}
                            } else {
                                {t!("Uninstall {0}? If it's the theme in use, the default theme takes over.", 0 = entry.name.clone())}
                            }
                        }
                        if entry.kind == AddonKind::Plugin {
                            Checkbox {
                                "data-spatial": "focusable",
                                label: t!("Keep its settings and data for a reinstall"),
                                checked: keep_data(),
                                on_change: move |on: bool| keep_data.set(on),
                            }
                        }
                        div { class: "flex gap-2",
                            Button { "data-spatial": "focusable", size: ButtonSize::Sm, on_press: move |_| confirm_remove.set(false), {t!("Keep it")} }
                            Button {
                                "data-part": "remove-confirmed",
                                "data-spatial": "focusable",
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Danger,
                                disabled: removing(),
                                on_press: remove,
                                {t!("Uninstall")}
                            }
                        }
                        if let Some(error) = remove_error() {
                            p { role: "alert", class: "text-sm text-danger", "{error}" }
                        }
                    }
                }
                div { "data-part": "actions", class: "flex flex-wrap justify-end gap-2",
                    if installing() {
                        Button {
                            "data-spatial": "focusable",
                            on_press: {
                                let addons = addons.clone();
                                let id = entry.id.clone();
                                move |_| {
                                    addons.cancel(&id);
                                }
                            },
                            {t!("Cancel download")}
                        }
                    }
                    if installed && !confirm_remove() {
                        Button {
                            "data-part": "remove",
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Ghost,
                            disabled: busy,
                            on_press: move |_| confirm_remove.set(true),
                            {t!("Uninstall…")}
                        }
                    }
                    if compatibility.state == ItemState::UpdateAvailable && release.is_some() {
                        Button {
                            "data-part": "skip",
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Ghost,
                            disabled: busy,
                            on_press: skip,
                            if skipped {
                                {t!("Offer this version again")}
                            } else {
                                {t!("Skip this version")}
                            }
                        }
                    }
                    if entry.kind == AddonKind::Theme && installed {
                        ApplyTheme { id: entry.id.clone() }
                    }
                    if release.is_some() && compatibility.state != ItemState::Installed {
                        Button {
                            "data-part": "install",
                            "data-spatial": "focusable",
                            "data-focus-default": "true",
                            variant: ButtonVariant::Primary,
                            disabled: busy,
                            on_press: {
                                let retry = rejected.is_some();
                                move |_| install((false, retry))
                            },
                            "{install_label}"
                        }
                    }
                    Button { "data-spatial": "focusable", disabled: busy, on_press: move |_| on_close.call(()), {t!("Close")} }
                }
                if let Some(error) = install_error() {
                    p { role: "alert", class: "text-sm text-danger", "{error}" }
                }
            }
        }
        if let Some(preview) = review {
            ReviewDialog {
                plugin: preview.plugin.clone(),
                installed_version: preview.installed_version.clone(),
                action: t!("Install and allow"),
                busy: installing(),
                error: install_error(),
                on_accept: move |_| install((true, false)),
                on_close: move |_| outcome.set(None),
            }
        }
    }
}

#[component]
pub(super) fn Screenshot(id: String, index: u32, alt: String) -> Element {
    let media = use_addons_query({
        let id = id.clone();
        move |addons| {
            let id = id.clone();
            async move { addons.media(&id, index).await }
        }
    });
    let url = match &*media.read() {
        Some(Ok(Some(cached))) => Some(format!(
            "{}://localhost/{}",
            crate::library::ART_SCHEME,
            cached.cached_path.trim_start_matches('/')
        )),
        _ => None,
    };
    match url {
        Some(url) => rsx! {
            div { class: "relative h-36 w-64 overflow-hidden rounded-control",
                img { "data-part": "screenshot", src: "{url}", alt: "{alt}", class: "absolute inset-0 size-full object-cover" }
            }
        },
        None => rsx! {
            div {
                "data-part": "screenshot-missing",
                role: "img",
                "aria-label": if alt.is_empty() { t!("Screenshot") } else { alt.clone() },
                class: "grid h-36 w-64 place-items-center rounded-control border border-border bg-surface-base/60 text-fg-subtle",
                crate::ui::Svg { icon: IMAGE_OFF, class: "size-5" }
            }
        },
    }
}

#[component]
pub(super) fn Outcome(
    outcome: Option<AddonOutcome>,
    rejected: Option<caskade_ui_data::addons::RejectedRelease>,
) -> Element {
    match (outcome, rejected) {
        (
            Some(AddonOutcome::Installed {
                version,
                needs_consent,
                restart_required,
                ..
            }),
            _,
        ) => {
            let text = if needs_consent {
                t!(
                    "{0} is installed. It asks for more than before, so it's off until you allow it under Installed plugins.",
                    0 = version
                )
            } else if restart_required {
                t!(
                    "{0} is installed. Its pages change when Caskade restarts.",
                    0 = version
                )
            } else {
                t!("{0} is installed.", 0 = version)
            };
            rsx! {
                p { "data-part": "outcome", "data-outcome": "installed", role: "status", class: "text-sm text-success", "{text}" }
            }
        }
        (outcome, rejected) => {
            let failure = match (outcome, rejected) {
                (
                    Some(AddonOutcome::Failed {
                        kind,
                        message,
                        rolled_back,
                        ..
                    }),
                    _,
                ) => Some((kind, message, rolled_back)),
                (None, Some(rejected)) => Some((rejected.kind, rejected.message, false)),
                _ => None,
            };
            let Some((kind, message, rolled_back)) = failure else {
                return rsx! {};
            };
            let transient = kind.transient();
            let text = if rolled_back {
                t!(
                    "It didn't install, and the previous version is back: {0}",
                    0 = message
                )
            } else if transient {
                t!("It didn't install; try again later: {0}", 0 = message)
            } else {
                t!("It didn't install: {0}", 0 = message)
            };
            rsx! {
                p { "data-part": "outcome", "data-outcome": "failed", role: "alert", class: "text-sm text-danger", "{text}" }
            }
        }
    }
}

/// Makes an installed theme the one in use, as Appearance does.
#[component]
pub(super) fn ApplyTheme(id: String) -> Element {
    let section = use_settings_section("core.appearance");
    let active = section
        .value()
        .is_some_and(|value| value.get("theme").and_then(Value::as_str) == Some(id.as_str()));
    rsx! {
        Button {
            "data-part": "apply",
            "data-spatial": "focusable",
            disabled: active || section.saving() || section.value().is_none(),
            on_press: move |_| section.set_field("theme", Value::String(id.clone())),
            if active {
                {t!("In use")}
            } else {
                {t!("Use this theme")}
            }
        }
    }
}

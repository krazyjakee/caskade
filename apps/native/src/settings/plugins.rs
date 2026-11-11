//! Settings → Plugins (#104), as the webview's `PluginsSettings` and
//! `pluginTerms.ts`: the installed plugins with their status, what they
//! may do in a few words, turning them on and off, reviewing what one asks
//! for before it's allowed or installed from a folder, its details, and
//! uninstalling. A right click offers a row's buttons. The add-ons
//! catalogue follows (`addons.rs`). Plugins' own settings pages are UI
//! modules the native app doesn't run yet (#109).

use caskade_ui_data::ErrorReport;
use caskade_ui_data::plugins::{LibraryAccess, PluginStatus, PluginSummary, Plugins};
use caskade_ui_i18n::{t, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use crate::data::{change, use_plugins, use_plugins_query};
use crate::flows::paths::{PathRequest, PickKind, use_path_picker};
use crate::ui::icons::{FOLDER_OPEN, INFO, POWER, PUZZLE, SHIELD_CHECK, TRASH};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Dialog, DotTone, Menu, MenuEntry, MenuItem, SettingsSection,
    StatusDot, Switch,
};

// ── In words (#53) ──

fn slot_name(slot: &str) -> String {
    match slot {
        "views" => tp!("plugin slot", "A page of its own"),
        "home.sections" => tp!("plugin slot", "A section on Home"),
        "game.details.tabs" => tp!("plugin slot", "A tab on game pages"),
        "game.details.panels" => tp!("plugin slot", "A panel on game pages"),
        "game.card.badges" => tp!("plugin slot", "A badge on game cards"),
        "game.card.overlay" => tp!("plugin slot", "An overlay on game cards"),
        "game.actions" => tp!("plugin slot", "An item in the game menu"),
        "topbar.items" => tp!("plugin slot", "A button in the top bar"),
        "sidebar.items" => tp!("plugin slot", "An item in the sidebar"),
        "settings.pages" => tp!("plugin slot", "A settings page"),
        "commands" => tp!("plugin slot", "A command"),
        "notifications" => tp!("plugin slot", "Notifications"),
        other => other.to_owned(),
    }
}

/// A hook that changes what Caskade does, by what it changes.
fn interceptor(hook: &str) -> Option<String> {
    Some(match hook {
        "launch.before" => tp!("plugin hook", "games before they start"),
        "launch.after" => tp!("plugin hook", "what happens after a game exits"),
        "library.import" => tp!("plugin hook", "games joining the library"),
        "metadata.merge" => tp!("plugin hook", "game details"),
        "install.plan" => tp!("plugin hook", "how games install"),
        "install.after" => tp!("plugin hook", "finished installs"),
        "install.moved" => tp!("plugin hook", "moved games"),
        _ => return None,
    })
}

/// An event a plugin hears about, by the first part of its name.
fn event(hook: &str) -> Option<String> {
    Some(match hook.split('.').next().unwrap_or_default() {
        "app" => tp!("plugin event", "Caskade starting"),
        "library" => tp!("plugin event", "library changes"),
        "game" => tp!("plugin event", "games starting, stopping and moving"),
        "settings" => tp!("plugin event", "settings changes"),
        "emulation" => tp!("plugin event", "emulator changes"),
        "install" | "uninstall" => tp!("plugin event", "installs"),
        "input" => tp!("plugin event", "controllers connecting"),
        "achievements" => tp!("plugin event", "achievements"),
        _ => return None,
    })
}

fn provider_name(provider: &str) -> String {
    match provider {
        "library-source" => tp!("plugin provider", "games for your library"),
        "metadata-provider" => tp!("plugin provider", "game details"),
        "runner" => tp!("plugin provider", "a way to run games"),
        "install-handler" => tp!("plugin provider", "a way to install games"),
        "url-resolver" => tp!("plugin provider", "downloads from web pages"),
        other => other.to_owned(),
    }
}

/// A plugin's hooks as what it can change and what it hears about, each
/// named once.
pub fn hook_names(hooks: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut changes, mut hears) = (Vec::new(), Vec::new());
    for hook in hooks {
        let (list, name) = match interceptor(hook) {
            Some(name) => (&mut changes, name),
            None => (&mut hears, event(hook).unwrap_or_else(|| hook.clone())),
        };
        if !list.contains(&name) {
            list.push(name);
        }
    }
    (changes, hears)
}

/// What a plugin asks to do, in plain words.
pub fn capability_lines(plugin: &PluginSummary) -> Vec<String> {
    let Some(permissions) = &plugin.permissions else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if !permissions.network.is_empty() {
        lines.push(t!(
            "Connect to {hosts}",
            hosts = permissions.network.join(", ")
        ));
    }
    for folder in &permissions.fs {
        match folder.strip_suffix(":ro") {
            Some(path) => lines.push(t!("Read files in {path}", path = path.to_owned())),
            None => lines.push(t!(
                "Read and change files in {path}",
                path = folder.trim_end_matches(":rw").to_owned()
            )),
        }
    }
    if permissions.process {
        lines.push(t!("Run programs and change how games are launched"));
    }
    match permissions.library {
        LibraryAccess::Read => lines.push(t!("Read your library")),
        LibraryAccess::Write => lines.push(t!("Read and change your library")),
        LibraryAccess::None => {}
    }
    lines
}

/// Everything a plugin can do, its UI half included: UI modules run with
/// the app's access (a known gap).
fn access_lines(plugin: &PluginSummary) -> Vec<String> {
    let mut lines = Vec::new();
    if plugin.has_ui {
        lines.push(t!(
            "Run interface code inside Caskade, with the same access as the app"
        ));
    }
    lines.extend(capability_lines(plugin));
    lines
}

/// A plugin's access in a few words, for its row.
pub fn access_summary(plugin: &PluginSummary) -> String {
    if plugin.has_ui {
        return t!("Same access as the app");
    }
    let mut uses = Vec::new();
    if let Some(permissions) = &plugin.permissions {
        if !permissions.network.is_empty() {
            uses.push(tp!("plugin access", "network"));
        }
        if !permissions.fs.is_empty() {
            uses.push(tp!("plugin access", "files"));
        }
        if permissions.process {
            uses.push(tp!("plugin access", "programs"));
        }
        if permissions.library != LibraryAccess::None {
            uses.push(tp!("plugin access", "your library"));
        }
    }
    if uses.is_empty() {
        t!("Nothing beyond its own storage")
    } else {
        t!("Uses {what}", what = uses.join(", "))
    }
}

fn contribution_lines(plugin: &PluginSummary) -> Vec<String> {
    let Some(contributes) = &plugin.contributes else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for view in &contributes.views {
        lines.push(t!("View: {title}", title = view.title.clone()));
    }
    for slot in &contributes.slots {
        lines.push(format!(
            "{}: {}",
            slot_name(&slot.slot),
            slot.title.clone().unwrap_or_else(|| slot.id.clone())
        ));
    }
    let (changes, hears) = hook_names(&contributes.hooks);
    if !changes.is_empty() {
        lines.push(t!("Can change {what}", what = changes.join(", ")));
    }
    if !hears.is_empty() {
        lines.push(t!("Is told about {what}", what = hears.join(", ")));
    }
    if !contributes.providers.is_empty() {
        lines.push(t!(
            "Provides {providers}",
            providers = contributes
                .providers
                .iter()
                .map(|provider| provider_name(provider))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if contributes.settings.is_some() {
        lines.push(t!("Settings page"));
    }
    lines
}

fn status_label(status: PluginStatus) -> String {
    match status {
        PluginStatus::Invalid => tp!("plugin status", "Invalid"),
        PluginStatus::Disabled => tp!("plugin status", "Off"),
        PluginStatus::NeedsConsent => tp!("plugin status", "Needs your approval"),
        PluginStatus::Enabled => tp!("plugin status", "On"),
        PluginStatus::Active => tp!("plugin status", "Running"),
    }
}

fn status_name(status: PluginStatus) -> &'static str {
    match status {
        PluginStatus::Invalid => "invalid",
        PluginStatus::Disabled => "disabled",
        PluginStatus::NeedsConsent => "needs_consent",
        PluginStatus::Enabled => "enabled",
        PluginStatus::Active => "active",
    }
}

fn status_tone(status: PluginStatus) -> DotTone {
    match status {
        PluginStatus::Active | PluginStatus::Enabled => DotTone::Ok,
        PluginStatus::NeedsConsent => DotTone::Warning,
        PluginStatus::Invalid => DotTone::Danger,
        PluginStatus::Disabled => DotTone::Off,
    }
}

fn problems(errors: &[caskade_ui_data::plugins::ManifestError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

// ── The review ──

/// Name, version, contributions and permissions, before anything is
/// granted; without `on_accept`, an installed plugin's details.
#[component]
pub fn ReviewDialog(
    plugin: PluginSummary,
    #[props(default)] installed_version: Option<String>,
    #[props(default)] action: Option<String>,
    #[props(default)] busy: bool,
    #[props(default)] error: Option<String>,
    #[props(default)] on_accept: Option<EventHandler<()>>,
    on_close: EventHandler<()>,
) -> Element {
    let capabilities = access_lines(&plugin);
    let contributions = contribution_lines(&plugin);
    let valid = plugin.status != PluginStatus::Invalid;
    let title = match &plugin.version {
        Some(version) => format!("{} {version}", plugin.name),
        None => plugin.name.clone(),
    };
    rsx! {
        Dialog {
            title,
            "data-cs": "plugin-review",
            on_close: move |_| {
                if !busy {
                    on_close.call(());
                }
            },
            p { class: "font-mono text-xs text-fg-muted", "{plugin.id}" }
            if let Some(description) = &plugin.description {
                p { class: "mt-3 text-sm", "{description}" }
            }
            if let Some(installed) = installed_version.clone() {
                p { class: "mt-2 text-sm text-fg-muted",
                    {t!("Replaces the installed version {installedVersion}.", installedVersion = installed)}
                }
            }
            if valid {
                h3 { class: "mt-4 text-sm font-semibold",
                    if on_accept.is_some() {
                        {t!(note = "Heading over a plugin's permissions", "It asks to")}
                    } else {
                        {t!(note = "Heading over an installed plugin's permissions", "It can")}
                    }
                }
                ul { "aria-label": t!("Permissions"), class: "mt-1 list-disc pl-5 text-sm",
                    if capabilities.is_empty() {
                        li { {t!("Nothing beyond its own storage")} }
                    }
                    for line in capabilities.clone() {
                        li { key: "{line}", "{line}" }
                    }
                }
                if !contributions.is_empty() {
                    h3 { class: "mt-4 text-sm font-semibold",
                        {t!(note = "Heading over what a plugin contributes", "It adds")}
                    }
                    ul { "aria-label": t!("Contributions"), class: "mt-1 list-disc pl-5 text-sm",
                        for line in contributions.clone() {
                            li { key: "{line}", "{line}" }
                        }
                    }
                }
            } else {
                ul { "aria-label": t!("Problems"), class: "mt-4 list-disc pl-5 text-sm text-danger",
                    for problem in plugin.errors.clone() {
                        li { key: "{problem}", "{problem}" }
                    }
                }
            }
            if let Some(error) = error {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            div { class: "mt-6 flex justify-end gap-2",
                if let Some(accept) = on_accept {
                    Button { disabled: busy, on_press: move |_| on_close.call(()), {t!("Cancel")} }
                    Button {
                        variant: ButtonVariant::Primary,
                        disabled: !valid || busy,
                        on_press: move |_| accept.call(()),
                        {action.clone().unwrap_or_default()}
                    }
                } else {
                    Button { on_press: move |_| on_close.call(()), {t!("Close")} }
                }
            }
        }
    }
}

// ── The list ──

/// What the review is about: an installed plugin to allow, or a folder to
/// install.
#[derive(Clone, PartialEq)]
struct Review {
    plugin: PluginSummary,
    folder: Option<String>,
    installed_version: Option<String>,
}

#[component]
pub fn PluginsSettings() -> Element {
    let plugins = use_plugins();
    let mut list = use_plugins_query(|plugins| async move { plugins.list().await });
    let mut review = use_signal(|| None::<Review>);
    let mut details = use_signal(|| None::<PluginSummary>);
    let mut choose_error = use_signal(|| None::<String>);
    let mut choosing = use_signal(|| false);
    let mut accepting = use_signal(|| false);
    let mut accept_error = use_signal(|| None::<String>);
    let picker = use_path_picker();
    let choose = {
        let plugins = plugins.clone();
        move |_| {
            let plugins = plugins.clone();
            choose_error.set(None);
            spawn(async move {
                let Some(folder) = picker
                    .pick(PathRequest::new(PickKind::Folder, t!("Plugin folder")))
                    .await
                else {
                    return;
                };
                choosing.set(true);
                let look = folder.clone();
                let outcome = change(async move { plugins.preview(&look).await }).await;
                choosing.set(false);
                match outcome {
                    Ok(preview) => review.set(Some(Review {
                        plugin: preview.plugin,
                        folder: Some(folder),
                        installed_version: preview.installed_version,
                    })),
                    Err(error) => choose_error.set(Some(error.summary)),
                }
            });
        }
    };
    let accept = {
        let plugins = plugins.clone();
        move |_| {
            let Some(current) = review.peek().clone() else {
                return;
            };
            let plugins = plugins.clone();
            accepting.set(true);
            accept_error.set(None);
            spawn(async move {
                let outcome = change(async move {
                    match current.folder {
                        Some(folder) => plugins.install(&folder).await.map(|_| ()),
                        None => plugins
                            .set_enabled(&current.plugin.id, true)
                            .await
                            .map(|_| ()),
                    }
                })
                .await;
                accepting.set(false);
                match outcome {
                    Ok(()) => {
                        review.set(None);
                        list.restart();
                    }
                    Err(error) => accept_error.set(Some(error.summary)),
                }
            });
        }
    };
    let read: Option<Result<Vec<PluginSummary>, ErrorReport>> = list.read().clone();
    rsx! {
        div { "data-settings": "plugins", class: "flex flex-col gap-4",
            SettingsSection {
                icon: Some(PUZZLE),
                title: t!("Installed plugins"),
                description: t!("What you've added to Caskade. Drop a plugin's folder here to install it."),
                aside: rsx! {
                    Button {
                        size: ButtonSize::Sm,
                        icon: Some(FOLDER_OPEN),
                        disabled: choosing(),
                        on_press: choose,
                        {t!("Install from folder…")}
                    }
                },
                if let Some(error) = choose_error() {
                    p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
                }
                match read {
                    None => rsx! {
                        p { class: "mt-3 text-sm text-fg-muted", {t!("Loading plugins…")} }
                    },
                    Some(Err(error)) => rsx! {
                        p { role: "alert", class: "mt-3 text-sm text-danger", "{error.summary}" }
                    },
                    Some(Ok(found)) if found.is_empty() => rsx! {
                        p { class: "mt-3 rounded-control border border-dashed border-border p-5 text-sm text-fg-muted",
                            {t!("No plugins yet. Get one from the catalogue below, install one from a folder, or drop its folder here.")}
                        }
                    },
                    Some(Ok(found)) => rsx! {
                        ul { "aria-label": t!("Installed plugins"), class: "flex flex-col",
                            for plugin in found {
                                PluginRow {
                                    key: "{plugin.id}",
                                    plugins: plugins.clone(),
                                    on_review: move |plugin: PluginSummary| {
                                        accept_error.set(None);
                                        review.set(Some(Review { plugin, folder: None, installed_version: None }))
                                    },
                                    on_details: move |plugin: PluginSummary| details.set(Some(plugin)),
                                    on_changed: move |_| list.restart(),
                                    plugin,
                                }
                            }
                        }
                    },
                }
            }
            // The add-ons catalogue (#104) follows in `addons.rs`.
            if let Some(current) = review() {
                ReviewDialog {
                    plugin: current.plugin.clone(),
                    installed_version: current.installed_version.clone(),
                    action: if current.folder.is_some() { t!("Install and allow") } else { tp!("plugin", "Allow") },
                    busy: accepting(),
                    error: accept_error(),
                    on_accept: accept,
                    on_close: move |_| {
                        review.set(None);
                        accept_error.set(None);
                    },
                }
            }
            if let Some(plugin) = details() {
                ReviewDialog { plugin, on_close: move |_| details.set(None) }
            }
        }
    }
}

#[component]
fn PluginRow(
    plugin: PluginSummary,
    plugins: Plugins,
    on_review: EventHandler<PluginSummary>,
    on_details: EventHandler<PluginSummary>,
    on_changed: EventHandler<()>,
) -> Element {
    let mut confirming = use_signal(|| false);
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None::<String>);
    let mut row = use_signal(|| None::<NodeHandle>);
    let mut menu = use_signal(|| None);
    let on = matches!(plugin.status, PluginStatus::Enabled | PluginStatus::Active);
    let id = plugin.id.clone();
    let toggle = {
        let plugins = plugins.clone();
        let id = id.clone();
        use_callback(move |enabled: bool| {
            let plugins = plugins.clone();
            let id = id.clone();
            pending.set(true);
            failed.set(None);
            spawn(async move {
                let outcome = change(async move { plugins.set_enabled(&id, enabled).await }).await;
                pending.set(false);
                match outcome {
                    Ok(_) => on_changed.call(()),
                    Err(error) => failed.set(Some(error.summary)),
                }
            });
        })
    };
    let uninstall = {
        let plugins = plugins.clone();
        let id = id.clone();
        move |_| {
            let plugins = plugins.clone();
            let id = id.clone();
            pending.set(true);
            failed.set(None);
            spawn(async move {
                let outcome = change(async move { plugins.uninstall(&id).await }).await;
                pending.set(false);
                match outcome {
                    Ok(()) => on_changed.call(()),
                    Err(error) => failed.set(Some(error.summary)),
                }
            });
        }
    };
    let mut first = if plugin.status == PluginStatus::NeedsConsent {
        let mut item = MenuItem::new("review", t!("Review and allow…"));
        item.icon = Some(SHIELD_CHECK);
        item
    } else {
        let mut item = MenuItem::new("toggle", if on { t!("Turn off") } else { t!("Turn on") });
        item.icon = Some(POWER);
        item.disabled = plugin.status == PluginStatus::Invalid || pending();
        item
    };
    first.description = None;
    let mut info = MenuItem::new("details", t!("Details…"));
    info.icon = Some(INFO);
    let mut remove = MenuItem::new("uninstall", t!("Uninstall…"));
    remove.icon = Some(TRASH);
    remove.danger = true;
    let items = vec![
        MenuEntry::Item(first),
        MenuEntry::Item(info),
        MenuEntry::Item(remove),
    ];
    let summary = plugin.clone();
    let for_menu = plugin.clone();
    let for_details = plugin.clone();
    let for_review = plugin.clone();
    rsx! {
        li {
            "data-part": "plugin",
            "data-status": status_name(plugin.status),
            class: "flex flex-wrap items-start justify-between gap-x-4 gap-y-2 border-t border-border py-3 first:border-t-0",
            onmounted: move |event: Event<MountedData>| row.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                menu.set(Some(crate::frame::pointer_anchor(row.peek().as_ref(), &event)));
            },
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items,
                    label: plugin.name.clone(),
                    on_close: move |_| menu.set(None),
                    on_action: move |action: String| {
                        menu.set(None);
                        match action.as_str() {
                            "review" => on_review.call(for_menu.clone()),
                            "toggle" => toggle(!on),
                            "details" => on_details.call(for_menu.clone()),
                            _ => confirming.set(true),
                        }
                    },
                }
            }
            div { class: "min-w-0 flex-[1_1_16rem]",
                div { class: "flex flex-wrap items-center gap-3",
                    h3 { class: "font-semibold", "{plugin.name}" }
                    if let Some(version) = &plugin.version {
                        span { class: "font-mono text-xs text-fg-muted", "{version}" }
                    }
                    StatusDot { tone: status_tone(plugin.status), {status_label(plugin.status)} }
                    if plugin.dev {
                        span {
                            "data-part": "dev-badge",
                            title: t!("Loaded in place from its folder and reloaded on change; not installed"),
                            class: "rounded-control border border-accent-alt/50 px-2 py-0.5 font-mono text-[11px] text-accent-alt",
                            {t!(note = "Badge on a plugin loaded from its development folder", "Dev")}
                        }
                    }
                }
                if let Some(description) = &plugin.description {
                    p { class: "mt-1 text-sm text-fg-muted", "{description}" }
                }
                if plugin.status != PluginStatus::Invalid {
                    p { "data-part": "access", class: "mt-1 text-xs text-fg-muted", {access_summary(&summary)} }
                } else {
                    p { class: "mt-2 text-sm text-danger", {problems(&plugin.errors)} }
                }
                if let Some(error) = &plugin.last_error {
                    p { class: "mt-2 text-xs text-warning", "{error}" }
                }
                if !plugin.warnings.is_empty() {
                    p { class: "mt-2 text-xs text-warning", {problems(&plugin.warnings)} }
                }
                if let Some(error) = failed() {
                    p { role: "alert", class: "mt-2 text-sm text-danger", "{error}" }
                }
            }
            div { "data-part": "plugin-actions", class: "flex flex-wrap items-center gap-2",
                Button {
                    size: ButtonSize::Sm,
                    icon: Some(INFO),
                    "aria-label": t!("{plugin} details", plugin = plugin.name.clone()),
                    on_press: move |_| on_details.call(for_details.clone()),
                    {t!("Details…")}
                }
                if confirming() {
                    Button {
                        variant: ButtonVariant::Danger,
                        size: ButtonSize::Sm,
                        disabled: pending(),
                        on_press: uninstall,
                        {t!("Uninstall {0}", 0 = plugin.name.clone())}
                    }
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::Sm,
                        on_press: move |_| confirming.set(false),
                        {t!(note = "Cancels uninstalling a plugin", "Keep")}
                    }
                } else {
                    Button {
                        size: ButtonSize::Sm,
                        icon: Some(TRASH),
                        on_press: move |_| confirming.set(true),
                        {t!("Uninstall…")}
                    }
                }
                if plugin.status == PluginStatus::NeedsConsent {
                    Button {
                        size: ButtonSize::Sm,
                        on_press: move |_| on_review.call(for_review.clone()),
                        {t!("Review and allow…")}
                    }
                } else {
                    Switch {
                        "aria-label": t!("Enable {plugin}", plugin = plugin.name.clone()),
                        checked: on,
                        disabled: plugin.status == PluginStatus::Invalid || pending(),
                        on_change: move |enabled: bool| toggle(enabled),
                    }
                }
            }
        }
    }
}

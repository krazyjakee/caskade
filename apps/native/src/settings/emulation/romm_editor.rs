//! A RomM server's editor, as the webview's `romm/ServerEditor`: its
//! address, sign-in, what it imports, how its platforms launch and how it
//! syncs. The password or API token is typed here, sent only to save or
//! test, and never shown back.

use caskade_core::error::{ErrorCode, ErrorReport};
use caskade_core::romm::RommPlatformMapping;
use caskade_ui_data::romm::{
    ConnectionReport, ConnectionState, Romm, RommAuth, RommServerConfig, RommSyncReport,
    ServerStatus,
};
use caskade_ui_i18n::{t, tn, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use super::fields::{StackedCell, StackedRow, StackedTable};
use super::romm::{Shown, connected, not_tested_text, sign_in_needed_text, state_name};
use crate::data::change;
use crate::ui::icons::{
    CLOCK, KEY_ROUND, PLUG, POWER, REFRESH_CW, SAVE, SERVER, SHIELD_ALERT, TRASH,
};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Checkbox, ConfirmDialog, DotTone, Segment, SegmentedControl,
    Select, SelectOption, SettingRow, SettingsSection, StatusDot, Svg, Switch, SwitchSize,
    TextField,
};

/// The server address field's example.
const URL_EXAMPLE: &str = "https://romm.example.com";

/// How often a server syncs, in the order offered.
const SYNC_INTERVALS: [&str; 4] = ["manual", "1h", "6h", "24h"];

fn interval_label(id: &str) -> String {
    match id {
        "1h" => t!(note = "How often a RomM server syncs", "Every hour"),
        "6h" => t!(note = "How often a RomM server syncs", "Every 6 hours"),
        "24h" => t!(note = "How often a RomM server syncs", "Daily"),
        _ => t!(note = "How often a RomM server syncs", "Only when I ask"),
    }
}

fn auth_label(auth: RommAuth) -> String {
    match auth {
        RommAuth::Password => t!(
            note = "How Caskade signs in to a RomM server",
            "Username & password"
        ),
        RommAuth::Token => t!(note = "How Caskade signs in to a RomM server", "API token"),
    }
}

/// One RomM platform, as the server reported it or as last saved.
#[derive(Clone, PartialEq)]
struct PlatformRow {
    slug: String,
    name: String,
    rom_count: Option<u32>,
    suggested: Option<String>,
}

fn rows(server: &RommServerConfig, report: Option<&ConnectionReport>) -> Vec<PlatformRow> {
    match report {
        // Platforms without ROMs have nothing to import.
        Some(report) => report
            .platforms
            .iter()
            .filter(|found| found.platform.rom_count > 0)
            .map(|found| PlatformRow {
                slug: found.platform.slug.clone(),
                name: found
                    .platform
                    .custom_name
                    .clone()
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| found.platform.name.clone()),
                rom_count: Some(found.platform.rom_count),
                suggested: found.suggested.clone(),
            })
            .collect(),
        None => server
            .platforms
            .iter()
            .map(|mapping| PlatformRow {
                slug: mapping.slug.clone(),
                name: mapping.slug.clone(),
                rom_count: None,
                suggested: None,
            })
            .collect(),
    }
}

fn mapping_of(server: &RommServerConfig, slug: &str) -> RommPlatformMapping {
    server
        .mapping(slug)
        .cloned()
        .unwrap_or_else(|| RommPlatformMapping {
            slug: slug.to_owned(),
            enabled: true,
            platform: None,
            emulator_id: None,
            profile_id: None,
        })
}

/// Changes platform `slug`'s mapping, adding it first if the server has
/// none yet.
fn set_mapping(
    server: &mut RommServerConfig,
    slug: &str,
    patch: impl FnOnce(&mut RommPlatformMapping),
) {
    match server
        .platforms
        .iter_mut()
        .find(|mapping| mapping.slug == slug)
    {
        Some(mapping) => patch(mapping),
        None => {
            let mut mapping = mapping_of(server, slug);
            patch(&mut mapping);
            server.platforms.push(mapping);
        }
    }
}

/// "Launches with": the platform's default, or any profile that runs it.
fn launch_options(
    platform: &str,
    snapshot: &super::Snapshot,
    default_label: String,
) -> Vec<SelectOption> {
    let mut options = vec![SelectOption::new("default", default_label)];
    for view in &snapshot.emulators {
        for config in &view.emulator.profiles {
            let Some(state) = view.profiles.iter().find(|state| state.id == config.id()) else {
                continue;
            };
            if state.platforms.iter().any(|id| id == platform) {
                options.push(SelectOption::new(
                    format!("{}:{}", view.emulator.id, config.id()),
                    format!("{} · {}", view.emulator.name, state.name),
                ));
            }
        }
    }
    options
}

/// What went wrong in this editor; `sign_in` when the server refused the
/// sign-in rather than couldn't be reached or failed.
#[derive(Clone, PartialEq)]
struct EditorError {
    message: String,
    sign_in: bool,
}

fn editor_error(report: &ErrorReport) -> EditorError {
    EditorError {
        message: report.summary.clone(),
        sign_in: report.code == ErrorCode::Auth,
    }
}

/// The connection's state as the editor knows it.
fn connection(
    status: Option<&ServerStatus>,
    busy: bool,
    report: Option<&ConnectionReport>,
    error: Option<&EditorError>,
) -> Element {
    let failed = || {
        (
            DotTone::Danger,
            t!(note = "A RomM server's status", "Can't connect"),
        )
    };
    let state = status.map(|status| status.state);
    let (tone, text) = if busy {
        (
            DotTone::Warning,
            t!(note = "A RomM server's status", "Connecting…"),
        )
    } else if error.is_some_and(|error| error.sign_in) {
        (
            DotTone::Warning,
            t!(
                note = "A RomM server's status: it refused the username and password or API token",
                "Can't sign in"
            ),
        )
    } else if error.is_some() {
        failed()
    } else if report.is_some() || state == Some(ConnectionState::Connected) {
        let version = report
            .and_then(|report| report.version.clone())
            .or_else(|| status.and_then(|status| status.version.clone()));
        (DotTone::Ok, connected(version.as_deref()))
    } else {
        match state {
            Some(ConnectionState::Offline) => (
                DotTone::Warning,
                t!(note = "A RomM server's status", "Unreachable"),
            ),
            Some(ConnectionState::Error) => failed(),
            Some(ConnectionState::SignInNeeded) => (DotTone::Warning, sign_in_needed_text()),
            _ => (DotTone::Off, not_tested_text()),
        }
    };
    rsx! {
        StatusDot { tone, "{text}" }
    }
}

/// A server's address, sign-in, what it imports and how it syncs. Keyed by
/// its parent: another server, or a new one, starts it over.
#[component]
pub fn ServerEditor(
    initial: RommServerConfig,
    status: Option<ServerStatus>,
    is_new: bool,
    /// Above 0: Sign in was pressed, the password or token field takes
    /// focus (again each time it grows).
    #[props(default)]
    focus_secret: u32,
    snapshot: super::Snapshot,
    romm: Romm,
    shown: Shown,
    on_saved: EventHandler<RommServerConfig>,
    on_removed: EventHandler<()>,
) -> Element {
    let mut shown = shown;
    let mut server = use_signal(|| initial.clone());
    // Never shown back, never logged: sent only to save or test.
    let mut secret = use_signal(String::new);
    let mut report = use_signal(|| None::<ConnectionReport>);
    let mut error = use_signal(|| None::<EditorError>);
    let mut testing = use_signal(|| false);
    let mut syncing = use_signal(|| false);
    let mut synced = use_signal(|| None::<RommSyncReport>);
    // A save, removal or reload under way, and the removal's failure.
    let mut write_pending = use_signal(|| false);
    let mut write_error = use_signal(|| None::<ErrorReport>);
    let mut confirming_remove = use_signal(|| false);
    let mut secret_field = use_signal(|| None::<NodeHandle>);
    use_effect(use_reactive!(|focus_secret| {
        if focus_secret > 0
            && let Some(handle) = secret_field.peek().as_ref()
        {
            handle.doc_mut().set_focus_to(handle.node_id());
        }
    }));
    let typed = move || {
        let text = String::from(secret.peek().as_str());
        (!text.is_empty()).then_some(text)
    };

    let test = {
        let romm = romm.clone();
        move |_| {
            testing.set(true);
            error.set(None);
            let romm = romm.clone();
            let config = server.peek().clone();
            let key = typed();
            spawn(async move {
                match change(async move { romm.test(config, key).await }).await {
                    Ok(found) => report.set(Some(found)),
                    Err(failure) => {
                        report.set(None);
                        error.set(Some(editor_error(&failure)));
                    }
                }
                testing.set(false);
            });
        }
    };
    let save = {
        let romm = romm.clone();
        move |_| {
            error.set(None);
            write_error.set(None);
            write_pending.set(true);
            let romm = romm.clone();
            let config = server.peek().clone();
            let key = typed();
            spawn(async move {
                let saved = config.clone();
                match change(async move { romm.save_server(config, key).await }).await {
                    Ok(next) => {
                        write_pending.set(false);
                        secret.set(String::new());
                        shown.set(Some(Ok(next)));
                        on_saved.call(saved);
                    }
                    Err(failure) => {
                        write_pending.set(false);
                        write_error.set(Some(failure.clone()));
                        error.set(Some(editor_error(&failure)));
                    }
                }
            });
        }
    };
    let sync = {
        let romm = romm.clone();
        move |_| {
            syncing.set(true);
            error.set(None);
            let romm = romm.clone();
            let id = server.peek().id.clone();
            spawn(async move {
                let job = romm.clone();
                match change(async move { job.sync(Some(id)).await }).await {
                    Ok(reports) => {
                        synced.set(reports.into_iter().next());
                        syncing.set(false);
                        write_error.set(None);
                        write_pending.set(true);
                        let result = change(async move { romm.overview().await }).await;
                        write_pending.set(false);
                        match result {
                            Ok(next) => shown.set(Some(Ok(next))),
                            Err(failure) => write_error.set(Some(failure)),
                        }
                    }
                    Err(failure) => {
                        error.set(Some(editor_error(&failure)));
                        syncing.set(false);
                    }
                }
            });
        }
    };
    let remove = move |_| {
        if is_new {
            on_removed.call(());
            return;
        }
        // It takes the server's games and downloads with it (#49). A save's
        // failure shows above; only the removal's shows in the dialog.
        write_error.set(None);
        confirming_remove.set(true);
    };
    let confirm_remove = {
        let romm = romm.clone();
        move |_| {
            write_pending.set(true);
            write_error.set(None);
            let romm = romm.clone();
            let id = server.peek().id.clone();
            spawn(async move {
                match change(async move { romm.remove_server(id).await }).await {
                    Ok(next) => {
                        write_pending.set(false);
                        confirming_remove.set(false);
                        shown.set(Some(Ok(next)));
                        on_removed.call(());
                    }
                    Err(failure) => {
                        write_pending.set(false);
                        write_error.set(Some(failure));
                    }
                }
            });
        }
    };

    let current = server();
    let found = report();
    let failure = error();
    let platforms = rows(&current, found.as_ref());
    let unchanged = !is_new && current == initial && secret.read().is_empty();
    let roms_on_server: u32 = found.as_ref().map_or(0, |found| {
        found
            .platforms
            .iter()
            .map(|row| row.platform.rom_count)
            .sum()
    });
    let connection_status = match &failure {
        Some(failure) if failure.sign_in => "sign_in_needed",
        Some(_) => "error",
        None => status
            .as_ref()
            .map_or("untested", |status| state_name(status.state)),
    };
    let title = if !current.name.is_empty() {
        current.name.clone()
    } else if is_new {
        t!("New server")
    } else {
        t!("Unnamed server")
    };
    let auth_id = format!("romm-{}-auth", current.id);
    let token = current.auth == RommAuth::Token;
    let auth_value = if token { "token" } else { "password" };
    let secret_class = if token { "sm:col-span-2" } else { "" };
    let platform_options: Vec<SelectOption> = snapshot
        .platforms
        .iter()
        .map(|view| SelectOption::new(view.platform.id.clone(), view.platform.name.clone()))
        .collect();
    let interval = SYNC_INTERVALS
        .iter()
        .find(|id| **id == current.sync_interval)
        .copied()
        .unwrap_or("manual");
    let removed_name = if initial.name.is_empty() {
        t!("Unnamed server")
    } else {
        initial.name.clone()
    };

    rsx! {
        SettingsSection {
            icon: SERVER,
            title,
            description: t!("ROMs on this server appear in your library. They download the first time you play them."),
            aside: rsx! {
                Button {
                    id: "romm-remove",
                    variant: ButtonVariant::Danger,
                    size: ButtonSize::Sm,
                    icon: TRASH,
                    on_press: remove,
                    if is_new {
                        {t!(note = "Drops a RomM server that isn't saved yet", "Discard")}
                    } else {
                        {t!(note = "Asks before removing a RomM server", "Remove…")}
                    }
                }
            },
            div { class: "mt-3 grid gap-4 md:grid-cols-2",
                TextField {
                    id: "romm-name",
                    label: tp!("RomM server", "Name"),
                    value: current.name.clone(),
                    on_input: move |value: String| server.write().name = value,
                }
                TextField {
                    id: "romm-url",
                    label: t!("Server address"),
                    mono: true,
                    placeholder: URL_EXAMPLE.to_owned(),
                    value: current.url.clone(),
                    on_input: move |value: String| server.write().url = value,
                }
                div { class: "flex flex-col gap-1.5",
                    span { id: "{auth_id}", class: "text-sm font-medium",
                        {t!(note = "Followed by the ways to sign in to a RomM server", "Sign in with")}
                    }
                    SegmentedControl {
                        id: "romm-auth",
                        "aria-labelledby": auth_id.clone(),
                        options: vec![
                            Segment::new("password", auth_label(RommAuth::Password)),
                            Segment::new("token", auth_label(RommAuth::Token)),
                        ],
                        value: auth_value.to_owned(),
                        on_change: move |value: String| {
                            server.write().auth = if value == "token" {
                                RommAuth::Token
                            } else {
                                RommAuth::Password
                            };
                            secret.set(String::new());
                        },
                        class: "self-start",
                    }
                }
                div { class: "grid gap-4 sm:grid-cols-2",
                    if !token {
                        TextField {
                            id: "romm-username",
                            label: t!("Username"),
                            value: current.username.clone(),
                            on_input: move |value: String| server.write().username = value,
                        }
                    }
                    TextField {
                        id: "romm-secret",
                        label: if token { auth_label(RommAuth::Token) } else { t!("Password") },
                        kind: "password",
                        icon: KEY_ROUND,
                        placeholder: if status.as_ref().is_some_and(|status| status.has_secret) {
                            t!("Saved in keychain")
                        } else {
                            String::new()
                        },
                        value: secret(),
                        on_input: move |value: String| secret.set(value),
                        on_mounted: move |event: Event<MountedData>| {
                            let handle = event.data().downcast::<NodeHandle>().cloned();
                            if focus_secret > 0
                                && let Some(handle) = &handle
                            {
                                handle.doc_mut().set_focus_to(handle.node_id());
                            }
                            secret_field.set(handle);
                        },
                        class: "{secret_class}",
                    }
                }
            }
            div { class: "mt-2",
                SettingRow {
                    icon: SHIELD_ALERT,
                    label: t!("Allow self-signed certificates"),
                    description: t!("For a server on your home network without a public certificate."),
                    Switch {
                        id: "romm-allow-invalid-certs",
                        checked: current.allow_invalid_certs,
                        on_change: move |value: bool| server.write().allow_invalid_certs = value,
                    }
                }
            }
            div {
                "data-part": "connection",
                "data-status": connection_status,
                class: "mt-1 flex flex-wrap items-center gap-3 rounded-control border border-border bg-surface-overlay/50 p-3 data-[status=error]:border-danger/40 data-[status=error]:bg-danger/5 data-[status=sign_in_needed]:border-warning/40 data-[status=sign_in_needed]:bg-warning/5",
                Button {
                    id: "romm-test",
                    icon: PLUG,
                    disabled: testing() || current.url.is_empty(),
                    on_press: test,
                    if testing() {
                        {t!(note = "A RomM server's status", "Connecting…")}
                    } else {
                        {t!("Test connection")}
                    }
                }
                {connection(status.as_ref(), testing(), found.as_ref(), failure.as_ref())}
                if let Some(failure) = &failure {
                    span { role: "alert", class: "text-xs text-danger", "{failure.message}" }
                }
                if found.is_some() {
                    span { class: "text-xs text-fg-muted",
                        {format!(
                            "{} · {}",
                            tn!("# platform", "# platforms", platforms.len()),
                            tn!("# ROM on the server", "# ROMs on the server", roms_on_server)
                        )}
                    }
                }
                span { class: "ml-auto flex items-center gap-1.5 text-xs text-fg-muted",
                    Svg { icon: KEY_ROUND, class: "size-3.5" }
                    {t!("Credentials are kept in the system keychain.")}
                }
            }
            div { class: "mt-6 flex flex-wrap items-center gap-3",
                h3 { class: "font-display font-semibold tracking-wide", {tp!("RomM server", "Platforms")} }
                Switch {
                    id: "romm-import-all",
                    size: SwitchSize::Sm,
                    checked: current.import_all,
                    on_change: move |value: bool| server.write().import_all = value,
                    label: t!("Import everything, including platforms added later"),
                    class: "ml-auto",
                }
            }
            if platforms.is_empty() {
                p { class: "mt-3 rounded-control border border-dashed border-border-strong p-6 text-center text-sm text-fg-muted",
                    if found.is_some() {
                        {t!("This server has no ROMs yet.")}
                    } else {
                        {t!("Test the connection to see this server's platforms.")}
                    }
                }
            } else {
                StackedTable { class: "mt-3",
                    for row in platforms.iter().cloned() {
                        {
                            let mapping = mapping_of(&current, &row.slug);
                            let platform = mapping.platform.clone().or_else(|| row.suggested.clone());
                            let on = platform.is_some() && (current.import_all || mapping.enabled);
                            let launch = match (&mapping.emulator_id, &mapping.profile_id) {
                                (Some(emulator), Some(profile)) => format!("{emulator}:{profile}"),
                                _ => "default".to_owned(),
                            };
                            let auto = match &row.suggested {
                                Some(suggested) => t!(
                                    note = "The Caskade platform matched to a RomM platform",
                                    "Automatic ({platform})",
                                    platform = snapshot.platform_name(suggested)
                                ),
                                None => t!(
                                    note = "No Caskade platform matches a RomM platform",
                                    "Not matched"
                                ),
                            };
                            let mut options = vec![SelectOption::new("auto", auto)];
                            options.extend(platform_options.iter().cloned());
                            let launches = platform
                                .as_deref()
                                .map(|platform| launch_options(platform, &snapshot, t!("Platform default")));
                            let enable_slug = row.slug.clone();
                            let platform_slug = row.slug.clone();
                            let launch_slug = row.slug.clone();
                            rsx! {
                                StackedRow {
                                    key: "{row.slug}",
                                    "data-part": "romm-platform",
                                    "data-enabled": on.then_some("true"),
                                    "data-unmatched": platform.is_none().then_some("true"),
                                    StackedCell { full: true, class: "flex items-start gap-3",
                                        Checkbox {
                                            id: "romm-platform-{row.slug}-import",
                                            "aria-label": t!("Import {platform}", platform = row.name.clone()),
                                            checked: on,
                                            disabled: current.import_all || platform.is_none(),
                                            on_change: move |enabled: bool| {
                                                set_mapping(&mut server.write(), &enable_slug, |mapping| {
                                                    mapping.enabled = enabled;
                                                });
                                            },
                                        }
                                        div {
                                            div { class: "font-medium", "{row.name}" }
                                            span {
                                                "data-part": "chip",
                                                class: "rounded-full bg-surface-hover px-2 py-0.5 font-mono text-[11px] whitespace-nowrap text-fg-muted",
                                                "{row.slug}"
                                            }
                                        }
                                    }
                                    StackedCell {
                                        label: tp!("RomM platform column", "ROMs"),
                                        class: "font-mono text-xs text-fg-muted",
                                        {row.rom_count.map_or_else(|| "—".to_owned(), |count| count.to_string())}
                                    }
                                    StackedCell { label: tp!("RomM platform column", "Caskade platform"),
                                        Select {
                                            id: "romm-platform-{row.slug}-platform",
                                            "aria-label": t!("Caskade platform for {platform}", platform = row.name.clone()),
                                            options,
                                            value: Some(mapping.platform.clone().unwrap_or_else(|| "auto".to_owned())),
                                            on_change: move |value: String| {
                                                set_mapping(&mut server.write(), &platform_slug, |mapping| {
                                                    mapping.platform = (value != "auto").then_some(value);
                                                    mapping.emulator_id = None;
                                                    mapping.profile_id = None;
                                                });
                                            },
                                            class: "w-56",
                                        }
                                    }
                                    StackedCell { label: tp!("RomM platform column", "Launches with"), full: true,
                                        if let Some(launches) = launches {
                                            Select {
                                                id: "romm-platform-{row.slug}-launch",
                                                "aria-label": t!("Emulator for {platform}", platform = row.name.clone()),
                                                options: launches,
                                                value: Some(launch),
                                                disabled: !on,
                                                on_change: move |value: String| {
                                                    let (emulator, profile) = match value.split_once(':') {
                                                        Some((emulator, profile)) if value != "default" => {
                                                            (Some(emulator.to_owned()), Some(profile.to_owned()))
                                                        }
                                                        _ => (None, None),
                                                    };
                                                    set_mapping(&mut server.write(), &launch_slug, |mapping| {
                                                        mapping.emulator_id = emulator;
                                                        mapping.profile_id = profile;
                                                    });
                                                },
                                                class: "w-full max-w-96 min-w-64",
                                            }
                                        } else {
                                            span { class: "text-xs text-fg-muted",
                                                {t!("Match a platform to import these ROMs.")}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(found) = found.as_ref().filter(|found| !found.collections.is_empty()) {
                div { class: "mt-6",
                    h3 { class: "font-display font-semibold tracking-wide", {tp!("RomM server", "Collections")} }
                    p { class: "text-xs text-fg-muted",
                        {t!("Their ROMs are imported even when their platform is off.")}
                    }
                    div { class: "mt-3 flex flex-wrap gap-x-6 gap-y-2",
                        for collection in found.collections.iter().cloned() {
                            div { key: "{collection.id}", class: "flex items-center gap-2",
                                Checkbox {
                                    id: "romm-collection-{collection.id}",
                                    label: collection.name.clone(),
                                    checked: current.import_all || current.collections.contains(&collection.id),
                                    disabled: current.import_all,
                                    on_change: move |selected: bool| {
                                        let mut server = server.write();
                                        if selected {
                                            server.collections.push(collection.id);
                                        } else {
                                            server.collections.retain(|id| *id != collection.id);
                                        }
                                    },
                                }
                                span { class: "font-mono text-xs text-fg-muted", "{collection.rom_ids.len()}" }
                            }
                        }
                    }
                }
            }
            div { class: "mt-6 grid gap-x-8 xl:grid-cols-2",
                SettingRow {
                    icon: POWER,
                    label: t!("Sync on startup"),
                    description: t!("Pick up new and changed ROMs when Caskade opens."),
                    Switch {
                        id: "romm-sync-on-startup",
                        checked: current.sync_on_startup,
                        on_change: move |value: bool| server.write().sync_on_startup = value,
                    }
                }
                SettingRow { icon: CLOCK, label: t!("Sync in the background"),
                    Select {
                        id: "romm-sync-interval",
                        options: SYNC_INTERVALS
                            .iter()
                            .map(|id| SelectOption::new(*id, interval_label(id)))
                            .collect::<Vec<_>>(),
                        value: Some(interval.to_owned()),
                        on_change: move |value: String| server.write().sync_interval = value,
                        class: "w-44",
                    }
                }
            }
            div { class: "mt-4 flex flex-wrap items-center gap-3",
                Button {
                    id: "romm-save",
                    variant: ButtonVariant::Primary,
                    icon: SAVE,
                    disabled: write_pending() || unchanged || current.url.is_empty(),
                    on_press: save,
                    if is_new {
                        {t!("Save server")}
                    } else {
                        {t!("Save changes")}
                    }
                }
                if !is_new {
                    Button {
                        id: "romm-sync",
                        icon: REFRESH_CW,
                        disabled: syncing(),
                        on_press: sync,
                        if syncing() {
                            {t!(note = "A RomM server's status", "Syncing…")}
                        } else {
                            {t!("Sync now")}
                        }
                    }
                }
                if let Some(synced) = synced() {
                    span { "data-part": "sync-report", class: "text-xs text-fg-muted",
                        {t!(
                            note = "What a RomM sync changed: counts of games",
                            "{added} added · {updated} updated · {removed} removed",
                            added = synced.added,
                            updated = synced.updated,
                            removed = synced.removed
                        )}
                        if !synced.unmatched_platforms.is_empty() {
                            {format!(
                                " · {}",
                                t!(
                                    note = "After a RomM sync's counts: RomM platforms, by slug, that match none of Caskade's",
                                    "no Caskade platform for {platforms}",
                                    platforms = synced.unmatched_platforms.join(", ")
                                )
                            )}
                        }
                    }
                }
            }
            if confirming_remove() {
                ConfirmDialog {
                    title: t!("Remove {server}?", server = removed_name.clone()),
                    confirm_label: t!(note = "Removes a RomM server", "Remove"),
                    pending_label: t!("Removing…"),
                    pending: write_pending(),
                    on_confirm: confirm_remove,
                    on_close: move |_| {
                        write_error.set(None);
                        confirming_remove.set(false);
                    },
                    p {
                        {t!("Its games leave your library, the ROMs downloaded from it are deleted and its sign-in is forgotten. Devices that sync RomM servers remove it too.")}
                    }
                    p { {t!("Adding it again brings its games back, to download again.")} }
                    if let Some(report) = write_error() {
                        p { role: "alert", class: "text-danger", "{report.summary}" }
                    }
                }
            }
        }
    }
}

//! Settings → Emulation → RomM server (#1, #105), as the webview's `romm/`
//! `RommPanel`, `CacheSection` and `useRomm`: the servers with their state,
//! the selected server's editor (`romm_editor`), the downloads Play started
//! with their live progress, and the download cache. Passwords and API
//! tokens go straight to the keychain and never come back.

use std::collections::BTreeMap;

use caskade_app::events::{Event, RommDownloadChanged};
use caskade_core::error::{ErrorCode, ErrorReport};
use caskade_ui_data::romm::{
    ConnectionState, DownloadPhase, DownloadProgress, Romm, RommAuth, RommOverview,
    RommServerConfig, RommSettings, ServerStatus,
};
use caskade_ui_i18n::{format, t, tn, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use tokio::sync::broadcast;

use super::romm_editor::ServerEditor;
use crate::data::{DataSource, change};
use crate::settings::sync::SectionSync;
use crate::ui::icons::{
    DOWNLOAD, FOLDER, HARD_DRIVE, HEART, INFO, KEY_ROUND, PENCIL, PLUS, REFRESH_CW, SERVER,
    SHIELD_CHECK, TRASH, X,
};
use crate::ui::interact::{Keys, flag, use_interaction};
use crate::ui::{
    Anchor, Button, ButtonSize, ButtonVariant, DotTone, FailureNotice, Menu, MenuEntry, MenuItem,
    NoticeVariant, SettingRow, SettingsSection, Slider, StatusDot, Svg, Switch, TextField,
};

use super::fields::{StackedCell, StackedTable};

/// A server, its status, whether it is busy and selected, and its last
/// sync's error.
type Row = (
    RommServerConfig,
    Option<ServerStatus>,
    bool,
    bool,
    Option<String>,
);

/// The overview the panel shows: `None` while it first loads.
pub(super) type Shown = Signal<Option<Result<RommOverview, ErrorReport>>>;

/// Loads the overview again. A failure keeps the overview already shown.
pub(super) async fn refresh(romm: Romm, mut shown: Shown) {
    let result = change(async move { romm.overview().await }).await;
    let keep = result.is_err() && matches!(&*shown.peek(), Some(Ok(_)));
    if !keep {
        shown.set(Some(result));
    }
}

/// Newest state per game; finished downloads leave the list, failed ones
/// stay (the webview's `mergeDownload`).
pub(super) fn merge_download(list: &mut Vec<DownloadProgress>, next: DownloadProgress) {
    list.retain(|download| download.entry_id != next.entry_id);
    if !matches!(next.phase, DownloadPhase::Done | DownloadPhase::Cancelled) {
        list.push(next);
    }
}

/// A server id the core accepts: lowercase letters, digits and dashes
/// (the webview's `serverId`).
pub(crate) fn server_id(name: &str, taken: &[String]) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if dash && !slug.is_empty() {
                slug.push('-');
            }
            dash = false;
            slug.push(c);
        } else {
            dash = true;
        }
    }
    let mut base: String = slug.chars().take(24).collect();
    if base.is_empty() {
        base = "romm".to_owned();
    }
    let mut id = base.clone();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// A connection state as themes see it in `data-status`.
pub(super) fn state_name(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Untested => "untested",
        ConnectionState::Connected => "connected",
        ConnectionState::Offline => "offline",
        ConnectionState::Error => "error",
        ConnectionState::SignInNeeded => "sign_in_needed",
    }
}

fn phase_name(phase: DownloadPhase) -> &'static str {
    match phase {
        DownloadPhase::Downloading => "downloading",
        DownloadPhase::Verifying => "verifying",
        DownloadPhase::Unpacking => "unpacking",
        DownloadPhase::Done => "done",
        DownloadPhase::Failed => "failed",
        DownloadPhase::Cancelled => "cancelled",
    }
}

fn phase_label(phase: DownloadPhase) -> String {
    match phase {
        DownloadPhase::Downloading => tp!("download status", "Downloading"),
        DownloadPhase::Verifying => tp!("download status", "Verifying"),
        DownloadPhase::Unpacking => tp!("download status", "Unpacking"),
        DownloadPhase::Done => tp!("download status", "Done"),
        DownloadPhase::Failed => tp!("download status", "Failed"),
        DownloadPhase::Cancelled => tp!("download status", "Cancelled"),
    }
}

/// "Connected", with the server's version when known.
pub(super) fn connected(version: Option<&str>) -> String {
    match version.filter(|version| !version.is_empty()) {
        Some(version) => t!(
            note = "A RomM server's status, with the server's version",
            "Connected · v{version}",
            version = version
        ),
        None => t!(note = "A RomM server's status", "Connected"),
    }
}

pub(super) fn sign_in_needed_text() -> String {
    t!(
        note = "A RomM server's status: this device has no working password or API token for it",
        "Sign-in needed on this device"
    )
}

pub(super) fn not_tested_text() -> String {
    t!(
        note = "A RomM server's status: its connection hasn't been tried",
        "Not tested"
    )
}

/// A server's state in the list.
fn server_state(status: Option<&ServerStatus>) -> Element {
    let (tone, text) = match status {
        Some(status) if status.syncing => (
            DotTone::Warning,
            t!(note = "A RomM server's status", "Syncing…"),
        ),
        Some(status) => match status.state {
            ConnectionState::Connected => (DotTone::Ok, connected(status.version.as_deref())),
            ConnectionState::Offline => (
                DotTone::Warning,
                t!(note = "A RomM server's status", "Unreachable"),
            ),
            ConnectionState::Error => (
                DotTone::Danger,
                t!(note = "A RomM server's status", "Can't connect"),
            ),
            ConnectionState::SignInNeeded => (DotTone::Warning, sign_in_needed_text()),
            ConnectionState::Untested => (DotTone::Off, not_tested_text()),
        },
        None => (DotTone::Off, not_tested_text()),
    };
    rsx! {
        StatusDot { tone, "{text}" }
    }
}

/// A server this device can't sign in to: no saved password or token (they
/// stay on each device), or one the server refused.
fn needs_sign_in(status: Option<&ServerStatus>) -> bool {
    status.is_some_and(|status| status.state == ConnectionState::SignInNeeded)
}

/// Under the status: why the server needs signing in, or what failed. One
/// line per problem: a failed sync replaces the row's recorded error.
fn server_problem(
    server: &RommServerConfig,
    status: Option<&ServerStatus>,
    sync_error: Option<String>,
) -> Element {
    if needs_sign_in(status) {
        // The server refused the saved sign-in: its own reason says why.
        let refused = status
            .filter(|status| status.has_secret)
            .and_then(|status| status.error.clone());
        let text = refused.unwrap_or_else(|| {
            if server.auth == RommAuth::Token {
                t!(
                    "Sign-ins stay on each device. Enter this server's API token here to import its games."
                )
            } else {
                t!("Sign-ins stay on each device. Sign in here to import this server's games.")
            }
        });
        return rsx! {
            div { "data-part": "sign-in-reason", class: "mt-1 max-w-80 text-xs text-fg-muted", "{text}" }
        };
    }
    if let Some(error) = sync_error {
        return rsx! {
            div { role: "alert", class: "mt-1 text-xs text-danger", "{error}" }
        };
    }
    match status.and_then(|status| status.error.clone()) {
        Some(error) => rsx! {
            div { class: "mt-1 text-xs text-fg-muted", "{error}" }
        },
        None => rsx! {},
    }
}

fn sign_in_label(server: &RommServerConfig) -> String {
    if server.auth == RommAuth::Token {
        t!(
            note = "Opens a RomM server's editor at its API token",
            "Enter API token"
        )
    } else {
        t!(
            note = "Opens a RomM server's editor at its password",
            "Sign in"
        )
    }
}

fn last_sync(status: Option<&ServerStatus>) -> String {
    match status.and_then(|status| status.last_sync_ms) {
        Some(ms) => crate::library::date_time(ms),
        None => t!(
            note = "A RomM server's last sync: none yet",
            "Not this session"
        ),
    }
}

/// Settings → Emulation → RomM server: remote ROM libraries. Where RomM
/// didn't start this session, what the webview shows when its commands
/// aren't there.
#[component]
pub fn RommPanel(snapshot: super::Snapshot) -> Element {
    match super::use_romm() {
        Some(romm) => rsx! {
            Panel { romm, snapshot }
        },
        None => rsx! {
            FailureNotice {
                error: ErrorReport::new(
                    ErrorCode::Unavailable,
                    t!("This part of Caskade didn't start this session. Restarting Caskade may fix it."),
                ),
                title: t!("RomM servers couldn't be loaded"),
                variant: NoticeVariant::Inline,
            }
        },
    }
}

#[component]
fn Panel(romm: Romm, snapshot: super::Snapshot) -> Element {
    let source = use_context::<DataSource>();
    let mut shown: Shown = use_signal(|| None);
    let load = {
        let romm = romm.clone();
        use_callback(move |()| {
            spawn(refresh(romm.clone(), shown));
        })
    };
    // The overview, and every download's progress as it comes.
    use_hook(move || {
        load.call(());
        let mut events = source.events.subscribe();
        spawn(async move {
            loop {
                match events.recv().await {
                    Ok(Event::RommDownloadChanged(RommDownloadChanged(download))) => {
                        let ended =
                            matches!(download.phase, DownloadPhase::Done | DownloadPhase::Failed);
                        if let Some(Ok(overview)) = shown.write().as_mut() {
                            merge_download(&mut overview.downloads, download);
                        }
                        if ended {
                            load.call(());
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => load.call(()),
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
    let mut selected_id = use_signal(|| None::<String>);
    // Sign in: opens the server's editor with its password or token field
    // focused; a count, so pressing it again focuses the field again.
    let mut sign_in = use_signal(|| None::<(String, u32)>);
    let mut draft = use_signal(|| None::<RommServerConfig>);
    let mut syncing = use_signal(|| None::<String>);
    // A sync that fails before the core records it on the server's row.
    let mut sync_errors = use_signal(BTreeMap::<String, String>::new);
    // Loading the overview again after a sync.
    let mut write_error = use_signal(|| None::<ErrorReport>);
    let edit = use_callback(move |id: String| {
        draft.set(None);
        selected_id.set(Some(id));
    });
    let start_sign_in = use_callback(move |id: String| {
        edit.call(id.clone());
        let count = sign_in.peek().as_ref().map_or(0, |(_, count)| *count) + 1;
        sign_in.set(Some((id, count)));
    });
    let sync = {
        let romm = romm.clone();
        use_callback(move |id: String| {
            syncing.set(Some(id.clone()));
            sync_errors.write().remove(&id);
            let romm = romm.clone();
            spawn(async move {
                let job = romm.clone();
                let target = id.clone();
                if let Err(report) = change(async move { job.sync(Some(target)).await }).await {
                    sync_errors.write().insert(id, report.summary);
                }
                syncing.set(None);
                write_error.set(None);
                match change(async move { romm.overview().await }).await {
                    Ok(next) => shown.set(Some(Ok(next))),
                    Err(report) => write_error.set(Some(report)),
                }
            });
        })
    };

    let loaded = shown.read().clone();
    let data = match loaded {
        None => {
            return rsx! {
                p { role: "status", class: "text-sm text-fg-muted", {t!("Loading RomM servers…")} }
            };
        }
        Some(Err(report)) => {
            return rsx! {
                FailureNotice {
                    error: report,
                    title: t!("RomM servers couldn't be loaded"),
                    variant: NoticeVariant::Inline,
                    on_retry: move |_| load.call(()),
                }
            };
        }
        Some(Ok(data)) => data,
    };
    let servers = data.settings.servers.clone();
    let status_of = |id: &str| data.servers.iter().find(|server| server.id == id).cloned();
    let is_draft = draft.read().is_some();
    let selected: Option<RommServerConfig> = draft()
        .or_else(|| {
            selected_id().and_then(|id| servers.iter().find(|server| server.id == id).cloned())
        })
        .or_else(|| servers.first().cloned());
    let new_id = server_id(
        "romm",
        &servers
            .iter()
            .map(|server| server.id.clone())
            .collect::<Vec<_>>(),
    );
    let add = {
        let new_id = new_id.clone();
        move |_| draft.set(Some(RommServerConfig::new(new_id.clone(), "", "")))
    };
    let connect = move |_| draft.set(Some(RommServerConfig::new(new_id.clone(), "", "")));
    let rows: Vec<Row> = servers
        .iter()
        .map(|server| {
            let state = status_of(&server.id);
            let busy = syncing.read().as_deref() == Some(server.id.as_str())
                || state.as_ref().is_some_and(|state| state.syncing);
            let active = !is_draft && selected.as_ref().is_some_and(|s| s.id == server.id);
            let error = sync_errors.read().get(&server.id).cloned();
            (server.clone(), state, busy, active, error)
        })
        .collect();
    // The editor starts over for another server, or a new one.
    let editor = selected.clone().map(|initial| {
        let key = format!("{}{}", initial.id, if is_draft { ":new" } else { "" });
        let status = if is_draft {
            None
        } else {
            status_of(&initial.id)
        };
        let focus_secret = match &*sign_in.read() {
            Some((id, count)) if !is_draft && *id == initial.id => *count,
            _ => 0,
        };
        (key, initial, status, focus_secret)
    });

    rsx! {
        div { "data-cs": "romm", class: "flex flex-col gap-4",
            SettingsSection {
                icon: SERVER,
                title: t!("RomM servers"),
                description: t!("Connect a RomM server to add its ROMs to your library. Each game downloads when you first play it and runs with your emulators."),
                aside: rsx! {
                    Button {
                        id: "romm-add",
                        size: ButtonSize::Sm,
                        icon: PLUS,
                        disabled: is_draft,
                        on_press: add,
                        {t!("Add server")}
                    }
                },
                if servers.is_empty() && !is_draft {
                    div { class: "mt-3 flex flex-col items-center gap-3 rounded-control border border-dashed border-border-strong p-8 text-center",
                        Svg { icon: SERVER, class: "size-8 text-fg-subtle" }
                        p { class: "max-w-md text-sm text-fg-muted",
                            {t!("RomM is a self-hosted ROM manager. Connect yours and every ROM on it shows up in your library, with artwork, without downloading anything up front.")}
                        }
                        Button {
                            id: "romm-connect",
                            variant: ButtonVariant::Primary,
                            icon: PLUS,
                            on_press: connect,
                            {t!("Connect a server")}
                        }
                    }
                } else if !servers.is_empty() {
                    StackedTable { "data-cs": "romm-servers", class: "mt-3",
                        for (server, status, busy, active, sync_error) in rows {
                            ServerRow {
                                key: "{server.id}",
                                on_edit: {
                                    let id = server.id.clone();
                                    move |_| edit.call(id.clone())
                                },
                                on_sign_in: {
                                    let id = server.id.clone();
                                    move |_| start_sign_in.call(id.clone())
                                },
                                on_sync: {
                                    let id = server.id.clone();
                                    move |_| sync.call(id.clone())
                                },
                                server,
                                status,
                                busy,
                                active,
                                sync_error,
                            }
                        }
                    }
                }
                if let Some(report) = write_error() {
                    p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
                }
                p { class: "mt-3 flex gap-2 text-xs text-fg-muted",
                    Svg { icon: INFO, class: "size-4 shrink-0" }
                    {t!("Games from a server you can't reach stay in your library. Downloaded ones still play; the rest show as unavailable until the server is back.")}
                }
                SectionSync {
                    namespace: "core.romm",
                    note: t!("Sign-ins don't: each device signs in to a server on its own."),
                }
            }
            for (key, initial, status, focus_secret) in editor {
                ServerEditor {
                    key: "{key}",
                    initial,
                    status,
                    is_new: is_draft,
                    focus_secret,
                    snapshot: snapshot.clone(),
                    romm: romm.clone(),
                    shown,
                    on_saved: move |server: RommServerConfig| {
                        draft.set(None);
                        selected_id.set(Some(server.id));
                    },
                    on_removed: move |_| {
                        draft.set(None);
                        selected_id.set(None);
                    },
                }
            }
            Downloads { romm: romm.clone(), downloads: data.downloads.clone() }
            CacheSection { romm: romm.clone(), overview: data.clone(), shown }
        }
    }
}

/// One server in the list; a right click offers its actions.
#[component]
fn ServerRow(
    server: RommServerConfig,
    status: Option<ServerStatus>,
    busy: bool,
    active: bool,
    sync_error: Option<String>,
    on_edit: EventHandler<()>,
    on_sign_in: EventHandler<()>,
    on_sync: EventHandler<()>,
) -> Element {
    let interaction = use_interaction();
    let mut row = use_signal(|| None::<NodeHandle>);
    let mut menu = use_signal(|| None::<Anchor>);
    let signing_in = needs_sign_in(status.as_ref());
    let name = if server.name.is_empty() {
        server.url.clone()
    } else {
        server.name.clone()
    };
    let mut items = Vec::new();
    if signing_in {
        let mut item = MenuItem::new("sign-in", sign_in_label(&server));
        item.icon = Some(KEY_ROUND);
        items.push(MenuEntry::Item(item));
    }
    let mut sync_item = MenuItem::new("sync", t!("Sync now"));
    sync_item.icon = Some(REFRESH_CW);
    sync_item.disabled = busy;
    items.push(MenuEntry::Item(sync_item));
    let mut edit_item = MenuItem::new("edit", t!("Edit server"));
    edit_item.icon = Some(PENCIL);
    items.push(MenuEntry::Item(edit_item));
    let state = status
        .as_ref()
        .map_or("untested", |status| state_name(status.state));
    let title = if server.name.is_empty() {
        t!(note = "A RomM server without a name", "Unnamed")
    } else {
        server.name.clone()
    };
    let games = format::count(f64::from(status.as_ref().map_or(0, |status| status.games)));
    let press = move || on_edit.call(());
    let sign_in_aria = if server.auth == RommAuth::Token {
        t!("Enter the API token for {server}", server = name.clone())
    } else {
        t!("Sign in to {server}", server = name.clone())
    };
    rsx! {
        div {
            role: "listitem",
            "data-part": "romm-server",
            "data-status": state,
            "data-selected": flag(active),
            class: "grid grid-cols-2 gap-x-4 gap-y-3 border-t border-border p-4 first:border-t-0 data-selected:bg-accent/10",
            onmounted: move |event: dioxus_native::prelude::Event<MountedData>| row.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                menu.set(Some(crate::frame::pointer_anchor(row.peek().as_ref(), &event)));
            },
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items,
                    label: name.clone(),
                    on_close: move |_| menu.set(None),
                    on_action: move |action: String| {
                        menu.set(None);
                        match action.as_str() {
                            "sign-in" => on_sign_in.call(()),
                            "sync" => on_sync.call(()),
                            _ => on_edit.call(()),
                        }
                    },
                }
            }
            StackedCell { label: tp!("RomM server column", "Server"),
                div {
                    id: "romm-server-{server.id}",
                    role: "button",
                    tabindex: "0",
                    "aria-pressed": if active { "true" } else { "false" },
                    "data-hovered": interaction.hovered(),
                    "data-focus-visible": interaction.ring(),
                    class: "rounded-[4px] text-left font-medium outline-none data-focus-visible:focus-ring data-hovered:text-accent-alt",
                    onmouseenter: move |_| interaction.enter(),
                    onmouseleave: move |_| interaction.leave(),
                    onfocus: move |_| interaction.focus(),
                    onblur: move |_| interaction.blur(),
                    onclick: move |_| press(),
                    onkeydown: move |event| {
                        interaction.key_down(&event, Keys::EnterAndSpace, press);
                    },
                    onkeyup: move |event| interaction.key_up(&event, press),
                    "{title}"
                }
                div { class: "font-mono text-xs text-fg-muted", "{server.url}" }
            }
            StackedCell { label: tp!("RomM server column", "Status"),
                {server_state(status.as_ref())}
                {server_problem(&server, status.as_ref(), sync_error.clone())}
            }
            StackedCell { label: tp!("RomM server column", "In library"), class: "font-mono",
                "{games}"
            }
            StackedCell { label: tp!("RomM server column", "Last sync"), class: "text-xs whitespace-nowrap text-fg-muted",
                {last_sync(status.as_ref())}
            }
            StackedCell { full: true, class: "flex justify-end",
                if signing_in {
                    Button {
                        id: "romm-sign-in-{server.id}",
                        "data-part": "sign-in",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Primary,
                        icon: KEY_ROUND,
                        "aria-label": sign_in_aria,
                        on_press: move |_| on_sign_in.call(()),
                        {sign_in_label(&server)}
                    }
                } else {
                    Button {
                        id: "romm-sync-{server.id}",
                        size: ButtonSize::Sm,
                        icon: REFRESH_CW,
                        disabled: busy,
                        "aria-label": t!("Sync {server}", server = name.clone()),
                        on_press: move |_| on_sync.call(()),
                        {t!("Sync now")}
                    }
                }
            }
        }
    }
}

/// Downloads started by Play, with their progress.
#[component]
fn Downloads(romm: Romm, downloads: Vec<DownloadProgress>) -> Element {
    if downloads.is_empty() {
        return rsx! {};
    }
    rsx! {
        SettingsSection { icon: DOWNLOAD, title: tp!("RomM", "Downloads"),
            ul { "aria-label": t!("RomM downloads"), class: "mt-3 flex flex-col gap-3",
                for download in downloads {
                    DownloadRow { key: "{download.entry_id}", romm: romm.clone(), download }
                }
            }
        }
    }
}

#[component]
fn DownloadRow(romm: Romm, download: DownloadProgress) -> Element {
    let mut row = use_signal(|| None::<NodeHandle>);
    let mut menu = use_signal(|| None::<Anchor>);
    let percent = download.total.filter(|total| *total > 0).map(|total| {
        (download.received as f64 / total as f64 * 100.0)
            .round()
            .min(100.0)
    });
    let shown = match percent {
        Some(percent) if download.phase == DownloadPhase::Downloading => {
            format!(" · {}%", percent as u32)
        }
        _ => String::new(),
    };
    let progress = format!(
        "{}{shown} · {}",
        phase_label(download.phase),
        format::bytes(Some(download.received))
    );
    let cancellable = download.phase != DownloadPhase::Failed;
    let cancel = {
        let romm = romm.clone();
        let entry = download.entry_id.clone();
        move || {
            romm.cancel_download(entry.clone());
        }
    };
    let mut items = Vec::new();
    if cancellable {
        let mut item = MenuItem::new("cancel", t!("Cancel download"));
        item.icon = Some(X);
        item.danger = true;
        items.push(MenuEntry::Item(item));
    }
    let width = percent.unwrap_or(0.0);
    let cancel_menu = cancel.clone();
    rsx! {
        li {
            "data-part": "romm-download",
            "data-status": phase_name(download.phase),
            class: "flex flex-col gap-1.5",
            onmounted: move |event: dioxus_native::prelude::Event<MountedData>| row.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                if cancellable {
                    menu.set(Some(crate::frame::pointer_anchor(row.peek().as_ref(), &event)));
                }
            },
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items,
                    label: download.title.clone(),
                    on_close: move |_| menu.set(None),
                    on_action: move |_: String| {
                        menu.set(None);
                        cancel_menu();
                    },
                }
            }
            div { class: "flex items-center gap-3 text-sm",
                span { class: "font-medium", "{download.title}" }
                span { class: "font-mono text-xs text-fg-muted", "{progress}" }
                if let Some(error) = download.error.clone() {
                    span { class: "text-xs text-danger", "{error}" }
                }
                if cancellable {
                    Button {
                        id: "romm-cancel-{download.entry_id}",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        icon: X,
                        class: "ml-auto",
                        on_press: move |_| cancel(),
                        {t!("Cancel")}
                    }
                }
            }
            div { class: "h-1.5 overflow-hidden rounded-full bg-surface-base/80 ring-1 ring-border ring-inset",
                div { class: "h-full bg-accent", style: "width: {width}%" }
            }
        }
    }
}

/// Downloaded ROMs: where they go, how much may stay, and whether
/// favourites stay.
#[component]
fn CacheSection(romm: Romm, overview: RommOverview, shown: Shown) -> Element {
    let settings = overview.settings.cache.clone();
    let mut limit = use_signal(|| f64::from(settings.limit_gb));
    let mut dir = use_signal(|| settings.dir.clone());
    let mut error = use_signal(|| None::<String>);
    let save = {
        let romm = romm.clone();
        use_callback(move |next: RommSettings| {
            let romm = romm.clone();
            let mut shown = shown;
            spawn(async move {
                match change(async move { romm.set_settings(next).await }).await {
                    Ok(overview) => shown.set(Some(Ok(overview))),
                    Err(report) => error.set(Some(report.summary)),
                }
            });
        })
    };
    let clear = {
        let romm = romm.clone();
        move |_| {
            let romm = romm.clone();
            spawn(async move {
                let job = romm.clone();
                match change(async move { job.clear_cache().await }).await {
                    Ok(_) => refresh(romm, shown).await,
                    Err(report) => error.set(Some(report.summary)),
                }
            });
        }
    };
    let cache = overview.cache.clone();
    let used = match cache.limit {
        Some(limit) if limit > 0 => (cache.used as f64 / limit as f64 * 100.0).min(100.0),
        _ => 0.0,
    };
    let games = tn!("# game", "# games", cache.games.len());
    let limit_text = match cache.limit {
        Some(limit) => t!(
            note = "The download cache's size limit, as a size",
            "limit {size}",
            size = format::bytes(Some(limit))
        ),
        None => t!(note = "The download cache has no size limit", "no limit"),
    };
    let base = overview.settings.clone();
    let commit_dir = {
        let base = base.clone();
        move |_: String| {
            let typed = dir.peek().clone();
            if typed != base.cache.dir {
                let mut next = base.clone();
                next.cache.dir = typed.trim().to_owned();
                save.call(next);
            }
        }
    };
    let commit_limit = {
        let base = base.clone();
        move |value: f64| {
            let mut next = base.clone();
            next.cache.limit_gb = value.round() as u32;
            save.call(next);
        }
    };
    let keep_favorites = {
        let base = base.clone();
        move |keep: bool| {
            let mut next = base.clone();
            next.cache.keep_favorites = keep;
            save.call(next);
        }
    };
    let verify = move |verify: bool| {
        let mut next = base.clone();
        next.cache.verify = verify;
        save.call(next);
    };
    rsx! {
        SettingsSection {
            icon: HARD_DRIVE,
            title: t!("Download cache"),
            description: t!("ROMs download the first time you play them and stay here, so they start instantly and work offline."),
            aside: rsx! {
                Button {
                    id: "romm-cache-clear",
                    size: ButtonSize::Sm,
                    variant: ButtonVariant::Danger,
                    icon: TRASH,
                    disabled: cache.games.iter().all(|game| game.pinned),
                    on_press: clear,
                    {t!("Clear Cache")}
                }
            },
            div { class: "mt-3",
                div {
                    "data-part": "cache-usage",
                    class: "h-2 overflow-hidden rounded-full bg-surface-base/80 ring-1 ring-border ring-inset",
                    div {
                        class: "h-full bg-linear-to-r from-accent to-accent-alt transition-[width] duration-(--cs-motion-duration-base)",
                        style: "width: {used}%",
                    }
                }
                div { class: "mt-2 flex justify-between font-mono text-[11px] text-fg-subtle",
                    span { {format!("{games} · {}", format::bytes(Some(cache.used)))} }
                    span { "{limit_text}" }
                }
                if let Some(error) = error() {
                    p { role: "alert", class: "mt-2 text-xs text-danger", "{error}" }
                }
            }
            div { class: "mt-2 grid gap-x-8 xl:grid-cols-2",
                div {
                    SettingRow {
                        icon: FOLDER,
                        label: tp!("RomM download cache", "Location"),
                        description: t!(
                            note = "romm-cache is a folder name; keep it as is",
                            "Empty uses romm-cache on your default storage location. Now: {folder}",
                            folder = cache.dir.to_string_lossy().into_owned()
                        ),
                        TextField {
                            id: "romm-cache-dir",
                            mono: true,
                            placeholder: t!(note = "The default cache folder is used", "Default"),
                            value: dir(),
                            on_input: move |next: String| dir.set(next),
                            on_submit: commit_dir,
                            class: "w-64",
                        }
                    }
                    SettingRow {
                        icon: TRASH,
                        label: t!("Size limit"),
                        description: t!("The least recently played games are removed first. 0 is no limit."),
                        Slider {
                            id: "romm-cache-limit",
                            min: 0.0,
                            max: 1000.0,
                            step: 10.0,
                            value: limit(),
                            on_change: move |value: f64| limit.set(value),
                            on_change_end: commit_limit,
                            format: Callback::new(|value: f64| {
                                if value == 0.0 {
                                    t!(note = "The download cache's size limit", "No limit")
                                } else {
                                    t!(note = "A size in gigabytes", "{value} GB", value = value.round() as u32)
                                }
                            }),
                        }
                    }
                }
                div {
                    SettingRow { icon: HEART, label: t!("Never remove favourites"),
                        Switch {
                            id: "romm-keep-favorites",
                            checked: settings.keep_favorites,
                            on_change: keep_favorites,
                        }
                    }
                    SettingRow {
                        icon: SHIELD_CHECK,
                        label: t!("Verify downloads"),
                        description: t!("Check each file against the hash RomM reports."),
                        Switch { id: "romm-verify", checked: settings.verify, on_change: verify }
                    }
                }
            }
        }
    }
}

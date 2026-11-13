//! Settings → General (#104), as the webview's `GeneralSettings`: startup,
//! the window while you play, where Caskade keeps things, language and
//! time, Caskade's own updates (`AppUpdateSection`) beside game updates,
//! what goes online, and first-run setup again. One reset covers every
//! preference here, from whichever section keeps it (#44).

use blitz_dom::ScrollBehavior;
use caskade_app::commands::app_update::{AppUpdatePhase, AppUpdateStatus};
use caskade_ui_data::Key;
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use serde_json::Value;

use super::reset::{ResetDefaults, ResetField, ResetPart};
use super::schema::SchemaSettings;
use super::state::flag;
use super::state::{
    SectionState, SettingsFeedback, SettingsLoading, SettingsStatus, use_settings_section,
};
use crate::data::use_settings_query;
use crate::router::{Route, use_router};
use crate::ui::icons::{
    CIRCLE_ARROW_UP, CLOCK, CLOUD, DOWNLOAD, FOLDER_OPEN, GAMEPAD_2, GLOBE, HARD_DRIVE, HOUSE,
    IMAGE_DOWN, LANGUAGES, LOG_OUT, MESSAGE_SQUARE_WARNING, MONITOR_PLAY, MONITOR_UP, PLAY, POWER,
    REFRESH_CW, ROCKET, ROTATE_CCW, ROTATE_CW, SPARKLES, WRENCH,
};
use crate::ui::{
    Button, ButtonVariant, Progress, Segment, SegmentedControl, Select, SelectOption, SettingRow,
    SettingsSection, Svg, Switch, TextField,
};

/// The Language setting's value for the OS's language.
const SYSTEM: &str = "system";

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn select(options: &[(&str, String)]) -> Vec<SelectOption> {
    options
        .iter()
        .map(|(id, label)| SelectOption::new(*id, label.clone()))
        .collect()
}

fn segments(options: &[(&str, String)]) -> Vec<Segment> {
    options
        .iter()
        .map(|(id, label)| Segment::new(*id, label.clone()))
        .collect()
}

/// General: `sub` is `updates` when a link asks for Caskade's updates (#68).
#[component]
pub fn GeneralSettings(sub: Option<String>) -> Element {
    let section = use_settings_section("core.general");
    let router = use_router();
    let Some(settings) = section.value() else {
        return rsx! {
            SettingsLoading { error: section.load_error(), on_reload: move |_| section.reload() }
        };
    };
    let set = move |key: &'static str, value: Value| section.set_field(key, value);
    let launch_on_login = flag(&settings, "launchOnLogin");
    let start_views = [
        ("home", tp!("navigation", "Home")),
        ("library", t!("Library")),
        ("collections", t!("Collections")),
        ("last", t!("Last opened view")),
    ];
    let on_launch = [
        ("stay", t!("Stay open")),
        ("minimize", tp!("window", "Minimize")),
        ("tray", t!("Hide to tray")),
    ];
    let on_close = [
        ("quit", tp!("close button setting", "Quit")),
        ("tray", t!("Hide to tray")),
    ];
    let times = [
        (
            "system",
            t!(
                note = "Time format: 12 or 24 hours, whichever the app's language uses",
                "By language"
            ),
        ),
        ("12h", t!("12-hour")),
        ("24h", t!("24-hour")),
    ];
    // Every preference on this page, from whichever section keeps it (#44);
    // first-run setup's completion and the console's last mode stay.
    let field = |key: &str, label: String| ResetField {
        key: key.to_owned(),
        label,
    };
    let part = |namespace: &str, whole: bool, fields: Vec<ResetField>| ResetPart {
        namespace: namespace.to_owned(),
        whole,
        fields,
    };
    let reset_parts = vec![
        part(
            "core.general",
            true,
            vec![
                field("launchOnLogin", t!("Launch on system startup")),
                field("startMinimized", t!("Start minimized")),
                field("startView", t!("Open to")),
                field("onLaunch", t!("When a game launches")),
                field("onClose", t!("Close button")),
                field("confirmQuit", t!("Confirm before quitting")),
                field("cacheDir", t!("Cache folder")),
                field("language", t!("Language")),
                field("timeFormat", t!("Time format")),
            ],
        ),
        part("core.console", false, vec![field("start", t!("Start in"))]),
        part(
            "core.appUpdates",
            false,
            vec![field("check", t!("Check automatically"))],
        ),
        part(
            "core.metadata",
            false,
            vec![field("online", t!("Fetch artwork and game info online"))],
        ),
        part(
            "core.ports",
            false,
            vec![field("enabled", t!("Show ports of games"))],
        ),
    ];
    let cache_dir = text(&settings, "cacheDir");
    let wanted = sub.as_deref() == Some("updates");

    // Two columns only where the panel has room beside both navigation
    // columns (the webview's container query at 72rem, plus the sidebar's
    // and the section list's width).
    rsx! {
        div { "data-settings": "general",
            div { class: "grid items-start gap-4 min-[102rem]:grid-cols-2",
                div { class: "col-span-full flex items-center justify-between gap-3",
                    SettingsFeedback { error: section.error(), saving: section.saving() }
                    ResetDefaults {
                        group: tp!("settings tab", "General"),
                        label: t!("Reset general settings"),
                        parts: reset_parts,
                        keeps: t!("Game updates, which has its own reset, settings on other pages and whether first-run setup is done stay as they are."),
                        disabled: section.saving(),
                    }
                }
                SettingsSection {
                    icon: Some(POWER),
                    title: t!("Startup"),
                    description: t!("What happens when Caskade opens."),
                    SettingRow {
                        icon: Some(ROCKET),
                        label: t!("Launch on system startup"),
                        description: t!("Start Caskade when you log in."),
                        Switch {
                            checked: launch_on_login,
                            on_change: move |next: bool| set("launchOnLogin", Value::Bool(next)),
                        }
                    }
                    SettingRow {
                        icon: Some(MONITOR_UP),
                        label: t!("Start minimized"),
                        // A disabled switch says why (#55).
                        description: if launch_on_login {
                            t!("Open in the system tray instead of a window.")
                        } else {
                            t!("Open in the system tray instead of a window. Needs Launch on system startup.")
                        },
                        Switch {
                            checked: flag(&settings, "startMinimized"),
                            disabled: !launch_on_login,
                            on_change: move |next: bool| set("startMinimized", Value::Bool(next)),
                        }
                    }
                    SettingRow {
                        icon: Some(HOUSE),
                        label: t!("Open to"),
                        description: t!("The first view you see."),
                        Select {
                            options: select(&start_views),
                            value: Some(text(&settings, "startView")),
                            on_change: move |next: String| set("startView", Value::String(next)),
                            class: "w-44",
                        }
                    }
                    ConsoleStartRow {}
                }
                SettingsSection {
                    icon: Some(GAMEPAD_2),
                    title: t!("Behaviour"),
                    description: t!("What the window does while you play."),
                    SettingRow {
                        icon: Some(PLAY),
                        label: t!("When a game launches"),
                        description: t!("What Caskade does with its window."),
                        Select {
                            options: select(&on_launch),
                            value: Some(text(&settings, "onLaunch")),
                            on_change: move |next: String| set("onLaunch", Value::String(next)),
                            class: "w-44",
                        }
                    }
                    SettingRow {
                        icon: Some(LOG_OUT),
                        label: t!("Close button"),
                        description: t!("Hiding keeps the guide button and downloads working."),
                        SegmentedControl {
                            options: segments(&on_close),
                            value: text(&settings, "onClose"),
                            on_change: move |next: String| set("onClose", Value::String(next)),
                        }
                    }
                    SettingRow {
                        icon: Some(MESSAGE_SQUARE_WARNING),
                        label: t!("Confirm before quitting"),
                        description: t!("Ask while a game is running."),
                        Switch {
                            checked: flag(&settings, "confirmQuit"),
                            on_change: move |next: bool| set("confirmQuit", Value::Bool(next)),
                        }
                    }
                }
                SettingsSection {
                    icon: Some(HARD_DRIVE),
                    title: t!("Locations"),
                    description: t!("Where Caskade installs games and keeps its data."),
                    div { class: "mt-3 flex flex-col gap-4",
                        div { "data-part": "storage-summary", class: "flex flex-col gap-1.5",
                            span { class: "text-sm font-medium", {t!("Game storage")} }
                            div { class: "flex gap-2",
                                div { class: "flex h-9 min-w-0 flex-1 items-center gap-2 rounded-control border border-border bg-surface-base/30 px-3",
                                    Svg { icon: HARD_DRIVE, class: "size-4 shrink-0 text-fg-subtle" }
                                    span { class: "truncate text-sm", {t!("Storage locations")} }
                                    crate::flows::storage::StorageSummary {}
                                }
                                Button {
                                    on_press: move |_| {
                                        router
                                            .navigate(
                                                &Route::Settings {
                                                    tab: "storage",
                                                    section: None,
                                                }
                                                    .href(),
                                            )
                                    },
                                    {t!("Manage")}
                                }
                            }
                            span { class: "text-xs text-fg-muted",
                                {t!("Manage the default location and other game storage roots.")}
                            }
                        }
                        TextField {
                            label: t!("Cache folder"),
                            description: t!("Artwork, thumbnails and game details that Caskade can download again."),
                            icon: Some(FOLDER_OPEN),
                            mono: true,
                            value: cache_dir,
                            readonly: true,
                            on_input: |_| {},
                        }
                    }
                }
                SettingsSection {
                    icon: Some(GLOBE),
                    title: t!("Language & Region"),
                    description: t!("How text, dates and times are shown."),
                    SettingRow { icon: Some(LANGUAGES), label: t!("Language"),
                        Select {
                            options: language_options(&text(&settings, "language")),
                            value: Some(text(&settings, "language")),
                            on_change: move |next: String| set("language", Value::String(next)),
                            class: "w-56",
                        }
                    }
                    SettingRow { icon: Some(CLOCK), label: t!("Time format"),
                        SegmentedControl {
                            options: segments(&times),
                            value: text(&settings, "timeFormat"),
                            on_change: move |next: String| set("timeFormat", Value::String(next)),
                        }
                    }
                }
                AppUpdateSection { scroll_to: wanted }
                // Beside Caskade's own updates, not under Installer (#54).
                SchemaSettings {
                    namespace: "core.updates",
                    title: t!("Game updates"),
                    icon: Some(REFRESH_CW),
                    description: t!("Games installed from GitHub are checked for new releases without an account. Updating keeps your saves and settings."),
                    reset_label: t!("Reset update settings"),
                }
                OnlineSection {}
                SetupSection {}
            }
        }
    }
}

/// The Language setting's choices, as the webview's `useLanguageOptions`:
/// System, showing what it resolves to, then each language by its own
/// name with how much of it is translated. `current` is kept when it isn't
/// one of them.
pub fn language_options(current: &str) -> Vec<SelectOption> {
    let locale = caskade_app::commands::i18n::locale_current();
    let mut options = vec![SelectOption::new(
        SYSTEM,
        t!(
            note = "The Language setting's first choice: the OS's language, then the one it means",
            "System ({systemName})",
            systemName = caskade_ui_i18n::language_name(&locale.system)
        ),
    )];
    for tag in &locale.available {
        let name = caskade_ui_i18n::language_name(tag);
        let label = match caskade_ui_i18n::translated(tag) {
            Some(share) if share < 1.0 => t!(
                note = "A language in the Language setting, and how much of Caskade is translated to it",
                "{name} ({percent}% translated)",
                name = name,
                percent = (share * 100.0).floor() as u64
            ),
            _ => name,
        };
        options.push(SelectOption::new(tag.clone(), label));
    }
    if !current.is_empty() && !options.iter().any(|option| option.value == current) {
        options.push(SelectOption::new(
            current,
            t!(
                note = "A language chosen earlier that this version of Caskade doesn't have",
                "{name} (not available, shown in English)",
                name = caskade_ui_i18n::language_name(current)
            ),
        ));
    }
    options
}

/// `core.console` (#148): which mode Caskade opens in.
#[component]
fn ConsoleStartRow() -> Element {
    let section = use_settings_section("core.console");
    let Some(settings) = section.value() else {
        return rsx! {
            SettingsStatus { section, what: t!("Start in") }
        };
    };
    let starts = [
        ("auto", tp!("start mode", "Automatic")),
        ("desktop", tp!("mode", "Desktop")),
        ("console", t!("Console mode")),
    ];
    rsx! {
        SettingRow {
            icon: Some(MONITOR_PLAY),
            label: t!("Start in"),
            description: t!("Automatic opens console mode in Steam Deck game mode (gamescope), otherwise the mode you last used."),
            Select {
                options: select(&starts),
                value: Some(text(&settings, "start")),
                on_change: move |next: String| section.set_field("start", Value::String(next)),
                class: "w-44",
            }
        }
        SettingsStatus { section, what: t!("Start in") }
    }
}

/// Space kept above a section scrolled to (`scroll-mt-4`).
const SCROLL_MARGIN: f64 = 16.0;

/// Scrolls the nearest scrolling box around `node` so it starts at the
/// top, as the webview's `scrollIntoView({ block: "start" })`: Blitz's own
/// scrolls only the viewport, and Settings scrolls inside its panel.
fn scroll_to_start(doc: &mut blitz_dom::BaseDocument, node: blitz_dom::NodeId) {
    let scrolls = |doc: &blitz_dom::BaseDocument, id: blitz_dom::NodeId| {
        doc.get_node(id).is_some_and(|node| {
            let overflow = node
                .primary_styles()
                .map(|styles| format!("{:?}", styles.get_box().overflow_y));
            matches!(overflow.as_deref(), Some("Auto" | "Scroll"))
                && node.scroll_height() > node.client_height() + 0.5
        })
    };
    let mut at = doc.get_node(node).and_then(|node| node.parent);
    while let Some(here) = at {
        if scrolls(doc, here) {
            let (Some(target), Some(scroller)) = (
                doc.get_client_bounding_rect(node),
                doc.get_client_bounding_rect(here),
            ) else {
                return;
            };
            doc.scroll_by(
                here,
                0.0,
                target.y - scroller.y - SCROLL_MARGIN,
                ScrollBehavior::Instant,
            );
            return;
        }
        at = doc.get_node(here).and_then(|node| node.parent);
    }
}

/// What the status line under the version says.
fn describe(status: &AppUpdateStatus) -> String {
    if let Some(unavailable) = &status.unavailable {
        return unavailable.clone();
    }
    let version = status
        .release
        .as_ref()
        .map(|release| release.version.clone())
        .unwrap_or_default();
    match status.phase {
        AppUpdatePhase::Idle => t!("Not checked yet."),
        AppUpdatePhase::Checking => t!("Checking for updates…"),
        AppUpdatePhase::UpToDate => match status.checked_at {
            None => t!("Caskade is up to date."),
            Some(ms) => t!(
                "Caskade is up to date. Checked {time}.",
                time = crate::library::date_time(ms)
            ),
        },
        AppUpdatePhase::Available => t!("Version {version} is available.", version = version),
        AppUpdatePhase::Downloading => t!("Downloading version {version}…", version = version),
        AppUpdatePhase::Installing => t!("Installing version {version}…", version = version),
        AppUpdatePhase::Ready => t!(
            "Version {version} is installed. Restart Caskade to use it.",
            version = version
        ),
        AppUpdatePhase::Failed => status
            .error
            .clone()
            .unwrap_or_else(|| t!("The update failed.")),
    }
}

/// Whether there's a release to download and install.
fn can_install(status: &AppUpdateStatus) -> bool {
    status.release.is_some()
        && matches!(
            status.phase,
            AppUpdatePhase::Available | AppUpdatePhase::Failed
        )
}

/// Caskade's own updates (`core.appUpdates`): the version running, what
/// the last check found, and installing it. `scroll_to` when a link asks
/// for them (#68): once they've loaded, so the panel above has its height.
#[component]
pub fn AppUpdateSection(#[props(default)] scroll_to: bool) -> Element {
    let status = use_settings_query(
        Key::AppUpdate,
        |_| async move { Ok(crate::updater::status()) },
    );
    let preferences = use_settings_section("core.appUpdates");
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None::<String>);
    let mut anchor = use_signal(|| None::<NodeHandle>);
    let mut scrolled = use_signal(|| false);
    use_effect(move || {
        let Some(handle) = anchor.read().clone() else {
            return;
        };
        if !scroll_to || *scrolled.peek() {
            return;
        }
        scrolled.set(true);
        // Laid out first: a node mounted this frame has no box yet.
        let mut doc = handle.doc_mut();
        doc.resolve(0.0);
        scroll_to_start(&mut doc, handle.node_id());
    });
    let title = t!("Caskade updates");
    let description = t!(
        "New versions of Caskade itself. Updates are signed, and nothing installs until you ask."
    );
    let Some(Ok(status)) = status.read().clone() else {
        return rsx! {};
    };
    let busy = pending() || status.phase.busy();
    let working = matches!(
        status.phase,
        AppUpdatePhase::Downloading | AppUpdatePhase::Installing
    );
    let version = status
        .release
        .as_ref()
        .map(|release| release.version.clone())
        .unwrap_or_default();
    let notes = status
        .release
        .as_ref()
        .and_then(|release| release.notes.clone())
        .filter(|_| status.phase != AppUpdatePhase::UpToDate);
    let check = move |_| {
        pending.set(true);
        failed.set(None);
        spawn(async move {
            let outcome = caskade_app::runtime::spawn(crate::updater::check()).await;
            pending.set(false);
            match outcome {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => failed.set(Some(error)),
                Err(error) => failed.set(Some(error.to_string())),
            }
        });
    };
    let install = move |_| {
        failed.set(None);
        if let Err(error) = crate::updater::install() {
            failed.set(Some(error));
        }
    };
    let restart = move |_| {
        failed.set(None);
        if let Err(error) = crate::updater::restart() {
            failed.set(Some(error));
        }
    };
    let action = if status.phase == AppUpdatePhase::Ready {
        rsx! {
            Button {
                variant: ButtonVariant::Primary,
                icon: Some(ROTATE_CW),
                disabled: busy,
                on_press: restart,
                {t!("Restart now")}
            }
        }
    } else if can_install(&status) {
        rsx! {
            Button {
                variant: ButtonVariant::Primary,
                icon: Some(DOWNLOAD),
                disabled: busy,
                on_press: install,
                if status.phase == AppUpdatePhase::Failed {
                    {t!("Try again")}
                } else {
                    {t!("Download and install")}
                }
            }
        }
    } else {
        rsx! {
            Button {
                disabled: busy || status.unavailable.is_some(),
                on_press: check,
                if status.phase == AppUpdatePhase::Checking {
                    {t!("Checking…")}
                } else {
                    {t!("Check now")}
                }
            }
        }
    };
    let progress = working.then(|| {
        let determinate = status.total.filter(|total| *total > 0);
        let ratio = determinate.map(|total| (status.downloaded as f64 / total as f64).min(1.0));
        let shown = match ratio {
            Some(ratio) => format::percent(Some(ratio * 100.0)),
            None => format::size(Some(status.downloaded)),
        };
        let doing = if status.phase == AppUpdatePhase::Installing {
            t!("Installing…")
        } else {
            t!("Downloading…")
        };
        (ratio, doing, shown)
    });
    rsx! {
        div {
            "data-anchor": "updates",
            class: "scroll-mt-4",
            onmounted: move |event| anchor.set(event.data().downcast::<NodeHandle>().cloned()),
        SettingsSection { icon: Some(CIRCLE_ARROW_UP), title, description,
            SettingRow {
                icon: Some(REFRESH_CW),
                label: t!("Caskade v{version}", version = status.current_version.clone()),
                description: describe(&status),
                {action}
            }
            if let Some((ratio, doing, shown)) = progress {
                div { class: "mt-3",
                    div { class: "mb-2 flex justify-between text-xs text-fg-muted",
                        span { "{doing}" }
                        span { "{shown}" }
                    }
                    Progress { value: ratio, aria_label: t!("Update download") }
                }
            }
            if let Some(error) = failed() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            if let Some(notes) = notes {
                div { class: "mt-3 flex flex-col gap-1.5",
                    span { class: "text-sm font-medium",
                        {t!("What's new in {version}", version = version.clone())}
                    }
                    p { class: "max-h-48 overflow-y-auto whitespace-pre-line rounded-control border border-border bg-surface-base/30 px-3 py-2 text-xs text-fg-muted",
                        "{notes}"
                    }
                }
            }
            if let Some(settings) = preferences.value().filter(|_| status.unavailable.is_none()) {
                SettingRow {
                    icon: Some(REFRESH_CW),
                    label: t!("Check automatically"),
                    description: t!("Look for a new version when Caskade starts and every six hours."),
                    Switch {
                        checked: flag(&settings, "check"),
                        on_change: move |next: bool| preferences.set_field("check", Value::Bool(next)),
                    }
                }
            }
            SettingsStatus { section: preferences, what: t!("Check automatically") }
        }
        }
    }
}

/// Looking games up online, in a card of its own rather than under Setup
/// (#54).
#[component]
fn OnlineSection() -> Element {
    let metadata = use_settings_section("core.metadata");
    let ports = use_settings_section("core.ports");
    let toggle = |section: SectionState, key: &'static str| {
        move |next: bool| section.set_field(key, Value::Bool(next))
    };
    rsx! {
        SettingsSection {
            icon: Some(CLOUD),
            title: tp!("settings section", "Online"),
            description: t!("What Caskade looks up on the internet."),
            if let Some(settings) = metadata.value() {
                SettingRow {
                    icon: Some(IMAGE_DOWN),
                    label: t!("Fetch artwork and game info online"),
                    description: t!("Covers, descriptions and genres from IGDB and Hasheous, looked up in the background."),
                    Switch { checked: flag(&settings, "online"), on_change: toggle(metadata, "online") }
                }
            }
            if let Some(settings) = ports.value() {
                SettingRow {
                    icon: Some(WRENCH),
                    label: t!("Show ports of games"),
                    description: t!("Fan decompilations, recompilations and source ports on game pages, from Decomp Buddy's install plans, checked daily."),
                    Switch { checked: flag(&settings, "enabled"), on_change: toggle(ports, "enabled") }
                }
            }
            SettingsStatus { section: ports, what: t!("Ports of games") }
            SettingsStatus { section: metadata, what: t!("Online lookups") }
        }
    }
}

/// First-run setup again (`core.onboarding`).
#[component]
fn SetupSection() -> Element {
    let onboarding = use_settings_section("core.onboarding");
    rsx! {
        SettingsSection {
            icon: Some(SPARKLES),
            title: tp!("settings section", "Setup"),
            description: t!("The setup Caskade starts with."),
            if onboarding.value().is_some() {
                SettingRow {
                    icon: Some(ROTATE_CCW),
                    label: t!("First-run setup"),
                    description: t!("Choose your games folder, what to look for and how you play again."),
                    Button {
                        disabled: onboarding.saving(),
                        on_press: move |_| onboarding.set_field("completed", Value::Bool(false)),
                        {t!("Run setup again")}
                    }
                }
            }
            SettingsStatus { section: onboarding, what: t!("First-run setup") }
        }
    }
}

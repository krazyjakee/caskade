//! Settings → Streaming (#265, #105), as the webview's `StreamingSettings`
//! and `PairDialog`: letting other devices stream games from this one
//! (admins), the devices paired with it and the ones waiting to pair; the
//! devices this one can stream from, pairing with them and adding one by
//! its address; the stream quality; and the streaming programs' updates.
//!
//! Every account sees the tab, can pair this device with another to stream
//! from and sets the quality for this device (#66); only an admin hosts or
//! accepts pairings (the core refuses the rest whatever this shows). No
//! technology's name appears: Caskade does the streaming.

use std::time::Duration;

use caskade_core::error::ErrorReport;
use caskade_ui_data::streaming::{
    HostOutcome, HostReach, HostState, HostStatus, PairRequest, PairedClient, PairingOutcome,
    PairingStarted, StreamHost, StreamQuality, Streaming, StreamingPart, StreamingProgram,
};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use serde_json::Value;

use super::state::{SettingsStatus, use_settings_section};
use crate::data::change;
use crate::flows::streaming::PairRequestForm;
use crate::ui::icons::{CAST, DOWNLOAD, GAUGE, MONITOR_PLAY, PLUS, REFRESH_CW, TRASH, WRENCH};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Dialog, DotTone, FailureNotice, NoticeVariant, Select,
    SelectOption, SettingRow, SettingsSection, StatusDot, Switch, TextField,
};

/// How often hosting, the waiting devices and the devices to stream from
/// are read again, as the webview's queries refetch them.
const TICK: Duration = Duration::from_secs(1);
const STATUS_EVERY: u32 = 5;
const REQUESTS_EVERY: u32 = 3;
const HOSTS_EVERY: u32 = 15;

/// The app's streaming: a test's, else the running app's.
pub fn use_streaming() -> Option<Streaming> {
    try_use_context::<Streaming>().or_else(|| {
        crate::shell::services().map(|services| {
            Streaming::of(
                services,
                crate::shell::sink(),
                crate::shell::events().clone(),
            )
        })
    })
}

async fn sleep(time: Duration) {
    // Timers need the app runtime's reactor; Dioxus's executor has none.
    let _ = caskade_app::runtime::spawn(async move { tokio::time::sleep(time).await }).await;
}

/// What the panel shows, read from the streaming host and client.
#[derive(Clone, Copy, PartialEq)]
struct Shown {
    status: Signal<Option<HostStatus>>,
    admin: Signal<bool>,
    requests: Signal<Vec<PairRequest>>,
    paired: Signal<Vec<PairedClient>>,
    programs: Signal<Vec<StreamingProgram>>,
    hosts: Signal<Vec<StreamHost>>,
}

impl Shown {
    /// Reads everything again, as the webview invalidates its queries after
    /// an action.
    async fn refresh(mut self, streaming: Streaming) {
        let read = streaming.clone();
        let admin = change(async move { Ok(read.is_admin().await) })
            .await
            .unwrap_or(false);
        self.admin.set(admin);
        let status = streaming.backend().status();
        let on = on(Some(status.state));
        let ready = status.state == HostState::Ready;
        self.status.set(Some(status));
        self.hosts.set(streaming.backend().hosts());
        let read = streaming.clone();
        if let Ok(programs) = change(async move { read.backend().programs().await }).await {
            self.programs.set(programs);
        }
        if admin && on {
            let read = streaming.clone();
            if let Ok(paired) = change(async move { read.backend().paired_devices().await }).await {
                self.paired.set(paired);
            }
        } else {
            self.paired.set(Vec::new());
        }
        if admin && ready {
            let read = streaming.clone();
            if let Ok(requests) = change(async move { read.backend().requests().await }).await {
                self.requests.set(requests);
            }
        } else {
            self.requests.set(Vec::new());
        }
    }
}

/// Hosting is on in some form: not off, and this system can host.
fn on(state: Option<HostState>) -> bool {
    matches!(state, Some(state) if state != HostState::Off && state != HostState::Unsupported)
}

/// Settings → Streaming.
#[component]
pub fn StreamingSettings() -> Element {
    let Some(streaming) = use_streaming() else {
        return rsx! {};
    };
    rsx! {
        StreamingPanel { streaming }
    }
}

#[component]
fn StreamingPanel(streaming: Streaming) -> Element {
    let shown = Shown {
        status: use_signal(|| None),
        admin: use_signal(|| false),
        requests: use_signal(Vec::new),
        paired: use_signal(Vec::new),
        programs: use_signal(Vec::new),
        hosts: use_signal(Vec::new),
    };
    use_context_provider(|| shown);
    {
        let streaming = streaming.clone();
        use_hook(move || {
            spawn(async move {
                shown.refresh(streaming.clone()).await;
                let mut tick: u32 = 0;
                loop {
                    sleep(TICK).await;
                    tick = tick.wrapping_add(1);
                    let state = shown.status.peek().as_ref().map(|status| status.state);
                    // Setting up or starting is read every second.
                    let busy = matches!(state, Some(HostState::SettingUp | HostState::Starting));
                    if busy || tick.is_multiple_of(STATUS_EVERY) {
                        let before = state;
                        let status = streaming.backend().status();
                        let changed = Some(status.state) != before;
                        let mut current = shown.status;
                        current.set(Some(status));
                        if changed {
                            shown.refresh(streaming.clone()).await;
                            continue;
                        }
                    }
                    if tick.is_multiple_of(REQUESTS_EVERY)
                        && *shown.admin.peek()
                        && state == Some(HostState::Ready)
                    {
                        let read = streaming.clone();
                        if let Ok(list) =
                            change(async move { read.backend().requests().await }).await
                        {
                            let mut requests = shown.requests;
                            if *requests.peek() != list {
                                requests.set(list);
                            }
                        }
                    }
                    if tick.is_multiple_of(HOSTS_EVERY) {
                        let list = streaming.backend().hosts();
                        let mut hosts = shown.hosts;
                        if *hosts.peek() != list {
                            hosts.set(list);
                        }
                    }
                }
            });
        });
    }
    let admin = (shown.admin)();
    rsx! {
        div { "data-cs": "streaming-settings", class: "flex flex-col gap-6",
            if !admin {
                p { role: "note", class: "text-sm text-fg-muted",
                    {t!("Only an admin can let other devices stream from this one. Anyone can pair this device with another to play its games, and set the stream quality here.")}
                }
            }
            HostingSection { streaming: streaming.clone() }
            StreamFromSection { streaming: streaming.clone() }
            QualitySection { streaming: streaming.clone() }
            UpdatesSection { streaming }
        }
    }
}

fn use_shown() -> Shown {
    use_context::<Shown>()
}

/// Runs `work`, then reads everything again; `done` hears how it went.
fn act<T: Send + 'static>(
    streaming: &Streaming,
    mut pending: Signal<bool>,
    mut failure: Signal<Option<ErrorReport>>,
    work: impl FnOnce(
        Streaming,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<T, ErrorReport>> + Send>>
    + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let shown = use_shown_peek();
    let streaming = streaming.clone();
    pending.set(true);
    failure.set(None);
    spawn(async move {
        let job = work(streaming.clone());
        let result = change(job).await;
        pending.set(false);
        match result {
            Ok(value) => done(value),
            Err(report) => failure.set(Some(report)),
        }
        if let Some(shown) = shown {
            shown.refresh(streaming).await;
        }
    });
}

/// The panel's state, outside a render (an action's handler).
fn use_shown_peek() -> Option<Shown> {
    try_consume_context::<Shown>()
}

#[component]
fn HostingSection(streaming: Streaming) -> Element {
    let shown = use_shown();
    let admin = (shown.admin)();
    let status = (shown.status)();
    let state = status.as_ref().map(|status| status.state);
    let mut outcome = use_signal(|| None::<HostOutcome>);
    let switching = use_signal(|| false);
    let mut wanted = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let set = {
        let streaming = streaming.clone();
        move |next: bool| {
            outcome.set(None);
            wanted.set(next);
            act(
                &streaming,
                switching,
                failure,
                move |streaming| {
                    Box::pin(async move { streaming.backend().set_hosting(next).await })
                },
                move |result| outcome.set(Some(result)),
            );
        }
    };
    let fix = {
        let streaming = streaming.clone();
        move |_| {
            outcome.set(None);
            act(
                &streaming,
                switching,
                failure,
                |streaming| Box::pin(async move { streaming.backend().repair().await }),
                move |result| outcome.set(Some(result)),
            );
        }
    };
    let busy = switching();
    rsx! {
        SettingsSection {
            icon: CAST,
            title: t!("Let other devices play"),
            description: t!("Your other devices at home can play this device's games, streamed over the network, with their own controller."),
            SettingRow {
                label: t!("Let other devices stream games from this one"),
                description: t!("Setting up asks once for an administrator's permission."),
                Switch {
                    id: "streaming-hosting",
                    checked: if busy { wanted() } else { on(state) },
                    disabled: !admin || busy || state.is_none() || state == Some(HostState::Unsupported),
                    on_change: set,
                }
            }
            HostStateLine {
                state,
                problem: status.as_ref().and_then(|status| status.problem.clone()),
                unreachable: status.as_ref().is_some_and(|status| status.unreachable),
                switching: busy,
            }
            if let (Some(HostState::Ready), Some(warning)) = (state, status.as_ref().and_then(|status| status.warning.clone())) {
                p { "data-part": "host-warning", role: "note", class: "mt-1 text-sm",
                    StatusDot { tone: DotTone::Warning, "{warning}" }
                }
            }
            if admin && on(state) {
                div { class: "mt-2 flex flex-wrap items-center gap-3",
                    Button {
                        id: "streaming-repair",
                        variant: if state == Some(HostState::NeedsAttention) { ButtonVariant::Primary } else { ButtonVariant::Secondary },
                        icon: WRENCH,
                        disabled: busy,
                        on_press: fix,
                        if busy && !wanted() && on(state) { {t!("Repairing…")} } else { {t!("Repair")} }
                    }
                    span { class: "text-xs text-fg-muted",
                        {t!("Sets it up again where something went missing or changed.")}
                    }
                }
            }
            match outcome() {
                Some(HostOutcome::Cancelled) => rsx! {
                    p { role: "status", class: "mt-2 text-sm text-fg-muted",
                        {t!("Nothing changed: the administrator's permission wasn't given.")}
                    }
                },
                Some(HostOutcome::Refused) => rsx! {
                    p { role: "status", class: "mt-2 text-sm text-fg-muted",
                        {t!("Nothing changed: the system didn't allow it.")}
                    }
                },
                _ => rsx! {},
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
            if outcome() == Some(HostOutcome::ExistingFound) {
                ExistingDialog { streaming: streaming.clone(), on_close: move |_| outcome.set(None) }
            }
            if admin && state == Some(HostState::Ready) {
                WaitingRequests { streaming: streaming.clone() }
            }
            if !admin && state == Some(HostState::Ready) {
                p { role: "note", class: "mt-2 text-sm text-fg-muted",
                    {t!("When another device asks to play from this one, it shows a code. An admin signs in here to enter it.")}
                }
            }
            if admin && on(state) {
                PairedDevices { streaming }
            }
        }
    }
}

/// What hosting is doing, in a line.
#[component]
fn HostStateLine(
    state: Option<HostState>,
    problem: Option<String>,
    unreachable: bool,
    switching: bool,
) -> Element {
    let shown = match state {
        None | Some(HostState::Off) if switching => Some(HostState::SettingUp),
        Some(HostState::Ready) if unreachable => Some(HostState::NeedsAttention),
        other => other,
    };
    let status = match shown {
        Some(HostState::Ready) => "ready",
        Some(HostState::SettingUp) => "settingUp",
        Some(HostState::Starting) => "starting",
        Some(HostState::NeedsAttention) => "needsAttention",
        Some(HostState::Unsupported) => "unsupported",
        Some(HostState::Off) => "off",
        None => "",
    };
    let (tone, text) = match shown {
        Some(HostState::Ready) => (
            DotTone::Ok,
            t!("Ready. Devices on this network can pair and play."),
        ),
        Some(HostState::SettingUp) => (
            DotTone::Warning,
            t!("Setting up… The first time takes a few minutes."),
        ),
        Some(HostState::Starting) => (DotTone::Warning, t!("Starting…")),
        Some(HostState::NeedsAttention) => (
            DotTone::Danger,
            problem.unwrap_or_else(|| t!("Needs attention.")),
        ),
        Some(HostState::Unsupported) => {
            (DotTone::Off, t!("This device can't share its games yet."))
        }
        Some(HostState::Off) | None => (DotTone::Off, t!("Off.")),
    };
    rsx! {
        p { "data-part": "host-state", "data-status": status, role: "status", class: "mt-1 text-sm",
            StatusDot { tone, "{text}" }
        }
    }
}

/// Someone already set up game streaming here: use it with its sign-in.
#[component]
fn ExistingDialog(streaming: Streaming, on_close: EventHandler<()>) -> Element {
    let mut user = use_signal(String::new);
    let mut password = use_signal(String::new);
    let pending = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let submit = {
        let streaming = streaming.clone();
        use_callback(move |()| {
            let (name, secret) = (user(), password());
            if name.is_empty() || secret.is_empty() || pending() {
                return;
            }
            act(
                &streaming,
                pending,
                failure,
                move |streaming| {
                    Box::pin(async move { streaming.backend().use_existing(&name, &secret).await })
                },
                move |_| on_close.call(()),
            );
        })
    };
    rsx! {
        Dialog {
            title: t!("Streaming is already set up"),
            on_close: move |_| {
                if !pending() {
                    on_close.call(());
                }
            },
            "data-cs": "streaming-existing",
            "data-focus-scope": "streaming-existing",
            p { class: "text-sm text-fg-muted",
                {t!("Game streaming was set up on this device before. Caskade can use it, adding only its own games. Enter the user name and password it was set up with.")}
            }
            div { class: "mt-4 flex flex-col gap-3",
                TextField {
                    id: "streaming-existing-user",
                    label: t!("User name"),
                    value: user(),
                    on_input: move |text| user.set(text),
                    on_submit: move |_| submit.call(()),
                    "data-focus-default": "true",
                }
                TextField {
                    id: "streaming-existing-password",
                    label: t!("Password"),
                    kind: "password",
                    value: password(),
                    on_input: move |text| password.set(text),
                    on_submit: move |_| submit.call(()),
                }
                if let Some(report) = failure() {
                    FailureNotice { error: report, variant: NoticeVariant::Inline }
                }
                div { class: "mt-2 flex justify-end gap-2",
                    Button { disabled: pending(), on_press: move |_| on_close.call(()), {t!("Leave it off")} }
                    Button {
                        id: "streaming-existing-use",
                        variant: ButtonVariant::Primary,
                        disabled: user().is_empty() || password().is_empty() || pending(),
                        on_press: move |_| submit.call(()),
                        if pending() { {t!("Checking…")} } else { {t!("Use it")} }
                    }
                }
            }
        }
    }
}

/// The devices waiting to pair, each with the code to enter.
#[component]
fn WaitingRequests(streaming: Streaming) -> Element {
    let shown = use_shown();
    let requests = (shown.requests)();
    if requests.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { "data-part": "requests", class: "mt-4 flex flex-col gap-3",
            h3 { class: "text-sm font-semibold", {t!("Waiting to pair")} }
            ul { class: "flex flex-col gap-3",
                for request in requests {
                    li { key: "{request.id}", class: "rounded-control border border-accent-alt/50 p-3",
                        PairRequestForm {
                            pairing: streaming.pairing(),
                            request,
                            on_done: {
                                let streaming = streaming.clone();
                                move |_| {
                                    let streaming = streaming.clone();
                                    spawn(async move { shown.refresh(streaming).await });
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

/// The devices that can play from this one, each forgotten after asking
/// in its row, as Forget all does (#49).
#[component]
fn PairedDevices(streaming: Streaming) -> Element {
    let shown = use_shown();
    let paired = (shown.paired)();
    let removing = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let mut confirming = use_signal(|| false);
    let mut forgetting = use_signal(|| None::<String>);
    let count = paired.len();
    rsx! {
        div { "data-part": "paired", class: "mt-4 flex flex-col gap-2",
            h3 { class: "text-sm font-semibold", {t!("Devices that can play from this one")} }
            if paired.is_empty() {
                p { class: "text-sm text-fg-muted",
                    {t!("None yet. On another device, open Settings → Streaming in Caskade and choose Pair with this one.")}
                }
            } else {
                ul { class: "flex flex-col gap-2",
                    for device in paired {
                        li {
                            key: "{device.uuid}",
                            "data-cs": "streaming-client",
                            class: "flex flex-wrap items-center justify-between gap-3 rounded-control border border-border p-3",
                            span { class: "font-medium", "{device.name}" }
                            if forgetting().as_deref() == Some(device.uuid.as_str()) {
                                span { class: "flex flex-wrap items-center gap-2",
                                    Button {
                                        size: ButtonSize::Sm,
                                        variant: ButtonVariant::Danger,
                                        disabled: removing(),
                                        on_press: {
                                            let (streaming, uuid) = (streaming.clone(), device.uuid.clone());
                                            move |_| {
                                                let uuid = uuid.clone();
                                                act(
                                                    &streaming,
                                                    removing,
                                                    failure,
                                                    move |streaming| Box::pin(async move { streaming.backend().unpair(&uuid).await }),
                                                    move |_| forgetting.set(None),
                                                );
                                            }
                                        },
                                        {t!("Forget {name}", name = device.name.clone())}
                                    }
                                    Button {
                                        size: ButtonSize::Sm,
                                        variant: ButtonVariant::Ghost,
                                        "data-focus-default": "true",
                                        on_press: move |_| forgetting.set(None),
                                        {t!("Keep")}
                                    }
                                }
                                p { class: "w-full text-xs text-fg-muted",
                                    {t!("It needs a new code to play from this device again.")}
                                }
                            } else {
                                Button {
                                    size: ButtonSize::Sm,
                                    variant: ButtonVariant::Ghost,
                                    icon: TRASH,
                                    "aria-label": t!("Forget {name}", name = device.name.clone()),
                                    disabled: removing(),
                                    on_press: {
                                        let uuid = device.uuid.clone();
                                        move |_| forgetting.set(Some(uuid.clone()))
                                    },
                                    {t!("Forget…")}
                                }
                            }
                        }
                    }
                }
            }
            if count > 1 {
                if confirming() {
                    div { class: "flex flex-wrap items-center gap-2",
                        span { class: "text-sm",
                            {t!("Every device then needs a new code to play from this one.")}
                        }
                        Button {
                            id: "streaming-forget-all",
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Danger,
                            disabled: removing(),
                            on_press: {
                                let streaming = streaming.clone();
                                move |_| {
                                    act(
                                        &streaming,
                                        removing,
                                        failure,
                                        |streaming| Box::pin(async move { streaming.backend().unpair_all().await }),
                                        move |_| confirming.set(false),
                                    );
                                }
                            },
                            {t!("Forget them all")}
                        }
                        Button {
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Ghost,
                            on_press: move |_| confirming.set(false),
                            {t!("Keep them")}
                        }
                    }
                } else {
                    Button {
                        id: "streaming-forget-all-ask",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        class: "self-start",
                        icon: TRASH,
                        on_press: move |_| confirming.set(true),
                        {t!("Forget all paired devices…")}
                    }
                }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
        }
    }
}

/// Newer versions of the streaming programs (#266): Update or Skip. An
/// update never interrupts a stream.
#[component]
fn UpdatesSection(streaming: Streaming) -> Element {
    let shown = use_shown();
    let admin = (shown.admin)();
    let programs = (shown.programs)();
    let pending = use_signal(|| false);
    let mut updating = use_signal(|| None::<StreamingPart>);
    let failure = use_signal(|| None::<ErrorReport>);
    if programs.is_empty() {
        return rsx! {};
    }
    rsx! {
        SettingsSection {
            icon: DOWNLOAD,
            title: t!("Streaming updates"),
            description: t!("Caskade keeps streaming up to date when you say so, never in the middle of a game."),
            ul { class: "flex flex-col gap-2",
                for program in programs {
                    li {
                        key: "{program.part:?}",
                        "data-cs": "streaming-program",
                        "data-status": if program.update { "update" } else { "current" },
                        class: "flex flex-wrap items-center gap-3 rounded-control border border-border p-3",
                        div { class: "flex min-w-0 flex-1 flex-col gap-1",
                            span { class: "font-medium", "{program.label}" }
                            span { "data-part": "version", class: "text-xs text-fg-muted",
                                if program.update {
                                    {t!("A newer version, {latest}, is ready.", latest = program.latest.clone().unwrap_or_default())}
                                } else {
                                    {t!("Up to date.")}
                                }
                            }
                        }
                        if program.update && admin {
                            div { class: "flex gap-2",
                                Button {
                                    size: ButtonSize::Sm,
                                    variant: ButtonVariant::Primary,
                                    disabled: pending(),
                                    on_press: {
                                        let (streaming, part) = (streaming.clone(), program.part);
                                        move |_| {
                                            updating.set(Some(part));
                                            act(
                                                &streaming,
                                                pending,
                                                failure,
                                                move |streaming| Box::pin(async move { streaming.backend().update(part).await }),
                                                move |_| updating.set(None),
                                            );
                                        }
                                    },
                                    if pending() && updating() == Some(program.part) { {t!("Updating…")} } else { {t!("Update")} }
                                }
                                Button {
                                    size: ButtonSize::Sm,
                                    variant: ButtonVariant::Ghost,
                                    disabled: pending(),
                                    on_press: {
                                        let (streaming, part) = (streaming.clone(), program.part);
                                        move |_| {
                                            act(
                                                &streaming,
                                                pending,
                                                failure,
                                                move |streaming| Box::pin(async move { streaming.backend().skip(part).await }),
                                                |_| {},
                                            );
                                        }
                                    },
                                    {t!("Skip this version")}
                                }
                            }
                        }
                    }
                }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
        }
    }
}

/// A device's state, for people: "Ready", "Asleep", …
pub fn reach_label(reach: HostReach) -> String {
    match reach {
        HostReach::Ready => tp!("streaming device", "Ready"),
        HostReach::Busy => tp!("streaming device", "Playing another game"),
        HostReach::Asleep => tp!("streaming device", "Asleep: it wakes when you stream"),
        HostReach::Unreachable => tp!("streaming device", "Can't be reached"),
    }
}

fn reach_name(reach: HostReach) -> &'static str {
    match reach {
        HostReach::Ready => "ready",
        HostReach::Busy => "busy",
        HostReach::Asleep => "asleep",
        HostReach::Unreachable => "unreachable",
    }
}

#[component]
fn StreamFromSection(streaming: Streaming) -> Element {
    let shown = use_shown();
    let hosts = (shown.hosts)();
    let mut pairing = use_signal(|| None::<StreamHost>);
    let mut address = use_signal(String::new);
    let looking = use_signal(|| false);
    let adding = use_signal(|| false);
    let look_failure = use_signal(|| None::<ErrorReport>);
    let add_failure = use_signal(|| None::<ErrorReport>);
    let add = {
        let streaming = streaming.clone();
        use_callback(move |()| {
            let value = address().trim().to_owned();
            if value.is_empty() || adding() {
                return;
            }
            act(
                &streaming,
                adding,
                add_failure,
                move |streaming| {
                    Box::pin(async move { streaming.backend().add_host(&value).await })
                },
                move |_| address.set(String::new()),
            );
        })
    };
    let look = {
        let streaming = streaming.clone();
        move |_| {
            act(
                &streaming,
                looking,
                look_failure,
                |streaming| Box::pin(async move { streaming.backend().look_again().await }),
                |_| {},
            );
        }
    };
    rsx! {
        SettingsSection {
            icon: MONITOR_PLAY,
            title: t!("Play from other devices"),
            description: t!("Pair once with a device that lets others play, and its games show in your library as Stream from that device."),
            aside: rsx! {
                Button {
                    id: "streaming-look-again",
                    size: ButtonSize::Sm,
                    variant: ButtonVariant::Ghost,
                    icon: REFRESH_CW,
                    disabled: looking(),
                    on_press: look,
                    {t!("Look again")}
                }
            },
            if hosts.is_empty() {
                p { class: "text-sm text-fg-muted",
                    {t!("No devices found on this network yet. On another device running Caskade, turn on Let other devices stream games from this one, or add it by its address below.")}
                }
            } else {
                ul { class: "flex flex-col gap-2",
                    for host in hosts {
                        HostRow {
                            key: "{host.id}",
                            streaming: streaming.clone(),
                            host: host.clone(),
                            on_pair: move |_| pairing.set(Some(host.clone())),
                        }
                    }
                }
            }
            if let Some(report) = look_failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
            div { class: "mt-4 flex flex-wrap items-end gap-3",
                TextField {
                    id: "streaming-address",
                    label: t!("Add a device by its address"),
                    description: t!("For a device on another network, or one that isn't found by itself."),
                    placeholder: ADDRESS_EXAMPLE.to_owned(),
                    value: address(),
                    on_input: move |text| address.set(text),
                    on_submit: move |_| add.call(()),
                    mono: true,
                    class: "min-w-64 flex-1",
                }
                Button {
                    id: "streaming-add",
                    icon: PLUS,
                    disabled: address().trim().is_empty() || adding(),
                    on_press: move |_| add.call(()),
                    {t!("Add")}
                }
            }
            if let Some(report) = add_failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
            if let Some(host) = pairing() {
                PairDialog {
                    key: "{host.id}",
                    streaming,
                    host,
                    on_close: move |_| pairing.set(None),
                }
            }
        }
    }
}

/// An address's example, not translated.
const ADDRESS_EXAMPLE: &str = "192.168.1.20";

#[component]
fn HostRow(streaming: Streaming, host: StreamHost, on_pair: EventHandler<()>) -> Element {
    let mut confirming = use_signal(|| false);
    let forgetting = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let name = host.name.clone();
    let state = if host.paired {
        t!(
            "{0} · Paired · {games} games",
            0 = reach_label(host.reach),
            games = host.games
        )
    } else {
        t!("{0} · Not paired", 0 = reach_label(host.reach))
    };
    rsx! {
        li {
            "data-cs": "stream-host",
            "data-status": reach_name(host.reach),
            class: "flex flex-wrap items-center gap-4 rounded-control border border-border p-3",
            div { class: "flex min-w-0 flex-1 flex-col gap-1",
                span { class: "font-medium", "{name}" }
                span { "data-part": "state", class: "text-xs text-fg-muted", "{state}" }
            }
            if host.paired {
                if confirming() {
                    div { class: "flex gap-2",
                        Button {
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Danger,
                            disabled: forgetting(),
                            on_press: {
                                let (streaming, id) = (streaming.clone(), host.id.clone());
                                move |_| {
                                    let id = id.clone();
                                    act(
                                        &streaming,
                                        forgetting,
                                        failure,
                                        move |streaming| Box::pin(async move { streaming.backend().forget_host(&id).await }),
                                        move |_| confirming.set(false),
                                    );
                                }
                            },
                            {t!("Forget {name}", name = name.clone())}
                        }
                        Button {
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Ghost,
                            "data-focus-default": "true",
                            on_press: move |_| confirming.set(false),
                            {t!("Keep")}
                        }
                    }
                } else {
                    Button {
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        icon: TRASH,
                        "aria-label": t!("Forget {name}", name = name.clone()),
                        on_press: move |_| confirming.set(true),
                        {t!("Forget…")}
                    }
                }
            } else {
                Button {
                    size: ButtonSize::Sm,
                    variant: ButtonVariant::Primary,
                    disabled: host.reach == HostReach::Unreachable,
                    on_press: move |_| on_pair.call(()),
                    {t!("Pair…")}
                }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
        }
    }
}

/// Where pairing with a device to stream from is.
#[derive(Clone, PartialEq)]
enum Step {
    Asking,
    Code(PairingStarted),
    Finished {
        outcome: PairingOutcome,
        message: Option<String>,
    },
    Failed(String),
}

/// Pairing this device with one to stream from (#265): its code, to enter
/// in Caskade on the other device, until that device answers. A wrong code
/// or a request that expired can be tried again.
#[component]
fn PairDialog(streaming: Streaming, host: StreamHost, on_close: EventHandler<()>) -> Element {
    let shown = use_shown();
    let mut step = use_signal(|| Step::Asking);
    let mut attempt = use_signal(|| 0u32);
    let device = host.name.clone();
    // Each attempt asks again; its answer and how it ends come back here.
    let _asking = {
        let (streaming, host_id) = (streaming.clone(), host.id.clone());
        use_resource(move || {
            let (streaming, host_id) = (streaming.clone(), host_id.clone());
            let _ = attempt();
            async move {
                step.set(Step::Asking);
                let mut events = streaming.backend().events();
                let ask = {
                    let (streaming, host_id) = (streaming.clone(), host_id.clone());
                    change(async move { streaming.backend().pair_with(&host_id).await })
                };
                match ask.await {
                    Ok(started) => {
                        if *step.peek() == Step::Asking {
                            step.set(Step::Code(started));
                        }
                    }
                    Err(report) => {
                        step.set(Step::Failed(report.summary));
                        return;
                    }
                }
                use caskade_app::events::Event;
                use tokio::sync::broadcast::error::RecvError;
                loop {
                    match events.recv().await {
                        Ok(Event::StreamingPairingFinished(finished))
                            if finished.host_id == host_id =>
                        {
                            step.set(Step::Finished {
                                outcome: finished.outcome,
                                message: finished.message,
                            });
                            shown.refresh(streaming.clone()).await;
                            break;
                        }
                        Ok(_) | Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => break,
                    }
                }
            }
        })
    };
    let current = step();
    let outcome = match &current {
        Step::Finished { outcome, .. } => Some(*outcome),
        _ => None,
    };
    let status = match (&current, outcome) {
        (_, Some(PairingOutcome::Paired)) => "paired",
        (_, Some(PairingOutcome::WrongCode)) => "wrongCode",
        (_, Some(PairingOutcome::Declined)) => "declined",
        (_, Some(PairingOutcome::Expired)) => "expired",
        (_, Some(PairingOutcome::Failed)) => "failed",
        (Step::Asking, _) => "asking",
        (Step::Code(_), _) => "code",
        _ => "failed",
    };
    let retry = matches!(current, Step::Failed(_))
        || matches!(
            outcome,
            Some(PairingOutcome::WrongCode | PairingOutcome::Expired | PairingOutcome::Failed)
        );
    rsx! {
        Dialog {
            title: t!("Pair with {device}", device = device.clone()),
            on_close: move |_| on_close.call(()),
            "data-cs": "pair-dialog",
            "data-status": status,
            "data-focus-scope": "pair-dialog",
            match current.clone() {
                Step::Asking => rsx! {
                    p { role: "status", class: "mt-3 text-sm text-fg-muted",
                        {t!("Asking {device}…", device = device.clone())}
                    }
                },
                Step::Code(started) if started.automatic => rsx! {
                    p { role: "status", class: "mt-3 text-sm text-fg-muted",
                        {t!("{device} is one of your devices, so it's pairing without a code. This takes a moment.", device = device.clone())}
                    }
                },
                Step::Code(started) => rsx! {
                    p { class: "mt-3 text-sm text-fg-muted",
                        {t!("On {device}, someone signed in to Caskade as an admin enters this code in Settings → Streaming:", device = device.clone())}
                    }
                    p { "data-part": "code", class: "mt-4 text-center font-mono text-5xl font-semibold tracking-[0.4em] text-accent-alt",
                        "{started.code}"
                    }
                    p { role: "status", class: "mt-4 text-sm text-fg-muted",
                        {t!("Waiting for {device}…", device = device.clone())}
                    }
                },
                Step::Finished { outcome, message } => {
                    let paired = outcome == PairingOutcome::Paired;
                    let text = match outcome {
                        PairingOutcome::Paired => t!("Paired. {device}'s games show in your library as Stream from {device}.", device = device.clone()),
                        PairingOutcome::WrongCode => t!("The code entered on {device} didn't match. Try again with a new code.", device = device.clone()),
                        PairingOutcome::Declined => t!("{device} declined.", device = device.clone()),
                        PairingOutcome::Expired => t!("Nobody entered the code on {device} in time. It takes an admin signed in there.", device = device.clone()),
                        PairingOutcome::Failed => message.unwrap_or_else(|| t!("Pairing with {device} didn't work.", device = device.clone())),
                    };
                    rsx! {
                        p {
                            role: if paired { "status" } else { "alert" },
                            class: if paired { "mt-3 text-sm" } else { "mt-3 text-sm text-danger" },
                            "{text}"
                        }
                    }
                }
                Step::Failed(message) => rsx! {
                    p { role: "alert", class: "mt-3 text-sm text-danger", "{message}" }
                },
            }
            div { "data-part": "actions", class: "mt-6 flex justify-end gap-2",
                if outcome == Some(PairingOutcome::Paired) {
                    Button {
                        id: "pair-dialog-done",
                        variant: ButtonVariant::Primary,
                        "data-focus-default": "true",
                        on_press: move |_| on_close.call(()),
                        {t!("Done")}
                    }
                } else {
                    if retry {
                        Button {
                            id: "pair-dialog-again",
                            variant: ButtonVariant::Primary,
                            "data-focus-default": "true",
                            on_press: move |_| attempt += 1,
                            {t!("Try again")}
                        }
                    }
                    Button {
                        id: "pair-dialog-close",
                        "data-focus-default": (!retry).then_some("true"),
                        on_press: move |_| on_close.call(()),
                        if outcome.is_some() { {t!("Close")} } else { {t!("Cancel")} }
                    }
                }
            }
        }
    }
}

const AUTOMATIC: &str = "auto";
const SECTION: &str = "core.streaming";

/// The quality saved in `core.streaming`.
fn saved_quality(value: Option<&Value>) -> StreamQuality {
    value
        .and_then(|value| value.get("quality"))
        .and_then(|quality| serde_json::from_value(quality.clone()).ok())
        .unwrap_or_default()
}

/// The stream quality: any account's to change, through its own command
/// (the section's other settings are an admin's, #66).
#[component]
fn QualitySection(streaming: Streaming) -> Element {
    let section = use_settings_section(SECTION);
    let saving = use_signal(|| false);
    let failure = use_signal(|| None::<ErrorReport>);
    let value = section.value();
    let quality = saved_quality(value.as_ref());
    let disabled = value.is_none() || saving();
    let change_to = {
        let streaming = streaming.clone();
        move |next: StreamQuality| {
            act(
                &streaming,
                saving,
                failure,
                move |streaming| {
                    Box::pin(async move { streaming.backend().set_quality(next).await })
                },
                move |_| section.reload(),
            );
        }
    };
    let automatic = tp!("stream quality", "Automatic");
    let resolutions: Vec<SelectOption> =
        std::iter::once(SelectOption::new(AUTOMATIC, automatic.clone()))
            .chain(
                [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)]
                    .into_iter()
                    .map(|(w, h)| SelectOption::new(format!("{w}x{h}"), format!("{w} × {h}"))),
            )
            .collect();
    let rates: Vec<SelectOption> =
        std::iter::once(SelectOption::new(AUTOMATIC, automatic.clone()))
            .chain([30, 60, 90, 120].into_iter().map(|fps| {
                SelectOption::new(fps.to_string(), t!("{fps} frames a second", fps = fps))
            }))
            .collect();
    let bitrates: Vec<SelectOption> = std::iter::once(SelectOption::new(AUTOMATIC, automatic))
        .chain([5, 10, 20, 40, 80].into_iter().map(|mbps| {
            SelectOption::new(
                (mbps * 1000).to_string(),
                t!(
                    note = "Network speed of a game stream",
                    "{mbps} Mbit/s",
                    mbps = mbps
                ),
            )
        }))
        .collect();
    let chosen = |value: Option<String>| Some(value.unwrap_or_else(|| AUTOMATIC.to_owned()));
    let number = |id: &str| (id != AUTOMATIC).then(|| id.parse().ok()).flatten();
    rsx! {
        SettingsSection {
            icon: GAUGE,
            title: t!("Stream quality"),
            description: t!("How games streamed to this device look. Automatic suits this screen and the network."),
            SettingRow { label: t!("Resolution"),
                Select {
                    id: "streaming-resolution",
                    options: resolutions,
                    value: chosen(quality.resolution.clone()),
                    disabled,
                    on_change: {
                        let (change_to, quality) = (change_to.clone(), quality.clone());
                        move |id: String| change_to(StreamQuality {
                            resolution: (id != AUTOMATIC).then_some(id),
                            ..quality.clone()
                        })
                    },
                }
            }
            SettingRow { label: t!("Frame rate"),
                Select {
                    id: "streaming-fps",
                    options: rates,
                    value: chosen(quality.fps.map(|fps| fps.to_string())),
                    disabled,
                    on_change: {
                        let (change_to, quality) = (change_to.clone(), quality.clone());
                        move |id: String| change_to(StreamQuality { fps: number(&id), ..quality.clone() })
                    },
                }
            }
            SettingRow {
                label: t!("Bitrate"),
                description: t!("Higher looks sharper and needs a faster network."),
                Select {
                    id: "streaming-bitrate",
                    options: bitrates,
                    value: chosen(quality.bitrate_kbps.map(|kbps| kbps.to_string())),
                    disabled,
                    on_change: {
                        let (change_to, quality) = (change_to.clone(), quality.clone());
                        move |id: String| change_to(StreamQuality { bitrate_kbps: number(&id), ..quality.clone() })
                    },
                }
            }
            SettingsStatus { section, what: t!("Stream quality") }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
        }
    }
}

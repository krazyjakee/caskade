//! Hosting: whether this device streams its games, its state, and the
//! devices paired to it or waiting.

use super::*;

#[component]
pub(super) fn HostingSection(streaming: Streaming) -> Element {
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
pub(super) fn HostStateLine(
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
pub(super) fn ExistingDialog(streaming: Streaming, on_close: EventHandler<()>) -> Element {
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
pub(super) fn WaitingRequests(streaming: Streaming) -> Element {
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
pub(super) fn PairedDevices(streaming: Streaming) -> Element {
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

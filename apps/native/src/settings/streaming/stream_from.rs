//! Streaming from another device: the hosts in reach and pairing with one.

use super::*;

/// A device's state, for people: "Ready", "Asleep", …
pub fn reach_label(reach: HostReach) -> String {
    match reach {
        HostReach::Ready => tp!("streaming device", "Ready"),
        HostReach::Busy => tp!("streaming device", "Playing another game"),
        HostReach::Asleep => tp!("streaming device", "Asleep: it wakes when you stream"),
        HostReach::Unreachable => tp!("streaming device", "Can't be reached"),
    }
}

pub(super) fn reach_name(reach: HostReach) -> &'static str {
    match reach {
        HostReach::Ready => "ready",
        HostReach::Busy => "busy",
        HostReach::Asleep => "asleep",
        HostReach::Unreachable => "unreachable",
    }
}

#[component]
pub(super) fn StreamFromSection(streaming: Streaming) -> Element {
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
pub(super) const ADDRESS_EXAMPLE: &str = "192.168.1.20";

#[component]
pub(super) fn HostRow(
    streaming: Streaming,
    host: StreamHost,
    on_pair: EventHandler<()>,
) -> Element {
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
pub(super) enum Step {
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
pub(super) fn PairDialog(
    streaming: Streaming,
    host: StreamHost,
    on_close: EventHandler<()>,
) -> Element {
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

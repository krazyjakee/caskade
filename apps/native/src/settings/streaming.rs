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

use crate::data::sleep;
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

mod hosting;
mod stream_from;
use hosting::HostingSection;
use stream_from::StreamFromSection;

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
                    // Nothing to show while the window is hidden (#111).
                    if !crate::shell::main_window_shown() {
                        continue;
                    }
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
    pending: Signal<bool>,
    failure: Signal<Option<ErrorReport>>,
    work: impl FnOnce(
        Streaming,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<T, ErrorReport>> + Send>>
    + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let shown = use_shown_peek();
    let streaming = streaming.clone();
    crate::data::run(pending, failure, work(streaming.clone()), move |value| {
        if let Some(value) = value {
            done(value);
        }
        if let Some(shown) = shown {
            spawn(shown.refresh(streaming));
        }
    });
}

/// The panel's state, outside a render (an action's handler).
fn use_shown_peek() -> Option<Shown> {
    try_consume_context::<Shown>()
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

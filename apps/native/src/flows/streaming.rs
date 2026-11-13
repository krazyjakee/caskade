//! Streaming's dialogs (#264, #265), as the webview's
//! `components/streaming`:
//! - [`StreamPairingHost`]: a device asking to pair with this one to play
//!   its games, a prompt over any view while this device lets others play,
//!   for an admin to enter the code that device shows. Settings →
//!   Streaming lists the same requests, so the prompt stays away while it
//!   is open; a request dismissed here waits there until it expires.
//! - [`PairRequestForm`]: the code, the name to know the device by, then
//!   Pair or Decline; in the prompt and in Settings → Streaming (#105).
//! - [`StopHostDialog`]: the device to stream from is playing another game;
//!   stop it and stream this one, or leave it be. The launch flow (#100)
//!   opens it on a busy host.

use crate::data::sleep;
use std::time::Duration;

use caskade_core::error::ErrorReport;
use caskade_ui_data::streaming::{PairRequest, StreamPairing};
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::data::change;
use crate::router::{Route, Router};
use crate::ui::icons::{CHECK, X};
use crate::ui::{Button, ButtonVariant, Dialog, FailureNotice, NoticeVariant, TextField};

/// How often the prompt asks the host for requests, as the webview's
/// `usePairRequests`; and whether it hosts, as `useHostStatus`.
const REQUESTS_EVERY: Duration = Duration::from_secs(3);
const HOSTING_EVERY: Duration = Duration::from_secs(5);

/// The app's streaming host for the prompt: a test's, else the running
/// app's; `None` where this system has none.
pub fn use_stream_pairing() -> Option<StreamPairing> {
    try_use_context::<StreamPairing>()
        .or_else(|| crate::shell::services().and_then(StreamPairing::of))
}

/// The prompt for the first request not dismissed, over any view.
#[component]
pub fn StreamPairingHost(pairing: StreamPairing) -> Element {
    let mut requests = use_signal(Vec::<PairRequest>::new);
    let mut dismissed = use_signal(Vec::<String>::new);
    {
        let pairing = pairing.clone();
        use_hook(move || {
            spawn(async move {
                loop {
                    // Hosting first: it's in memory, and an app that isn't
                    // hosting reads no accounts (#111).
                    let admin = pairing.hosting() && {
                        let read = pairing.clone();
                        matches!(
                            change(async move { Ok(read.is_admin().await) }).await,
                            Ok(true)
                        )
                    };
                    if admin {
                        let read = pairing.clone();
                        // A failed read keeps what's shown until the next.
                        if let Ok(list) = change(async move { read.requests().await }).await {
                            requests.set(list);
                        }
                        sleep(REQUESTS_EVERY).await;
                    } else {
                        if !requests.peek().is_empty() {
                            requests.set(Vec::new());
                        }
                        sleep(HOSTING_EVERY).await;
                    }
                }
            });
        });
    }
    let in_settings = try_use_context::<Router>().is_some_and(|router| {
        matches!(
            router.route(),
            Route::Settings {
                tab: "streaming",
                ..
            }
        )
    });
    let request = requests
        .read()
        .iter()
        .find(|request| !dismissed.read().contains(&request.id))
        .cloned();
    let Some(request) = request.filter(|_| !in_settings) else {
        return rsx! {};
    };
    let id = request.id.clone();
    rsx! {
        PromptDialog {
            key: "{request.id}",
            pairing,
            request,
            on_dismiss: move |_| dismissed.write().push(id.clone()),
        }
    }
}

#[component]
fn PromptDialog(
    pairing: StreamPairing,
    request: PairRequest,
    on_dismiss: EventHandler<()>,
) -> Element {
    let device = request.name.clone();
    rsx! {
        Dialog {
            title: t!("{device} wants to play from this device", device = device.clone()),
            on_close: move |_| on_dismiss.call(()),
            "data-cs": "stream-pair-prompt",
            "data-focus-scope": "stream-pair-prompt",
            p { class: "text-sm text-fg-muted",
                {t!("Enter the code {device} shows to let it play this device's games.", device = device.clone())}
            }
            div { class: "mt-4",
                PairRequestForm {
                    pairing,
                    request,
                    on_done: move |_| on_dismiss.call(()),
                    autofocus: true,
                }
            }
            div { class: "mt-4 flex justify-end",
                Button {
                    id: "stream-pair-not-now",
                    variant: ButtonVariant::Ghost,
                    on_press: move |_| on_dismiss.call(()),
                    {t!("Not now")}
                }
            }
        }
    }
}

/// The code a waiting device shows (four digits), only digits kept.
fn code_typed(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).take(4).collect()
}

/// A device waiting to pair with this one: the code it shows, the name to
/// know it by, then Pair or Decline. `on_done` hears it paired or declined.
#[component]
pub fn PairRequestForm(
    pairing: StreamPairing,
    request: PairRequest,
    #[props(default)] on_done: Option<EventHandler<()>>,
    #[props(default)] autofocus: bool,
) -> Element {
    let mut code = use_signal(String::new);
    let mut name = use_signal(|| request.name.clone());
    let mut accepting = use_signal(|| false);
    let mut declining = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let device = request.name.clone();
    let ready = code().len() == 4;
    let busy = accepting() || declining();
    let done = move || {
        if let Some(on_done) = on_done {
            on_done.call(());
        }
    };
    let accept = {
        let pairing = pairing.clone();
        let request = request.clone();
        use_callback(move |()| {
            if code().len() != 4 || accepting() || declining() {
                return;
            }
            let pairing = pairing.clone();
            let id = request.id.clone();
            let typed = code();
            let called = match name().trim() {
                "" => request.name.clone(),
                called => called.to_owned(),
            };
            accepting.set(true);
            failure.set(None);
            spawn(async move {
                let result =
                    change(async move { pairing.accept(&id, &typed, &called).await }).await;
                accepting.set(false);
                match result {
                    Ok(()) => done(),
                    Err(report) => failure.set(Some(report)),
                }
            });
        })
    };
    let decline = {
        let pairing = pairing.clone();
        let id = request.id.clone();
        move |_| {
            let pairing = pairing.clone();
            let id = id.clone();
            declining.set(true);
            failure.set(None);
            spawn(async move {
                let result = change(async move { pairing.decline(&id).await }).await;
                declining.set(false);
                match result {
                    Ok(()) => done(),
                    Err(report) => failure.set(Some(report)),
                }
            });
        }
    };
    let submit = move |_: String| accept.call(());
    rsx! {
        div { "data-cs": "pair-request", class: "flex flex-col gap-3",
            p { "data-part": "ask", class: "text-sm",
                {t!("{device} wants to stream games from this device.", device = device.clone())}
            }
            div { class: "flex flex-wrap items-end gap-3",
                TextField {
                    id: "pair-request-code",
                    label: t!("Code shown on {device}", device = device.clone()),
                    value: code(),
                    on_input: move |text: String| code.set(code_typed(&text)),
                    on_submit: submit,
                    mono: true,
                    "data-focus-default": autofocus.then_some("true"),
                    class: "w-40",
                }
                TextField {
                    id: "pair-request-name",
                    label: t!("Call it"),
                    value: name(),
                    on_input: move |text| name.set(text),
                    on_submit: submit,
                    class: "min-w-48 flex-1",
                }
            }
            div { class: "flex flex-wrap gap-2",
                Button {
                    id: "pair-request-accept",
                    variant: ButtonVariant::Primary,
                    icon: CHECK,
                    disabled: !ready || busy,
                    on_press: move |_| accept.call(()),
                    if accepting() { {t!("Pairing…")} } else { {t!("Pair")} }
                }
                Button {
                    id: "pair-request-decline",
                    icon: X,
                    disabled: busy,
                    on_press: decline,
                    {t!("Decline")}
                }
            }
            if let Some(report) = failure() {
                FailureNotice { error: report, variant: NoticeVariant::Inline }
            }
        }
    }
}

/// The device to stream from and what it plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBusy {
    /// The other device's name.
    pub device: String,
    /// The game it plays, when known.
    pub playing: Option<String>,
}

/// The question, in the webview's words for each case.
fn stop_question(busy: &HostBusy, title: Option<&str>) -> String {
    let device = busy.device.clone();
    match (title, busy.playing.as_deref()) {
        (Some(title), Some(playing)) => t!(
            note = "{device} is another device's name; {playing} and {title} are games",
            "{device} is playing {playing}. Stop it and stream {title}?",
            device = device,
            playing = playing,
            title = title
        ),
        (Some(title), None) => t!(
            note = "{device} is another device's name; {title} is a game",
            "{device} is playing another game. Stop it and stream {title}?",
            device = device,
            title = title
        ),
        (None, Some(playing)) => t!(
            note = "{device} is another device's name; {playing} is a game",
            "{device} is playing {playing}. Stop it and stream this game?",
            device = device,
            playing = playing
        ),
        (None, None) => t!(
            note = "{device} is another device's name",
            "{device} is playing another game. Stop it and stream this game?",
            device = device
        ),
    }
}

/// Stop the other game and stream this one? A pad starts on Stop and
/// stream; nothing closes it while the stream starts.
#[component]
pub fn StopHostDialog(
    busy: HostBusy,
    /// The game to stream, when known.
    #[props(default)]
    title: Option<String>,
    launching: bool,
    #[props(default)] error: Option<String>,
    on_close: EventHandler<()>,
    on_confirm: EventHandler<()>,
) -> Element {
    let question = stop_question(&busy, title.as_deref());
    rsx! {
        Dialog {
            title: t!("Stop the other game?"),
            on_close: move |_| {
                if !launching {
                    on_close.call(());
                }
            },
            "data-cs": "stop-host",
            "data-focus-scope": "stop-host",
            p { "data-part": "question", class: "text-sm text-fg-muted", "{question}" }
            p { class: "mt-2 text-sm text-fg-muted", {t!("Progress it hasn't saved is lost.")} }
            if let Some(error) = error {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            div { "data-part": "actions", class: "mt-6 flex justify-end gap-2",
                Button {
                    id: "stop-host-cancel",
                    disabled: launching,
                    on_press: move |_| on_close.call(()),
                    {t!("Cancel")}
                }
                Button {
                    id: "stop-host-confirm",
                    variant: ButtonVariant::Primary,
                    "data-focus-default": "true",
                    disabled: launching,
                    on_press: move |_| on_confirm.call(()),
                    if launching { {t!("Starting…")} } else { {t!("Stop and stream")} }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HostBusy, stop_question};

    #[test]
    fn the_stop_question_names_what_it_knows() {
        let busy = |playing: Option<&str>| HostBusy {
            device: "Desk".into(),
            playing: playing.map(str::to_owned),
        };
        assert_eq!(
            stop_question(&busy(Some("Celeste")), Some("Hades")),
            "Desk is playing Celeste. Stop it and stream Hades?"
        );
        assert_eq!(
            stop_question(&busy(None), Some("Hades")),
            "Desk is playing another game. Stop it and stream Hades?"
        );
        assert_eq!(
            stop_question(&busy(Some("Celeste")), None),
            "Desk is playing Celeste. Stop it and stream this game?"
        );
        assert_eq!(
            stop_question(&busy(None), None),
            "Desk is playing another game. Stop it and stream this game?"
        );
    }
}

//! Games another device asks to start here (#234), as the webview's
//! `RemotePlayHost`: each is played as Play would ([`PlayRequest`]), with
//! this device's own account, so a launch's questions ask here. How it
//! went goes back to the device that asked.

use std::sync::Arc;

use caskade_app::events::{Event, RemotePlayRequested};
use dioxus_native::prelude::*;
use tokio::sync::broadcast;

use crate::data::use_game;
use crate::game_menu::play::PlayRequest;

/// Answers request `id`: started, or why not (`None`: the core's words).
type Answer = Arc<dyn Fn(&str, bool, Option<String>) + Send + Sync>;

/// Where requests come from and where their answers go.
#[derive(Clone)]
pub struct RemotePlays {
    events: broadcast::Sender<Event>,
    answer: Answer,
}

impl RemotePlays {
    pub fn new(events: broadcast::Sender<Event>, answer: Answer) -> Self {
        Self { events, answer }
    }

    /// The running app's: its events, and its remote play's answers.
    pub fn of(services: &'static caskade_app::start::Services) -> Self {
        Self::new(
            crate::shell::events().clone(),
            Arc::new(|id, started, reason| {
                caskade_app::commands::remote_play::remote_play_finished(
                    &services.remote_play,
                    id,
                    started,
                    reason,
                );
            }),
        )
    }
}

impl PartialEq for RemotePlays {
    /// One app's: a flow's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// The app's remote plays: a test's, else the running app's.
pub fn use_remote_plays() -> Option<RemotePlays> {
    try_use_context::<RemotePlays>().or_else(|| crate::shell::services().map(RemotePlays::of))
}

/// Starts each request as it arrives.
#[component]
pub fn RemotePlayHost(plays: RemotePlays) -> Element {
    let mut requests = use_signal(Vec::<RemotePlayRequested>::new);
    {
        let mut events = plays.events.subscribe();
        use_hook(move || {
            spawn(async move {
                use broadcast::error::RecvError;
                loop {
                    match events.recv().await {
                        Ok(Event::RemotePlayRequested(request)) => requests.write().push(request),
                        Ok(_) | Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => break,
                    }
                }
            });
        });
    }
    rsx! {
        for request in requests() {
            Start {
                key: "{request.request_id}",
                plays: plays.clone(),
                request: request.clone(),
                on_done: move |id: String| requests.write().retain(|each| each.request_id != id),
            }
        }
    }
}

#[component]
fn Start(
    plays: RemotePlays,
    request: RemotePlayRequested,
    on_done: EventHandler<String>,
) -> Element {
    let game = use_game(request.game_id.clone());
    let starting = use_signal(|| None::<String>);
    let mut answered = use_signal(|| false);
    let title = game
        .read()
        .as_ref()
        .and_then(|game| {
            game.as_ref()
                .ok()?
                .as_ref()
                .map(|game| game.game.title.clone())
        })
        .unwrap_or_default();
    let id = request.request_id.clone();
    let answer = {
        let id = id.clone();
        use_callback(move |result: Result<(), Option<String>>| {
            if answered() {
                return;
            }
            answered.set(true);
            let (started, reason) = match result {
                Ok(()) => (true, None),
                Err(reason) => (false, reason),
            };
            (plays.answer)(&id, started, reason);
            on_done.call(id.clone());
        })
    };
    // Answered already, unless it closed without a word.
    let closed = move |_| answer.call(Err(None));
    rsx! {
        PlayRequest {
            game_id: request.game_id.clone(),
            title,
            entry_id: None,
            action: None,
            starting,
            on_close: closed,
            on_outcome: move |result| answer.call(result),
        }
    }
}

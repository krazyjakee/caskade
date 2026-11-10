//! The guide quick menu (the webview's `GuideMenu.tsx`): a pad's guide
//! button raises Caskade (`start_pads`); while a game runs, this menu then
//! offers going back to it, stopping it, or its details. A stream from
//! another device ends instead, and its game stops there.

use super::root::{Bind, use_input, use_input_action};
use blitz_dom::NodeId;
use caskade_core::tr;
use caskade_ui_input::Action;
use dioxus_native::prelude::*;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

/// A game running now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningGame {
    pub session_id: String,
    pub game_id: String,
    pub title: String,
    /// Streamed from another device.
    pub stream: bool,
}

type Pending<T> = Pin<Box<dyn Future<Output = T>>>;

/// What the menu does, in the app or in a test.
pub trait GuideApi {
    /// Running games, the latest first.
    fn running(&self) -> Pending<Vec<RunningGame>>;
    /// Steps aside so the game has the screen again.
    fn return_to_game(&self) -> Pending<Result<(), String>>;
    /// Ends the game; its session records the playtime.
    fn stop(&self, session_id: String) -> Pending<Result<(), String>>;
}

/// The menu's actions, for a prop.
#[derive(Clone)]
pub struct Guide(pub Rc<dyn GuideApi>);

impl PartialEq for Guide {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// The running app's: its launch service and library.
pub struct NativeGuide;

impl GuideApi for NativeGuide {
    fn running(&self) -> Pending<Vec<RunningGame>> {
        Box::pin(async {
            let found = caskade_app::runtime::spawn(async {
                let Some(services) = crate::shell::services() else {
                    return Vec::new();
                };
                let tickets = caskade_app::commands::launch::running_games(&services.launch)
                    .await
                    .unwrap_or_default();
                let mut games = Vec::new();
                for ticket in tickets {
                    let details =
                        caskade_app::commands::library::get_game(&services.store, &ticket.game_id)
                            .await
                            .ok()
                            .flatten();
                    let stream = details.as_ref().is_some_and(|details| {
                        details.entries.iter().any(|entry| {
                            entry.id == ticket.entry_id
                                && entry.source_id.as_str().starts_with("stream:")
                        })
                    });
                    games.push(RunningGame {
                        title: details
                            .map(|details| details.game.title)
                            .unwrap_or_else(|| tr!("Your game")),
                        session_id: ticket.session_id,
                        game_id: ticket.game_id,
                        stream,
                    });
                }
                games.reverse();
                games
            })
            .await;
            found.unwrap_or_default()
        })
    }

    fn return_to_game(&self) -> Pending<Result<(), String>> {
        Box::pin(async {
            // Out of the game's way: the window manager gives it focus back.
            match crate::shell::main_window() {
                Some(window) => {
                    window.set_minimized(true);
                    Ok(())
                }
                None => Err(tr!("the main window is gone")),
            }
        })
    }

    fn stop(&self, session_id: String) -> Pending<Result<(), String>> {
        Box::pin(async move {
            caskade_app::runtime::spawn(async move {
                let services =
                    crate::shell::services().ok_or_else(|| tr!("Caskade is starting"))?;
                caskade_app::commands::guide::guide_stop_game(session_id, &services.launch)
                    .await
                    .map(|_| ())
            })
            .await
            .unwrap_or_else(|error| Err(error.to_string()))
        })
    }
}

#[component]
pub fn GuideMenu(guide: Guide) -> Element {
    let input = use_input();
    let mut game = use_signal(|| None::<RunningGame>);
    let mut confirming = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut me = use_signal(|| None::<NodeId>);
    let mut close = move || {
        game.set(None);
        confirming.set(false);
        error.set(None);
    };
    let opener = guide.clone();
    let shown = input.clone();
    use_input_action(
        Action::Guide,
        // The guide button works over menus and dialogs too.
        Bind {
            label: Some(tr!("Quick menu")),
            global: true,
            ..Bind::default()
        },
        move || {
            // Pressed again, it closes the menu.
            if game.peek().is_some() {
                if !*busy.peek() {
                    close();
                }
                return true;
            }
            let api = opener.0.clone();
            let shown = shown.clone();
            spawn(async move {
                let running = api.running().await;
                confirming.set(false);
                error.set(None);
                game.set(running.into_iter().next());
                // Focus goes into the menu once it's drawn.
                shown.settle();
            });
            true
        },
    );
    use_input_action(
        Action::Back,
        Bind {
            owner: me(),
            ..Bind::default()
        },
        move || {
            // Closed, the menu leaves B to the view.
            if game.peek().is_none() {
                return false;
            }
            if !*busy.peek() {
                close();
            }
            true
        },
    );
    let mut act = move |run: Pin<Box<dyn Future<Output = Result<(), String>>>>| {
        busy.set(true);
        error.set(None);
        spawn(async move {
            match run.await {
                Ok(()) => close(),
                Err(failure) => error.set(Some(failure)),
            }
            busy.set(false);
        });
    };
    let Some(running) = game() else {
        return rsx! {};
    };
    let details = input.clone();
    let line = match (confirming(), running.stream) {
        (true, true) => tr!(
            "End the stream? The game stops on the other device, and progress it hasn't saved is lost."
        ),
        (true, false) => tr!("Stop the game? Progress it hasn't saved is lost."),
        (false, true) => tr!("is streaming from another device."),
        (false, false) => tr!("is running."),
    };
    let stop_label = if running.stream {
        tr!("End stream")
    } else {
        tr!("Stop {title}", title = running.title)
    };
    let disabled = busy();
    let session = running.session_id.clone();
    let game_id = running.game_id.clone();
    let returning = guide.clone();
    let stopping = guide.clone();
    rsx! {
        div {
            role: "dialog",
            "data-cs": "guide-menu",
            // Over everything, as a modal is.
            style: "position: fixed; inset: 0; display: flex; flex-direction: column; justify-content: center; align-items: center;",
            aria_label: tr!("Quick menu"),
            onmounted: move |event| {
                me.set(event.data().downcast::<dioxus_native::NodeHandle>().map(|h| h.node_id()))
            },
            h2 { "{running.title}" }
            p { "{line}" }
            if let Some(failure) = error() {
                p { role: "alert", "{failure}" }
            }
            div {
                // One column, as the webview's: the pad goes down the list.
                style: "display: flex; flex-direction: column;",
                if confirming() {
                    button {
                        "data-focus-default": "true",
                        disabled,
                        onclick: move |_| act(stopping.0.stop(session.clone())),
                        "{stop_label}"
                    }
                    button {
                        disabled,
                        onclick: move |_| confirming.set(false),
                        {tr!("Keep playing")}
                    }
                } else {
                    button {
                        "data-focus-default": "true",
                        disabled,
                        onclick: move |_| act(returning.0.return_to_game()),
                        {tr!("Return to game")}
                    }
                    button {
                        disabled,
                        onclick: move |_| confirming.set(true),
                        if running.stream { {tr!("End stream")} } else { {tr!("Stop game")} }
                    }
                    button {
                        disabled,
                        onclick: move |_| {
                            details.open_details(game_id.clone());
                            close();
                        },
                        {tr!("Game details")}
                    }
                }
            }
        }
    }
}

//! Runs one of a release's tools once (#178), as the webview's
//! `gameActions/ToolRequest`: the game menu's tool items start it, and it
//! says why when the tool couldn't start. A tool has no play session, so
//! nothing follows once it started. The admin's PIN an age limit asks for
//! is the age gate's (#106); until then the limit is the reason it didn't
//! start.

use caskade_app::commands::actions::{ToolOutcome, game_tool_run};
use caskade_core::launch::LaunchService;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::data::change;
use crate::ui::{Button, Dialog};

/// The app's launcher for a flow: a test's, else the running app's.
pub fn use_launch() -> Option<LaunchService> {
    try_use_context::<LaunchService>()
        .or_else(|| crate::shell::services().map(|services| services.launch.clone()))
}

/// Starts entry `entry_id`'s tool `action_id` (`name`) once it mounts;
/// `on_close` hears it started, or the failure read.
#[component]
pub fn ToolRequest(
    entry_id: String,
    action_id: String,
    name: String,
    on_close: EventHandler<()>,
) -> Element {
    let launch = use_launch();
    let mut failed = use_signal(|| None::<String>);
    use_hook(move || {
        spawn(async move {
            let Some(launch) = launch else {
                failed.set(Some(t!("That isn't available in this copy of Caskade.")));
                return;
            };
            let outcome =
                change(async move { game_tool_run(&launch, &entry_id, &action_id).await }).await;
            match outcome {
                Ok(ToolOutcome::Started) => on_close.call(()),
                Ok(ToolOutcome::AgeRestricted { .. }) => failed.set(Some(t!("Admin PIN needed"))),
                Err(report) => failed.set(Some(report.summary)),
            }
        });
    });
    let Some(reason) = failed() else {
        return rsx! {};
    };
    rsx! {
        Dialog {
            title: t!("{name} didn't start", name = name),
            alert: true,
            on_close: move |_| on_close.call(()),
            "data-cs": "tool-failed",
            "data-focus-scope": "tool-failed",
            p { class: "text-sm text-fg-muted", "{reason}" }
            div { class: "mt-4 flex justify-end",
                Button {
                    id: "tool-failed-ok",
                    "data-focus-default": "true",
                    on_press: move |_| on_close.call(()),
                    {t!("OK")}
                }
            }
        }
    }
}

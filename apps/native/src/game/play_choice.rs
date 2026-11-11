//! The game page's launch actions for the release Play starts (#100, the
//! webview's `components/gameActions/PlayChoice.tsx`): which play action
//! Play starts, or that it asks each time, and the release's tools.
//! Nothing shows while the release has one play action and no tools.

use super::data::{use_answer, use_page_data};
use crate::game_menu::{MenuDialog, Target, use_game_menus};
use crate::ui::{Button, Select, SelectOption, icons};
use caskade_core::model::{ActionKind, EntryActionSettings};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

/// "Ask each time"'s option.
const ASK: &str = "ask";

#[component]
pub fn PlayChoice(game_id: String, title: String, entry_id: String, disabled: bool) -> Element {
    let data = use_page_data();
    let menus = use_game_menus();
    let actions = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.actions)(game_id.clone())
        })
    };
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let release = actions
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok())
        .and_then(|list| list.iter().find(|each| each.entry_id == entry_id).cloned());
    let Some(release) = release else {
        return rsx! {};
    };
    let play: Vec<_> = release
        .actions
        .iter()
        .filter(|action| action.kind == ActionKind::Play)
        .cloned()
        .collect();
    let tools: Vec<_> = release
        .actions
        .iter()
        .filter(|action| action.kind == ActionKind::Utility)
        .cloned()
        .collect();
    let settings = release.settings.clone();
    let current = if settings.ask {
        Some(ASK.to_owned())
    } else {
        play.iter()
            .find(|action| Some(&action.id) == settings.default_action.as_ref())
            .or(play.first())
            .map(|action| action.id.clone())
    };
    let mut options: Vec<SelectOption> = play
        .iter()
        .map(|action| SelectOption::new(action.id.clone(), action.name.clone()))
        .collect();
    options.push(SelectOption::new(ASK, t!("Ask each time")));
    let tool_game = game_id.clone();
    let choose = move |value: String| {
        let next = if value == ASK {
            EntryActionSettings {
                ask: true,
                ..settings.clone()
            }
        } else {
            EntryActionSettings {
                ask: false,
                default_action: Some(value),
                ..settings.clone()
            }
        };
        let (data, game_id, entry_id) = (data.clone(), game_id.clone(), entry_id.clone());
        saving.set(true);
        error.set(None);
        spawn(async move {
            if let Err(report) = (data.save_action_settings)(game_id, entry_id, next).await {
                error.set(Some(report.summary));
            }
            saving.set(false);
        });
    };
    if play.len() <= 1 && tools.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { "data-cs": "play-choice", class: "flex flex-col gap-3",
            if play.len() > 1 && current.is_some() {
                Select {
                    label: t!(note = "Picks which of a release's launch actions Play starts", "Play starts"),
                    "data-part": "play-starts",
                    options,
                    value: current.clone(),
                    disabled: disabled || saving(),
                    on_change: choose,
                }
            }
            if let Some(error) = error() {
                p { role: "alert", class: "text-sm text-danger", "{error}" }
            }
            if !tools.is_empty() && let Some(menus) = menus {
                div { "data-part": "tools", class: "flex flex-wrap gap-2",
                    for tool in tools {
                        Button {
                            key: "{tool.id}",
                            "data-spatial": "focusable",
                            icon: Some(icons::WRENCH),
                            on_press: {
                                let target = Target { game_id: tool_game.clone(), title: title.clone(), entry_id: None };
                                let dialog = MenuDialog::Tool {
                                    entry_id: release.entry_id.clone(),
                                    action_id: tool.id.clone(),
                                    name: tool.name.clone(),
                                };
                                move |_| menus.dialog(target.clone(), dialog.clone())
                            },
                            "{tool.name}"
                        }
                    }
                }
            }
        }
    }
}

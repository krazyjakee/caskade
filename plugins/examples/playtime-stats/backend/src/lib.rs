//! Playtime stats (#133; plugin API 0.2 since #109): a view with the most
//! played games as a bar chart or a table, and the last session, kept when
//! a game exits. It reads the library (`library: read`) and draws with
//! Caskade's components, so it works in console mode too, where LB and RB
//! switch between the chart and the table.

use caskade_plugin_sdk::{
    Error, Plugin, host,
    i18n::{plural, t, tf},
    json, payloads, ui,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Page {
    items: Vec<Game>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Game {
    title: String,
    playtime_ms: u64,
}

/// One game, as `{"game": "<id>"}` answers.
#[derive(Deserialize)]
struct Named {
    title: String,
}

/// The last session, as `on_event` keeps it.
#[derive(Serialize, Deserialize)]
struct Last {
    title: String,
    minutes: u64,
}

fn hours(ms: u64) -> f64 {
    ms as f64 / 3_600_000.0
}

/// "12 h", or "1.5 h" under ten hours.
fn format(ms: u64) -> String {
    let value = hours(ms);
    let number = if value >= 10.0 {
        format!("{}", value.round())
    } else {
        format!("{value:.1}")
    };
    tf("{hours} h", &[("hours", &number)])
}

fn most_played() -> Result<Vec<Game>, Error> {
    let page: Page = json::from_str(&host::library_query(
        r#"{"games": {"sort": "playtime", "limit": 10}}"#,
    )?)?;
    Ok(page
        .items
        .into_iter()
        .filter(|game| game.playtime_ms > 0)
        .collect())
}

struct Stats;

impl Plugin for Stats {
    fn render(slot: &str, id: &str, context: &ui::Context) -> Result<Vec<ui::Node>, Error> {
        if (slot, id) != ("views", "stats") {
            return Err(Error::new("no_ui", format!("no {slot} {id}")));
        }
        let played = most_played()?;
        let mut tree = Vec::new();
        if let Some(last) = host::kv_json::<Last>("last")? {
            tree.push(ui::note(tf(
                "Last session: {title}, {minutes} min",
                &[
                    ("title", &last.title),
                    ("minutes", &last.minutes.to_string()),
                ],
            )));
        }
        if played.is_empty() {
            tree.push(ui::note(t(
                "No playtime yet. Play something and come back.",
            )));
            return Ok(tree);
        }
        let total: u64 = played.iter().map(|game| game.playtime_ms).sum();
        let view = context
            .state
            .get("view")
            .and_then(|view| view.as_str())
            .unwrap_or("chart")
            .to_owned();
        let shown = if view == "table" {
            ui::Node::Table {
                caption: t("Most played games"),
                columns: vec![t("Game"), t("Hours played")],
                rows: played
                    .iter()
                    .map(|game| {
                        vec![
                            game.title.clone(),
                            format!("{:.1}", hours(game.playtime_ms)),
                        ]
                    })
                    .collect(),
                row_headers: true,
            }
        } else {
            ui::bars(
                t("Most played games, hours played"),
                played.iter().map(|game| {
                    ui::bar(
                        game.title.clone(),
                        hours(game.playtime_ms),
                        format(game.playtime_ms),
                    )
                }),
            )
        };
        tree.push(ui::section(
            t("Most played"),
            [
                ui::tile(
                    format(total),
                    plural(
                        "across your most played game",
                        "across your {count} most played games",
                        played.len() as u64,
                    ),
                ),
                ui::row([
                    ui::heading(t("Most played")),
                    ui::segmented(
                        t("Show as"),
                        "view",
                        view,
                        [("chart", t("Chart")), ("table", t("Table"))],
                    ),
                ]),
                shown,
            ],
        ));
        Ok(tree)
    }

    fn act(
        _slot: &str,
        _id: &str,
        action: &str,
        value: &serde_json::Value,
        _context: &ui::Context,
    ) -> Result<ui::Outcome, Error> {
        match action {
            "view" => Ok(ui::Outcome {
                state: Some(serde_json::json!({ "view": value })),
                notice: None,
            }),
            _ => Err(Error::unknown_method(action)),
        }
    }

    fn on_event(name: &str, payload: &str) {
        if name != "game.exited" {
            return;
        }
        let Ok(session) = json::event::<payloads::GameSession>(payload) else {
            return;
        };
        let request = serde_json::json!({ "game": session.game_id }).to_string();
        let title = host::library_query(&request)
            .ok()
            .and_then(|answer| {
                serde_json::from_str::<Option<Named>>(&answer)
                    .ok()
                    .flatten()
            })
            .map(|game| game.title)
            .unwrap_or_else(|| t("a game"));
        let last = Last {
            title,
            minutes: (session.duration_ms / 60_000).max(1),
        };
        let _ = host::set_kv_json("last", &last);
    }
}

caskade_plugin_sdk::export!(Stats);

//! HowLongToBeat (#133; plugin API 0.2 since #109): a game page tab with how
//! long the game takes to beat. It looks the title up on howlongtobeat.com
//! through `wasi:http` (the `network` permission allows that one host),
//! keeps the answer in the plugin's key-value store, and draws the times
//! with Caskade's components.
use caskade_plugin_sdk::{
    Error, Plugin, host,
    i18n::{t, tf},
    json, ui,
};
use serde::{Deserialize, Serialize};

mod http;

#[derive(Deserialize)]
#[serde(default)]
struct Settings {
    endpoint: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            endpoint: "https://howlongtobeat.com/api/search".into(),
        }
    }
}

#[derive(Deserialize)]
struct Lookup {
    title: String,
}

/// What the tab shows; hours rounded to halves.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Times {
    title: String,
    main_hours: Option<f64>,
    extras_hours: Option<f64>,
    completionist_hours: Option<f64>,
}

/// One search result; times are seconds.
#[derive(Deserialize)]
struct Hit {
    game_name: String,
    #[serde(default)]
    comp_main: u64,
    #[serde(default)]
    comp_plus: u64,
    #[serde(default)]
    comp_100: u64,
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    data: Vec<Hit>,
}

fn hours(seconds: u64) -> Option<f64> {
    (seconds > 0).then(|| (seconds as f64 / 1800.0).round() / 2.0)
}

fn search(title: &str) -> Result<Option<Times>, Error> {
    let settings: Settings = host::settings()?.unwrap_or_default();
    let body = serde_json::json!({
        "searchType": "games",
        "searchTerms": title.split_whitespace().collect::<Vec<_>>(),
        "searchPage": 1,
        "size": 5,
    });
    let (status, text) = http::post_json(&settings.endpoint, &body.to_string())
        .map_err(|message| Error::new("http", message))?;
    if status != 200 {
        return Err(Error::new(
            "http",
            format!("HowLongToBeat answered {status}"),
        ));
    }
    let search: Search = json::from_str(&text)?;
    let wanted = title.to_lowercase();
    let best = search
        .data
        .iter()
        .find(|hit| hit.game_name.to_lowercase() == wanted)
        .or(search.data.first());
    Ok(best.map(|hit| Times {
        title: hit.game_name.clone(),
        main_hours: hours(hit.comp_main),
        extras_hours: hours(hit.comp_plus),
        completionist_hours: hours(hit.comp_100),
    }))
}

/// The times for `title`, from the plugin's store, else looked up and
/// kept.
fn lookup(title: &str) -> Result<Option<Times>, Error> {
    let key = format!("times:{}", title.trim().to_lowercase());
    if let Some(cached) = host::kv_json::<Option<Times>>(&key)? {
        return Ok(cached);
    }
    let found = search(title.trim())?;
    host::set_kv_json(&key, &found)?;
    Ok(found)
}

fn hours_text(value: Option<f64>) -> String {
    match value {
        Some(hours) => tf("{hours} h", &[("hours", &hours.to_string())]),
        None => "—".to_owned(),
    }
}

struct HowLongToBeat;

impl Plugin for HowLongToBeat {
    /// The game page's "How long to beat" tab (plugin API 0.2, #109).
    fn render(slot: &str, id: &str, context: &ui::Context) -> Result<Vec<ui::Node>, Error> {
        if (slot, id) != ("game.details.tabs", "hltb") {
            return Err(Error::new("no_ui", format!("no {slot} {id}")));
        }
        let Some(title) = &context.game_title else {
            return Ok(vec![ui::note(t("This game has no title to look up."))]);
        };
        let times = match lookup(title) {
            Ok(times) => times,
            Err(error) => {
                return Ok(vec![ui::alert(tf(
                    "HowLongToBeat couldn't be reached: {error}",
                    &[("error", &error.message)],
                ))]);
            }
        };
        let Some(times) = times else {
            return Ok(vec![ui::note(tf(
                "HowLongToBeat has no times for {title}.",
                &[("title", title)],
            ))]);
        };
        Ok(vec![ui::section(
            tf("How long to beat {title}", &[("title", &times.title)]),
            [
                ui::stats([
                    (t("Main story"), hours_text(times.main_hours)),
                    (t("Main + extras"), hours_text(times.extras_hours)),
                    (t("Completionist"), hours_text(times.completionist_hours)),
                ]),
                ui::note(tf(
                    "From howlongtobeat.com: {title}",
                    &[("title", &times.title)],
                )),
            ],
        )])
    }

    /// `lookup {"title"}`: the times as JSON, for scripts and tests.
    fn request(method: &str, payload: &str) -> Result<String, Error> {
        match method {
            "lookup" => {
                let Lookup { title } = json::from_str(payload)?;
                json::to_string(&lookup(&title)?)
            }
            _ => Err(Error::unknown_method(method)),
        }
    }
}

caskade_plugin_sdk::export!(HowLongToBeat);

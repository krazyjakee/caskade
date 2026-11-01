//! HowLongToBeat (#133): looks a title up on howlongtobeat.com through
//! `wasi:http` (the `network` permission allows that one host) and keeps the
//! answer in the plugin's key-value store.
use caskade_plugin_sdk::{Error, Plugin, host, json};
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
        return Err(Error::new("http", format!("HowLongToBeat answered {status}")));
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

struct HowLongToBeat;

impl Plugin for HowLongToBeat {
    fn request(method: &str, payload: &str) -> Result<String, Error> {
        match method {
            "lookup" => {
                let Lookup { title } = json::from_str(payload)?;
                let key = format!("times:{}", title.trim().to_lowercase());
                let times = match host::kv_json::<Option<Times>>(&key)? {
                    Some(cached) => cached,
                    None => {
                        let found = search(title.trim())?;
                        host::set_kv_json(&key, &found)?;
                        found
                    }
                };
                json::to_string(&times)
            }
            _ => Err(Error::unknown_method(method)),
        }
    }
}

caskade_plugin_sdk::export!(HowLongToBeat);

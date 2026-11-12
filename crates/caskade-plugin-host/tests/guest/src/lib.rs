//! A test plugin: echoes, stores, emits, intercepts, and misbehaves on
//! request so the host's limits can be tested. It is written with
//! `caskade-plugin-sdk`, like a real plugin backend.

use caskade_plugin_sdk::{Error, Intercept, Level, Plugin, host, i18n, ui};

struct TestPlugin;

impl Plugin for TestPlugin {
    fn request(method: &str, payload: &str) -> Result<String, Error> {
        match method {
            "echo" => Ok(payload.to_owned()),
            // Outside install-handler calls there is no input to read.
            "read-input" => host::install_input_read(0, 1)
                .map(|_| String::new())
                .map_err(Error::from),
            // Provider answers come from the test: `store:<key>` keeps one.
            store if store.starts_with("store:") => {
                host::kv_set(&store["store:".len()..], payload.as_bytes());
                Ok(String::new())
            }
            "kv" => {
                host::kv_set("key", payload.as_bytes());
                let stored = host::kv_get("key").unwrap_or_default();
                host::kv_delete("key");
                let gone = host::kv_get("key").is_none();
                Ok(format!("{}:{gone}", String::from_utf8_lossy(&stored)))
            }
            "settings" => Ok(host::settings_get().unwrap_or_else(|| "null".into())),
            // The language Caskade shows text in (#219), and a message in it.
            "locale" => Ok(host::locale()),
            "greeting" => Ok(match host::locale().split('-').next() {
                Some("sv") => "Hej".into(),
                _ => "Hello".into(),
            }),
            "emit" => {
                host::emit("greeted", payload);
                Ok(String::new())
            }
            "http" => http_status(payload).map_err(|message| Error::new("http", message)),
            "read" => std::fs::read_to_string(payload)
                .map_err(|error| Error::new("fs", error.to_string())),
            "write" => std::fs::write(payload, b"written")
                .map(|()| "ok".into())
                .map_err(|error| Error::new("fs", error.to_string())),
            "library" => host::library_query(payload).map_err(Error::from),
            "library-update" => host::library_update(payload).map_err(Error::from),
            "spin" => loop {
                core::hint::spin_loop();
            },
            "trap" => panic!("the plugin failed"),
            "grow" => {
                let big = vec![1u8; 256 << 20];
                Ok(big.iter().map(|b| *b as usize).sum::<usize>().to_string())
            }
            _ => Err(Error::unknown_method(method)),
        }
    }

    fn on_event(name: &str, payload: &str) {
        host::log(Level::Info, &format!("{name} {payload}"));
        host::kv_set(&format!("event:{name}"), payload.as_bytes());
    }

    fn intercept(point: &str, payload: &str) -> Result<Intercept, Error> {
        if payload.contains("trap-me") {
            panic!("the interceptor failed");
        }
        if payload.contains("spin-me") {
            loop {
                core::hint::spin_loop();
            }
        }
        if payload.contains("cancel-me") {
            return Ok(Intercept::Cancel(format!("{point}: not today")));
        }
        Ok(Intercept::Continue(
            payload.replace("\"program\":\"", "\"program\":\"gamemoderun "),
        ))
    }

    fn library_scan() -> Result<String, Error> {
        Ok(stored("scan", "[]"))
    }

    fn metadata_lookup(game: &str, fields: Vec<String>) -> Result<String, Error> {
        host::kv_set(
            "looked-up",
            format!("{game} {}", fields.join(",")).as_bytes(),
        );
        Ok(stored("metadata", "{}"))
    }

    fn runner_prepare(spec: &str) -> Result<String, Error> {
        host::kv_set("prepared", spec.as_bytes());
        Ok(stored("runner", "{}"))
    }

    /// Claims inputs that start with `CUSTOM`, read through the host.
    fn install_claims(_input: &str) -> bool {
        host::install_input_read(0, 6).is_ok_and(|magic| magic == b"CUSTOM")
    }

    /// The rest of the input becomes `game.bin`.
    fn install_plan(input: &str) -> Result<String, Error> {
        let artifact = input
            .split("\"artifactId\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default();
        let size = input
            .split("\"size\":")
            .nth(1)
            .and_then(|rest| rest.split([',', '}']).next())
            .and_then(|size| size.parse::<u64>().ok())
            .unwrap_or(6);
        Ok(stored(
            "plan",
            &format!(
                r#"{{"steps": [{{"type": "copy_range", "value": {{"from": "{artifact}", "offset": 6, "length": {}, "path": "game.bin"}}}}]}}"#,
                size - 6
            ),
        ))
    }

    /// Draws a view (#109): a heading from its catalog, a segmented
    /// control holding the state `act` returned, a plural, and the game in a
    /// game slot. `bad` draws an action Caskade refuses; `fail` fails.
    fn render(slot: &str, id: &str, context: &ui::Context) -> Result<Vec<ui::Node>, Error> {
        match id {
            "bad" => return Ok(vec![ui::button("Go", "has space")]),
            "fail" => return Err(Error::new("lookup", "the service is down")),
            _ => {}
        }
        let view = context
            .state
            .get("view")
            .and_then(|view| view.as_str())
            .unwrap_or("chart")
            .to_owned();
        let mut tree = vec![
            ui::heading(i18n::t("Hello")),
            ui::segmented(
                i18n::t("Show as"),
                "view",
                view,
                [("chart", i18n::t("Chart")), ("table", i18n::t("Table"))],
            ),
            ui::note(i18n::plural("{count} game", "{count} games", 2)),
            ui::text(format!("{slot} {:?}", context.mode)),
        ];
        if let Some(game) = &context.game_id {
            tree.push(ui::text(format!("game {game}")));
        }
        Ok(tree)
    }

    fn act(
        slot: &str,
        id: &str,
        action: &str,
        value: &serde_json::Value,
        _context: &ui::Context,
    ) -> Result<ui::Outcome, Error> {
        match action {
            "view" => Ok(ui::Outcome {
                state: Some(serde_json::json!({"view": value})),
                notice: None,
            }),
            "run" => Ok(ui::Outcome {
                state: None,
                notice: Some(i18n::tf("Ran {what}", &[("what", &format!("{slot} {id}"))])),
            }),
            _ => Err(Error::unknown_method(action)),
        }
    }

    fn resolve_url(url: &str) -> Result<String, Error> {
        Ok(if url.contains("/page") {
            format!(
                r#"[{{"url": "{}", "headers": {{"x-token": "secret"}}}}]"#,
                url.replace("/page", "/download")
            )
        } else {
            "[]".into()
        })
    }
}

caskade_plugin_sdk::export!(TestPlugin);

/// GETs `url` (`http://host:port/path`) and follows one redirect; returns
/// the final status or the error.
fn http_status(url: &str) -> Result<String, String> {
    let response = get(url)?;
    if (300..400).contains(&response.0) {
        let location = response.1.ok_or("redirect without a location")?;
        return get(&location).map(|(status, _)| status.to_string());
    }
    Ok(response.0.to_string())
}

fn get(url: &str) -> Result<(u16, Option<String>), String> {
    use wasi::http::{outgoing_handler, types};
    let rest = url.strip_prefix("http://").ok_or("only http:// URLs")?;
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_owned()), |(a, p)| (a, format!("/{p}")));
    let request = types::OutgoingRequest::new(types::Fields::new());
    request
        .set_scheme(Some(&types::Scheme::Http))
        .map_err(|()| "scheme")?;
    request
        .set_authority(Some(authority))
        .map_err(|()| "authority")?;
    request
        .set_path_with_query(Some(&path))
        .map_err(|()| "path")?;
    let pending = outgoing_handler::handle(request, None).map_err(|error| format!("{error:?}"))?;
    pending.subscribe().block();
    let response = pending
        .get()
        .ok_or("no response")?
        .map_err(|()| "response taken twice")?
        .map_err(|error| format!("{error:?}"))?;
    let location = response
        .headers()
        .get(&"location".to_owned())
        .first()
        .map(|value| String::from_utf8_lossy(value).into_owned());
    Ok((response.status(), location))
}

fn stored(key: &str, fallback: &str) -> String {
    host::kv_get(key)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|| fallback.to_owned())
}

//! A backend built for `caskade:plugin@0.1.1` (#109), against the frozen
//! 0.1 world: the host still links it while 0.1 plugins are deprecated. It
//! echoes, asks the host for the language, and cancels an interceptor.

wit_bindgen::generate!({
    path: "../../wit-0.1",
    world: "plugin",
});

use caskade::plugin::{
    host,
    types::{InterceptResult, PluginError},
};
use exports::caskade::plugin::{hooks, requests};

struct Old;

impl requests::Guest for Old {
    fn handle_request(method: String, payload: String) -> Result<String, PluginError> {
        match method.as_str() {
            "echo" => Ok(payload),
            "locale" => Ok(host::locale()),
            "kv" => {
                host::kv_set("key", payload.as_bytes());
                Ok(String::from_utf8_lossy(&host::kv_get("key").unwrap_or_default()).into_owned())
            }
            _ => Err(PluginError {
                code: "unknown_method".into(),
                message: method,
            }),
        }
    }
}

impl hooks::Guest for Old {
    fn on_event(name: String, payload: String) {
        host::kv_set(&format!("event:{name}"), payload.as_bytes());
    }
    fn intercept(point: String, _payload: String) -> Result<InterceptResult, PluginError> {
        Ok(InterceptResult::Cancel(format!("{point}: old")))
    }
}

export!(Old);

//! Rust SDK for Caskade plugin backends.
//!
//! A backend is a WebAssembly component for the `caskade:plugin@0.1.1`
//! world. This crate carries the bindings, so a plugin implements one
//! [`Plugin`] trait (every method has a default) and calls [`export!`]:
//!
//! ```ignore
//! use caskade_plugin_sdk::{Error, Plugin, host, json, payloads};
//!
//! struct Stats;
//!
//! impl Plugin for Stats {
//!     fn on_event(name: &str, payload: &str) {
//!         // Event payloads arrive as `{"version": 1, "data": …}`.
//!         if name == "game.exited"
//!             && let Ok(session) = json::event::<payloads::GameSession>(payload)
//!         {
//!             host::emit("stats.updated", &session.game_id);
//!         }
//!     }
//!     fn request(method: &str, payload: &str) -> Result<String, Error> {
//!         match method {
//!             "hours" => json::to_string(&serde_json::json!({"hours": 12})),
//!             _ => Err(Error::unknown_method(method)),
//!         }
//!     }
//! }
//!
//! caskade_plugin_sdk::export!(Stats);
//! ```
//!
//! Build with `cargo build --release --target wasm32-wasip2` and a
//! `crate-type = ["cdylib"]` library. Payloads cross as JSON text; the
//! [`json`] helpers parse and write them. The crate's version is the WIT
//! package's (§4.4).

/// The raw `wit-bindgen` bindings of the `all-providers` world, for what the
/// trait doesn't cover.
#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "all-providers",
        pub_export_macro: true,
        export_macro_name: "__export_bindings",
        default_bindings_module: "caskade_plugin_sdk::bindings",
    });
}

pub use bindings::caskade::plugin::types::Level;
use bindings::caskade::plugin::types::{InterceptResult, PluginError};

/// The `caskade:plugin` version these bindings implement.
pub const API_VERSION: &str = "0.1.1";

/// A failure the plugin reports; the host logs it under the plugin's id and
/// shows `message` where the call came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: String,
    pub message: String,
}

impl Error {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    /// `handle-request` was asked for a method the plugin doesn't have.
    pub fn unknown_method(method: &str) -> Self {
        Self::new("unknown_method", method)
    }
    /// A provider the plugin exports but doesn't implement.
    pub fn not_provided(provider: &str) -> Self {
        Self::new("not_provided", format!("this plugin is not a {provider}"))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::new("json", error.to_string())
    }
}

impl From<Error> for PluginError {
    fn from(error: Error) -> Self {
        Self {
            code: error.code,
            message: error.message,
        }
    }
}

impl From<PluginError> for Error {
    fn from(error: PluginError) -> Self {
        Self::new(error.code, error.message)
    }
}

/// What an interceptor decided (`intercept`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intercept {
    /// Go on with this (possibly rewritten) JSON payload.
    Continue(String),
    /// Stop the chain; the reason is shown to the user.
    Cancel(String),
}

impl From<Intercept> for InterceptResult {
    fn from(intercept: Intercept) -> Self {
        match intercept {
            Intercept::Continue(payload) => Self::Continue(payload),
            Intercept::Cancel(reason) => Self::Cancel(reason),
        }
    }
}

/// A plugin backend. Hooks run for what the manifest lists in
/// `contributes.hooks`; providers only for `contributes.providers`. Every
/// method has a default, so a plugin implements what it declares.
pub trait Plugin {
    /// An event from `contributes.hooks` (`game.exited`, `settings.changed`, …).
    fn on_event(name: &str, payload: &str) {
        let _ = (name, payload);
    }
    /// One step of an interceptor chain (`launch.before`, `library.import`, …).
    fn intercept(point: &str, payload: &str) -> Result<Intercept, Error> {
        let _ = point;
        Ok(Intercept::Continue(payload.to_owned()))
    }
    /// A call from the plugin's own UI half (`ctx.backend.call`).
    fn request(method: &str, payload: &str) -> Result<String, Error> {
        let _ = payload;
        Err(Error::unknown_method(method))
    }
    /// `library-source`: the games the source knows, as a JSON list.
    fn library_scan() -> Result<String, Error> {
        Err(Error::not_provided("library-source"))
    }
    /// `metadata-provider`: the requested `fields` for a JSON game reference.
    fn metadata_lookup(game: &str, fields: Vec<String>) -> Result<String, Error> {
        let _ = (game, fields);
        Err(Error::not_provided("metadata-provider"))
    }
    /// `runner`: the command to run for a JSON launch spec.
    fn runner_prepare(spec: &str) -> Result<String, Error> {
        let _ = spec;
        Err(Error::not_provided("runner"))
    }
    /// `install-handler`: whether the plugin knows this input.
    fn install_claims(input: &str) -> bool {
        let _ = input;
        false
    }
    /// `install-handler`: a JSON plan of installer steps for the input.
    fn install_plan(input: &str) -> Result<String, Error> {
        let _ = input;
        Err(Error::not_provided("install-handler"))
    }
    /// `url-resolver`: a JSON list of downloads for a web page URL.
    fn resolve_url(url: &str) -> Result<String, Error> {
        let _ = url;
        Err(Error::not_provided("url-resolver"))
    }
}

/// What the host offers every backend.
pub mod host {
    pub use crate::bindings::caskade::plugin::host::{
        emit, install_input_read, kv_delete, kv_get, kv_set, library_query, library_update, locale,
        log, settings_get,
    };
    use serde::de::DeserializeOwned;

    /// The plugin's saved settings (`contributes.settings`), if any.
    pub fn settings<T: DeserializeOwned>() -> Result<Option<T>, crate::Error> {
        settings_get()
            .map(|text| serde_json::from_str(&text).map_err(crate::Error::from))
            .transpose()
    }

    /// A JSON value from the plugin's key-value store.
    pub fn kv_json<T: DeserializeOwned>(key: &str) -> Result<Option<T>, crate::Error> {
        kv_get(key)
            .map(|bytes| serde_json::from_slice(&bytes).map_err(crate::Error::from))
            .transpose()
    }

    /// Stores a JSON value in the plugin's key-value store.
    pub fn set_kv_json<T: serde::Serialize>(key: &str, value: &T) -> Result<(), crate::Error> {
        kv_set(key, &serde_json::to_vec(value)?);
        Ok(())
    }
}

/// JSON payload helpers: every payload crosses as JSON text.
pub mod json {
    use serde::{Serialize, de::DeserializeOwned};

    /// Parses a payload.
    pub fn from_str<T: DeserializeOwned>(payload: &str) -> Result<T, crate::Error> {
        Ok(serde_json::from_str(payload)?)
    }

    /// Writes an answer.
    pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, crate::Error> {
        Ok(serde_json::to_string(value)?)
    }

    /// The data of an `on-event` payload, which arrives wrapped as
    /// `{"version": 1, "data": …}` ([`crate::payloads::Event`]). A version
    /// the plugin doesn't know is an error, not a misread.
    pub fn event<T: DeserializeOwned>(payload: &str) -> Result<T, crate::Error> {
        let event: crate::payloads::Event<T> = from_str(payload)?;
        if event.version != crate::payloads::EVENT_VERSION {
            return Err(crate::Error::new(
                "event_version",
                format!("event payload version {} is not supported", event.version),
            ));
        }
        Ok(event.data)
    }
}

/// Payload shapes the core sends. `on-event` payloads are wrapped in
/// [`payloads::Event`] (read them with [`json::event`]); `intercept` payloads
/// are the point's JSON as is.
pub mod payloads {
    use serde::{Deserialize, Serialize};

    /// The `on-event` envelope version this SDK reads.
    pub const EVENT_VERSION: u32 = 1;

    /// Every `on-event` payload: `{"version": 1, "data": …}`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Event<T> {
        pub version: u32,
        pub data: T,
    }

    /// `game.started` and `game.exited`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GameSession {
        pub game_id: String,
        pub entry_id: String,
        pub session_id: String,
        /// Unix milliseconds.
        pub started_at_ms: i64,
        /// Unix milliseconds; `None` while running.
        pub ended_at_ms: Option<i64>,
        pub duration_ms: u64,
    }

    /// `settings.changed` for this plugin's own settings (#128).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SettingsChanged {
        pub namespace: String,
        pub revision: u32,
        pub value: serde_json::Value,
    }
}

/// Glue between [`Plugin`] and the generated export traits.
#[doc(hidden)]
pub mod __private {
    use super::{Plugin, bindings::exports::caskade::plugin as e};
    use crate::bindings::caskade::plugin::types::{InterceptResult, PluginError};

    pub fn on_event<P: Plugin>(name: String, payload: String) {
        P::on_event(&name, &payload);
    }
    pub fn intercept<P: Plugin>(
        point: String,
        payload: String,
    ) -> Result<InterceptResult, PluginError> {
        P::intercept(&point, &payload)
            .map(Into::into)
            .map_err(Into::into)
    }
    pub fn handle_request<P: Plugin>(
        method: String,
        payload: String,
    ) -> Result<String, PluginError> {
        P::request(&method, &payload).map_err(Into::into)
    }
    pub fn scan<P: Plugin>() -> Result<String, PluginError> {
        P::library_scan().map_err(Into::into)
    }
    pub fn lookup<P: Plugin>(game: String, fields: Vec<String>) -> Result<String, PluginError> {
        P::metadata_lookup(&game, fields).map_err(Into::into)
    }
    pub fn prepare<P: Plugin>(spec: String) -> Result<String, PluginError> {
        P::runner_prepare(&spec).map_err(Into::into)
    }
    pub fn claims<P: Plugin>(input: String) -> bool {
        P::install_claims(&input)
    }
    pub fn plan<P: Plugin>(input: String) -> Result<String, PluginError> {
        P::install_plan(&input).map_err(Into::into)
    }
    pub fn resolve<P: Plugin>(url: String) -> Result<String, PluginError> {
        P::resolve_url(&url).map_err(Into::into)
    }
    // Keeps the generated module paths referenced from one place.
    pub use e::{
        hooks, install_handler, library_source, metadata_provider, requests, runner, url_resolver,
    };
}

/// Exports a [`Plugin`] as the component's backend: every interface of the
/// world, with the trait's defaults for what the plugin doesn't implement.
#[macro_export]
macro_rules! export {
    ($plugin:ident) => {
        const _: () = {
            use $crate::__private as __c;
            impl __c::hooks::Guest for $plugin {
                fn on_event(name: String, payload: String) {
                    __c::on_event::<$plugin>(name, payload)
                }
                fn intercept(
                    point: String,
                    payload: String,
                ) -> Result<
                    $crate::bindings::caskade::plugin::types::InterceptResult,
                    $crate::bindings::caskade::plugin::types::PluginError,
                > {
                    __c::intercept::<$plugin>(point, payload)
                }
            }
            impl __c::requests::Guest for $plugin {
                fn handle_request(
                    method: String,
                    payload: String,
                ) -> Result<String, $crate::bindings::caskade::plugin::types::PluginError> {
                    __c::handle_request::<$plugin>(method, payload)
                }
            }
            impl __c::library_source::Guest for $plugin {
                fn scan() -> Result<String, $crate::bindings::caskade::plugin::types::PluginError> {
                    __c::scan::<$plugin>()
                }
            }
            impl __c::metadata_provider::Guest for $plugin {
                fn lookup(
                    game: String,
                    fields: Vec<String>,
                ) -> Result<String, $crate::bindings::caskade::plugin::types::PluginError> {
                    __c::lookup::<$plugin>(game, fields)
                }
            }
            impl __c::runner::Guest for $plugin {
                fn prepare(
                    spec: String,
                ) -> Result<String, $crate::bindings::caskade::plugin::types::PluginError> {
                    __c::prepare::<$plugin>(spec)
                }
            }
            impl __c::install_handler::Guest for $plugin {
                fn claims(input: String) -> bool {
                    __c::claims::<$plugin>(input)
                }
                fn plan(
                    input: String,
                ) -> Result<String, $crate::bindings::caskade::plugin::types::PluginError> {
                    __c::plan::<$plugin>(input)
                }
            }
            impl __c::url_resolver::Guest for $plugin {
                fn resolve(
                    url: String,
                ) -> Result<String, $crate::bindings::caskade::plugin::types::PluginError> {
                    __c::resolve::<$plugin>(url)
                }
            }
    $crate::bindings::__export_bindings!($plugin with_types_in $crate::bindings);
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Quiet;
    impl Plugin for Quiet {}

    #[test]
    fn defaults_answer_like_a_plugin_that_declares_nothing() {
        assert_eq!(
            Quiet::intercept("launch.before", r#"{"program":"game"}"#),
            Ok(Intercept::Continue(r#"{"program":"game"}"#.into()))
        );
        assert_eq!(
            Quiet::request("hours", ""),
            Err(Error::unknown_method("hours"))
        );
        assert_eq!(Quiet::library_scan().unwrap_err().code, "not_provided");
        assert!(!Quiet::install_claims("{}"));
        assert_eq!(
            Quiet::resolve_url("https://example.com").unwrap_err().code,
            "not_provided"
        );
    }

    #[test]
    fn errors_and_intercepts_cross_as_the_wit_types() {
        let error: PluginError = Error::new("http", "refused").into();
        assert_eq!(
            (error.code.as_str(), error.message.as_str()),
            ("http", "refused")
        );
        assert!(matches!(
            InterceptResult::from(Intercept::Cancel("no".into())),
            InterceptResult::Cancel(reason) if reason == "no"
        ));
        let parse = json::from_str::<payloads::GameSession>("{").unwrap_err();
        assert_eq!(parse.code, "json");
    }

    #[test]
    fn payloads_match_what_the_core_sends() {
        let session: payloads::GameSession = json::event(
            r#"{"version":1,"data":{"gameId":"g","entryId":"e","sessionId":"s","startedAtMs":1,"endedAtMs":null,"durationMs":0}}"#,
        )
        .unwrap();
        let unknown = json::event::<serde_json::Value>(r#"{"version":2,"data":null}"#);
        assert_eq!(unknown.unwrap_err().code, "event_version");
        assert_eq!(session.ended_at_ms, None);
        let changed: payloads::SettingsChanged =
            json::event(r#"{"version":1,"data":{"namespace":"plugins.dev.x","revision":2,"value":{"limit":8}}}"#)
                .unwrap();
        assert_eq!(changed.value["limit"], 8);
        assert_eq!(json::to_string(&changed.value).unwrap(), r#"{"limit":8}"#);
    }
}

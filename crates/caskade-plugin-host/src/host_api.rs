//! The `caskade:plugin/host` functions a backend imports. Everything is
//! scoped to the calling plugin: its log target, its key-value store, its
//! settings namespace and its events.

use crate::capabilities::library_allowed;
use crate::runtime::{
    State,
    caskade::plugin::{host, types},
};
use caskade_core::{bus::Event, model::ListGames};
use serde::Deserialize;

fn denied(message: &str) -> types::PluginError {
    types::PluginError {
        code: "permission_denied".into(),
        message: message.into(),
    }
}

fn failed(code: &str, error: impl std::fmt::Display) -> types::PluginError {
    types::PluginError {
        code: code.into(),
        message: error.to_string(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
enum Query {
    Games {
        #[serde(default)]
        query: Option<String>,
        #[serde(default)]
        limit: Option<u32>,
    },
    Game(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
enum Update {
    SetFavorite {
        #[serde(rename = "gameId")]
        game_id: String,
        favorite: bool,
    },
    SetTags {
        #[serde(rename = "gameId")]
        game_id: String,
        tags: Vec<String>,
    },
}

impl types::Host for State {}

impl host::Host for State {
    async fn log(&mut self, level: types::Level, message: String) {
        let plugin = self.plugin.as_str();
        // Plugins can't log megabytes at once.
        let message: String = message.chars().take(4096).collect();
        match level {
            types::Level::Trace => tracing::trace!(plugin, "{message}"),
            types::Level::Debug => tracing::debug!(plugin, "{message}"),
            types::Level::Info => tracing::info!(plugin, "{message}"),
            types::Level::Warn => tracing::warn!(plugin, "{message}"),
            types::Level::Error => tracing::error!(plugin, "{message}"),
        }
    }

    /// The language Caskade shows text in (#219), so a backend can answer
    /// in it. It can change between calls.
    async fn locale(&mut self) -> String {
        self.language.current()
    }

    /// `text` from the plugin's own catalog, in the language in use (#109).
    async fn message(&mut self, text: String, context: Option<String>) -> String {
        self.locales
            .message(&self.language.current(), context.as_deref(), &text)
    }

    /// `one` or `other` for `count`, from the plugin's catalog (#109).
    async fn plural(
        &mut self,
        one: String,
        other: String,
        count: u64,
        context: Option<String>,
    ) -> String {
        self.locales.plural(
            &self.language.current(),
            context.as_deref(),
            &one,
            &other,
            count,
        )
    }

    async fn kv_get(&mut self, key: String) -> Option<Vec<u8>> {
        match self.services.store.plugin_kv_get(&self.plugin, &key).await {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(plugin = %self.plugin, %error, "plugin storage read failed");
                None
            }
        }
    }

    async fn kv_set(&mut self, key: String, value: Vec<u8>) {
        // Bounds are part of the contract: an oversized write is dropped
        // and logged rather than trapping the plugin.
        if let Err(error) = self
            .services
            .store
            .plugin_kv_set(&self.plugin, &key, &value)
            .await
        {
            tracing::warn!(plugin = %self.plugin, %error, "plugin storage write refused");
        }
    }

    async fn kv_delete(&mut self, key: String) {
        if let Err(error) = self
            .services
            .store
            .plugin_kv_delete(&self.plugin, &key)
            .await
        {
            tracing::warn!(plugin = %self.plugin, %error, "plugin storage delete failed");
        }
    }

    async fn emit(&mut self, name: String, payload: String) {
        let valid = !name.is_empty()
            && name.len() <= 128
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            && payload.len() <= 64 * 1024
            && serde_json::from_str::<serde_json::Value>(&payload).is_ok();
        if !valid {
            tracing::warn!(plugin = %self.plugin, name, "plugin event dropped: bad name or payload");
            return;
        }
        if let Some(bus) = &self.services.bus {
            bus.emit(Event::PluginEvent {
                plugin: self.plugin.clone(),
                name,
                payload,
            });
        }
    }

    async fn library_query(&mut self, request: String) -> Result<String, types::PluginError> {
        if !library_allowed(self.grants.library, false) {
            return Err(denied(
                "reading the library needs the library: read permission",
            ));
        }
        let query: Query =
            serde_json::from_str(&request).map_err(|error| failed("bad_request", error))?;
        let store = &self.services.store;
        let answer = match query {
            Query::Games { query, limit } => {
                let page = store
                    .list_games(&ListGames {
                        query,
                        limit: limit.unwrap_or(50).min(500),
                        ..ListGames::default()
                    })
                    .await
                    .map_err(|error| failed("library", error))?;
                serde_json::to_string(&page)
            }
            Query::Game(id) => {
                let game = store
                    .game(&id)
                    .await
                    .map_err(|error| failed("library", error))?;
                serde_json::to_string(&game)
            }
        };
        answer.map_err(|error| failed("library", error))
    }

    async fn library_update(&mut self, request: String) -> Result<String, types::PluginError> {
        if !library_allowed(self.grants.library, true) {
            return Err(denied(
                "changing the library needs the library: write permission",
            ));
        }
        let update: Update =
            serde_json::from_str(&request).map_err(|error| failed("bad_request", error))?;
        let store = &self.services.store;
        let game_id = match &update {
            Update::SetFavorite { game_id, .. } | Update::SetTags { game_id, .. } => {
                game_id.clone()
            }
        };
        let mut game = store
            .game(&game_id)
            .await
            .map_err(|error| failed("library", error))?
            .ok_or_else(|| failed("not_found", format!("unknown game {game_id}")))?;
        match update {
            Update::SetFavorite { favorite, .. } => game.favorite = favorite,
            Update::SetTags { tags, .. } => game.tags = tags,
        }
        let game = store
            .upsert_game(&game)
            .await
            .map_err(|error| failed("library", error))?;
        if let Some(bus) = &self.services.bus {
            bus.emit(Event::LibraryGameUpdated {
                game_id: game.id.clone(),
            });
        }
        serde_json::to_string(&game).map_err(|error| failed("library", error))
    }

    async fn install_input_read(
        &mut self,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, types::PluginError> {
        use std::io::{Read, Seek, SeekFrom};
        const MAX: u32 = 1 << 20;
        let Some(path) = self.install_input.clone() else {
            return Err(denied(
                "there is no install input outside install-handler calls",
            ));
        };
        if length > MAX {
            return Err(failed("too_large", "read at most 1 MiB at a time"));
        }
        caskade_jobs::blocking("Reading plugin files", move || {
            let mut file = std::fs::File::open(&path)?;
            file.seek(SeekFrom::Start(offset))?;
            let mut bytes = Vec::with_capacity(length as usize);
            file.take(u64::from(length)).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        })
        .await
        .map_err(|error| failed("io", error))?
        .map_err(|error| failed("io", error.kind()))
    }

    async fn settings_get(&mut self) -> Option<String> {
        let settings = self.services.settings.as_ref()?;
        let snapshot = settings
            .get(&crate::settings_namespace(&self.plugin))
            .await
            .ok()?;
        serde_json::to_string(&snapshot.value).ok()
    }
}

/// The 0.1 world's host functions (#109): the same as 0.2's.
mod v0_1 {
    use super::State;
    use crate::runtime::caskade::plugin::{host::Host as Current, types};
    use crate::runtime::v0_1::caskade::plugin::{host, types as old};

    fn level(level: old::Level) -> types::Level {
        match level {
            old::Level::Trace => types::Level::Trace,
            old::Level::Debug => types::Level::Debug,
            old::Level::Info => types::Level::Info,
            old::Level::Warn => types::Level::Warn,
            old::Level::Error => types::Level::Error,
        }
    }

    fn error(error: types::PluginError) -> old::PluginError {
        old::PluginError {
            code: error.code,
            message: error.message,
        }
    }

    impl old::Host for State {}

    impl host::Host for State {
        async fn log(&mut self, at: old::Level, message: String) {
            Current::log(self, level(at), message).await;
        }
        async fn locale(&mut self) -> String {
            Current::locale(self).await
        }
        async fn kv_get(&mut self, key: String) -> Option<Vec<u8>> {
            Current::kv_get(self, key).await
        }
        async fn kv_set(&mut self, key: String, value: Vec<u8>) {
            Current::kv_set(self, key, value).await;
        }
        async fn kv_delete(&mut self, key: String) {
            Current::kv_delete(self, key).await;
        }
        async fn emit(&mut self, name: String, payload: String) {
            Current::emit(self, name, payload).await;
        }
        async fn settings_get(&mut self) -> Option<String> {
            Current::settings_get(self).await
        }
        async fn library_query(&mut self, request: String) -> Result<String, old::PluginError> {
            Current::library_query(self, request).await.map_err(error)
        }
        async fn library_update(&mut self, request: String) -> Result<String, old::PluginError> {
            Current::library_update(self, request).await.map_err(error)
        }
        async fn install_input_read(
            &mut self,
            offset: u64,
            length: u32,
        ) -> Result<Vec<u8>, old::PluginError> {
            Current::install_input_read(self, offset, length)
                .await
                .map_err(error)
        }
    }
}

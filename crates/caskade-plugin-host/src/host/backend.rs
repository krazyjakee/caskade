//! Running a plugin's backend: compiling it (through the component cache),
//! starting it lazily and making calls that drop the instance on failure.

use super::{Entry, HostError, Live, PluginHost, Result};
use crate::{
    capabilities::folder_grants,
    runtime::{CallError, GuestError, Instance, InterceptResult, Sandbox},
};
use std::path::{Path, PathBuf};
use wasmtime::component::Component;

/// The largest backend component read.
const BACKEND_MAX: u64 = 64 << 20;

impl PluginHost {
    /// Drops the plugin's running backend and compiled component and
    /// compiles the backend file again, so a broken rebuild reports its
    /// error now. A dev plugin turned off this session is turned back on.
    pub async fn reload_backend(&self, id: &str) -> Result<()> {
        caskade_jobs::global()
            .run("Reloading plugin", self.reload_backend_queued(id))
            .await
    }

    async fn reload_backend_queued(&self, id: &str) -> Result<()> {
        let entry = self.entry(id)?;
        if entry.dev && !entry.enabled {
            self.set_enabled(id, true).await?;
        }
        let entry = self.entry(id)?;
        let manifest = Self::valid(&entry, id)?;
        let mut live = entry.live.lock().await;
        live.instance = None;
        live.component = None;
        let Some(backend) = manifest.backend else {
            return Ok(());
        };
        live.component = Some(
            self.compile(id, &entry.folder.join(backend), &mut live)
                .await?,
        );
        live.last_error = None;
        Ok(())
    }

    /// Compiles (or loads from the cache) the backend at `path`; a compile
    /// error is kept as the plugin's last error.
    async fn compile(&self, id: &str, path: &Path, live: &mut Live) -> Result<Component> {
        let call_error = |source| HostError::Call {
            id: id.to_owned(),
            source,
        };
        let (runtime, path) = (self.runtime.clone(), path.to_owned());
        caskade_jobs::blocking("Preparing plugin", move || {
            let unreadable = |error: std::io::Error| CallError::Instantiate(error.to_string());
            if std::fs::metadata(&path).map_err(unreadable)?.len() > BACKEND_MAX {
                return Err(CallError::Instantiate(
                    "the backend is larger than 64 MiB".into(),
                ));
            }
            runtime.compile(&std::fs::read(&path).map_err(unreadable)?)
        })
        .await
        .map_err(|error| call_error(CallError::Instantiate(error.to_string())))?
        .map_err(|source| {
            live.last_error = Some(source.to_string());
            call_error(source)
        })
    }

    /// Keeps compiled backends in `dir` across app starts (#136). Entries are
    /// signed with a random per-install key kept in the app database, never
    /// in `dir`; an entry that fails verification is compiled again.
    pub async fn enable_component_cache(&self, dir: PathBuf) -> Result<()> {
        const KEY: &str = "plugins.component-cache-key";
        let stored = self
            .services
            .store
            .get_setting(KEY)
            .await?
            .and_then(|value| value.as_str().and_then(decode_key));
        let key = match stored {
            Some(key) => key,
            None => {
                let key = caskade_component_cache::CacheKey::generate()?;
                let text: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
                self.services
                    .store
                    .set_setting(KEY, &serde_json::Value::String(text))
                    .await?;
                key
            }
        };
        self.runtime
            .set_cache(caskade_component_cache::ComponentCache::new(dir, key));
        Ok(())
    }

    /// Backends loaded from the compiled-component cache vs compiled.
    pub fn compile_counts(&self) -> crate::CompileCounts {
        self.runtime.compile_counts()
    }

    /// Starts the backend if it isn't running.
    pub async fn activate(&self, id: &str) -> Result<()> {
        caskade_jobs::global()
            .run("Starting plugin", async {
                let entry = self.entry(id)?;
                let mut live = entry.live.lock().await;
                self.ensure(id, &entry, &mut live).await
            })
            .await
    }

    async fn ensure(&self, id: &str, entry: &Entry, live: &mut Live) -> Result<()> {
        if live.instance.is_some() {
            return Ok(());
        }
        let manifest = Self::valid(entry, id)?;
        if !entry.enabled {
            return Err(HostError::Disabled(id.to_owned()));
        }
        let backend = manifest
            .backend
            .as_ref()
            .ok_or_else(|| HostError::NoBackend(id.to_owned()))?;
        if live.component.is_none() {
            live.component = Some(self.compile(id, &entry.folder.join(backend), live).await?);
        }
        let grants = self.granted(id).await;
        let sandbox = Sandbox {
            folders: folder_grants(&grants.fs, std::env::home_dir().as_deref()),
            data: Some(self.data_folder(id)?),
            grants,
            language: self.language.clone(),
            locales: entry.locales.clone(),
        };
        let component = live.component.as_ref().expect("compiled above");
        let instance = caskade_jobs::global()
            .run(
                "Starting plugin",
                self.runtime
                    .instantiate(component, id, self.services.clone(), sandbox),
            )
            .await
            .map_err(|source| {
                live.failures += 1;
                live.last_error = Some(source.to_string());
                HostError::Call {
                    id: id.to_owned(),
                    source,
                }
            })?;
        tracing::info!(plugin = id, "plugin backend started");
        live.instance = Some(instance);
        Ok(())
    }

    /// Stops the backend; the next call starts it again.
    pub async fn deactivate(&self, id: &str) -> Result<()> {
        let entry = self.entry(id)?;
        entry.live.lock().await.instance = None;
        Ok(())
    }

    /// Runs one call on the plugin's backend, starting it if needed. A
    /// failed call drops the instance.
    pub(crate) async fn call<T>(
        &self,
        id: &str,
        run: impl AsyncFnOnce(&mut Instance) -> std::result::Result<T, CallError>,
    ) -> Result<T> {
        // Always take the resource slot before a plugin's instance mutex.
        // Hooks already own a slot; UI/backend calls use the same lock order.
        caskade_jobs::global()
            .run("Running plugin", async {
                let entry = self.entry(id)?;
                let mut live = entry.live.lock().await;
                self.ensure(id, &entry, &mut live).await?;
                let instance = live.instance.as_mut().expect("started above");
                match run(instance).await {
                    Ok(value) => Ok(value),
                    Err(source) => {
                        tracing::warn!(
                            plugin = id,
                            error = %source,
                            "plugin call failed; restarting it on the next call"
                        );
                        live.instance = None;
                        live.failures += 1;
                        live.last_error = Some(source.to_string());
                        Err(HostError::Call {
                            id: id.to_owned(),
                            source,
                        })
                    }
                }
            })
            .await
    }

    pub(crate) fn answered<T>(id: &str, answer: std::result::Result<T, GuestError>) -> Result<T> {
        answer.map_err(|error| HostError::Plugin {
            id: id.to_owned(),
            code: error.code,
            message: error.message,
        })
    }

    /// A call from the plugin's UI half (`ctx.backend.call`).
    pub async fn request(&self, id: &str, method: &str, payload: &str) -> Result<String> {
        let answer = self
            .call(id, async |instance| {
                instance.handle_request(method, payload).await
            })
            .await?;
        Self::answered(id, answer)
    }

    /// Delivers an event the plugin declared in `contributes.hooks`.
    pub async fn event(&self, id: &str, name: &str, payload: &str) -> Result<()> {
        self.call(id, async |instance| instance.on_event(name, payload).await)
            .await
    }

    /// Runs the plugin's step of an interceptor chain.
    pub async fn intercept(&self, id: &str, point: &str, payload: &str) -> Result<InterceptResult> {
        let answer = self
            .call(id, async |instance| {
                instance.intercept(point, payload).await
            })
            .await?;
        Self::answered(id, answer)
    }
}

/// A 32-byte cache key stored as 64 hex digits.
fn decode_key(text: &str) -> Option<caskade_component_cache::CacheKey> {
    if text.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(caskade_component_cache::CacheKey::from_bytes(key))
}

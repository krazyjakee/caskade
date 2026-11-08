//! The wasmtime side: one engine for every plugin, and
//! per instance a memory cap and a CPU time box on every call. WASI is
//! linked with nothing granted: no preopened folders, no network, no
//! environment, no inherited stdio.

use crate::{
    PluginServices,
    capabilities::{FolderGrant, NetworkHooks},
    manifest::Permissions,
    read, write,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap,
    component::{Component, Linker, ResourceTable},
};
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

/// What one plugin instance may reach (#124).
#[derive(Debug, Clone, Default)]
pub struct Sandbox {
    /// The permissions the user granted.
    pub grants: Permissions,
    /// The plugin's private data folder, visible to it as `/data`.
    pub data: Option<std::path::PathBuf>,
    /// Granted folders, visible at their own paths.
    pub folders: Vec<FolderGrant>,
    /// What `host.locale` answers (#219).
    pub language: crate::locales::Language,
}

/// Where a plugin sees its private data folder.
pub const DATA_MOUNT: &str = "/data";

wasmtime::component::bindgen!({
    path: "../../wit",
    world: "plugin",
    imports: { default: async },
    exports: { default: async },
    additional_derives: [PartialEq, Eq],
});

pub use caskade::plugin::types::{InterceptResult, PluginError as GuestError};

/// Limits for every backend call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Linear memory per instance.
    pub memory: usize,
    /// Wall time per call, CPU included.
    pub call_timeout: Duration,
    /// Wall time per call for a backend granted network access: its calls
    /// wait on remote servers, which `call_timeout` would cut off on a slow
    /// link.
    pub network_call_timeout: Duration,
}

impl Limits {
    /// The time box of each call into a backend with these grants.
    pub fn call_timeout_for(&self, grants: &Permissions) -> Duration {
        if grants.network.is_empty() {
            self.call_timeout
        } else {
            self.network_call_timeout.max(self.call_timeout)
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory: 64 << 20,
            call_timeout: Duration::from_secs(2),
            network_call_timeout: Duration::from_secs(30),
        }
    }
}

/// The store's resource caps. Each memory is capped at `limits.memory`, and
/// the number of memories, tables and table elements is capped too, so a
/// component can't declare many memories (multi-memory is on) to use
/// gigabytes within its time box (CSK-07).
fn store_limits(limits: &Limits) -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(limits.memory)
        .memories(2)
        .tables(8)
        .table_elements(100_000)
        .trap_on_grow_failure(true)
        .instances(16)
        .build()
}

/// Why a call failed. The instance is discarded after any of these.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum CallError {
    #[error("the plugin ran past its {0:?} time limit")]
    TimedOut(Duration),
    #[error("the plugin went over its memory limit")]
    MemoryLimit,
    #[error("the plugin crashed: {0}")]
    Trap(String),
    #[error("the plugin can't be started: {0}")]
    Instantiate(String),
}

/// How often the engine's epoch advances; time boxes are counted in it.
const TICK: Duration = Duration::from_millis(10);

/// Advances the engine's epoch until dropped.
struct Ticker {
    stop: Arc<AtomicBool>,
}

impl Ticker {
    fn start(engine: Engine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let running = stop.clone();
        std::thread::Builder::new()
            .name("caskade-plugin-epoch".into())
            .spawn(move || {
                while !running.load(Ordering::Relaxed) {
                    std::thread::sleep(TICK);
                    engine.increment_epoch();
                }
            })
            .expect("start the plugin epoch thread");
        Self { stop }
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// How often backends were loaded from the compiled-component cache (#136)
/// and how often they had to be compiled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompileCounts {
    pub cached: u32,
    pub compiled: u32,
}

/// The engine and linker shared by every plugin.
pub struct Runtime {
    engine: Engine,
    linker: Linker<State>,
    limits: Limits,
    /// Started by the first instance: with no plugin running, nothing has
    /// to wake 100 times a second.
    ticker: std::sync::OnceLock<Ticker>,
    cache: std::sync::RwLock<Option<caskade_component_cache::ComponentCache>>,
    cached: std::sync::atomic::AtomicU32,
    compiled: std::sync::atomic::AtomicU32,
}

impl Runtime {
    pub fn new(limits: Limits) -> Result<Self, CallError> {
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .epoch_interruption(true)
            .parallel_compilation(false);
        let engine = Engine::new(&config).map_err(|e| CallError::Instantiate(e.to_string()))?;
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)
            .map_err(|e| CallError::Instantiate(e.to_string()))?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)
            .map_err(|e| CallError::Instantiate(e.to_string()))?;
        Plugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| state)
            .map_err(|e| CallError::Instantiate(e.to_string()))?;
        Ok(Self {
            ticker: std::sync::OnceLock::new(),
            engine,
            linker,
            limits,
            cache: std::sync::RwLock::default(),
            cached: Default::default(),
            compiled: Default::default(),
        })
    }

    /// Keeps compiled components in a verified on-disk cache from now on.
    pub fn set_cache(&self, cache: caskade_component_cache::ComponentCache) {
        *write(&self.cache) = Some(cache);
    }

    pub fn compile_counts(&self) -> CompileCounts {
        CompileCounts {
            cached: self.cached.load(Ordering::Relaxed),
            compiled: self.compiled.load(Ordering::Relaxed),
        }
    }

    /// Compiles a backend component, or loads it from the cache when one is
    /// set and holds a verified entry; compile errors name what's wrong.
    pub fn compile(&self, bytes: &[u8]) -> Result<Component, CallError> {
        let cache = read(&self.cache).clone();
        let (component, hit) = match cache {
            Some(cache) => {
                cache
                    .load_or_compile(&self.engine, bytes)
                    .map(|(component, outcome)| {
                        (component, outcome == caskade_component_cache::Outcome::Hit)
                    })
            }
            None => Component::new(&self.engine, bytes).map(|component| (component, false)),
        }
        .map_err(|e| CallError::Instantiate(format!("{e:#}")))?;
        let counter = if hit { &self.cached } else { &self.compiled };
        counter.fetch_add(1, Ordering::Relaxed);
        Ok(component)
    }

    pub async fn instantiate(
        &self,
        component: &Component,
        plugin: &str,
        services: Arc<PluginServices>,
        sandbox: Sandbox,
    ) -> Result<Instance, CallError> {
        // No sockets ever: network access is wasi:http through the allowlist.
        let mut wasi = WasiCtx::builder();
        wasi.allow_tcp(false)
            .allow_udp(false)
            .allow_ip_name_lookup(false);
        let preopen = |error: wasmtime::Error| CallError::Instantiate(format!("{error:#}"));
        if let Some(data) = &sandbox.data {
            wasi.preopened_dir(data, DATA_MOUNT, FsPerms::ReadWrite)
                .map_err(preopen)?;
        }
        for folder in &sandbox.folders {
            let perms = if folder.read_only {
                FsPerms::ReadOnly
            } else {
                FsPerms::ReadWrite
            };
            wasi.preopened_dir(&folder.path, folder.path.to_string_lossy(), perms)
                .map_err(preopen)?;
        }
        let timeout = self.limits.call_timeout_for(&sandbox.grants);
        let deadline = deadline(timeout);
        let state = State {
            plugin: plugin.to_owned(),
            wasi: wasi.build(),
            http: WasiHttpCtx::new(),
            network: NetworkHooks::new(plugin, &sandbox.grants.network),
            grants: sandbox.grants,
            table: ResourceTable::new(),
            limits: store_limits(&self.limits),
            services,
            install_input: None,
            language: sandbox.language,
        };
        self.ticker
            .get_or_init(|| Ticker::start(self.engine.clone()));
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_epoch_deadline(deadline);
        store.epoch_deadline_trap();
        let started = match self.linker.instantiate_async(&mut store, component).await {
            Ok(instance) => Plugin::new(&mut store, &instance).map(|bindings| (instance, bindings)),
            Err(error) => Err(error),
        };
        let (instance, bindings) =
            started
                .map_err(|e| classify(e, timeout))
                .map_err(|e| match e {
                    CallError::Trap(message) => CallError::Instantiate(message),
                    other => other,
                })?;
        Ok(Instance {
            store,
            instance,
            bindings,
            timeout,
            deadline,
        })
    }
}

/// `timeout` in epoch ticks.
fn deadline(timeout: Duration) -> u64 {
    let ticks = timeout.as_millis() / TICK.as_millis();
    u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
}

fn classify(error: wasmtime::Error, timeout: Duration) -> CallError {
    if let Some(trap) = error.downcast_ref::<Trap>()
        && *trap == Trap::Interrupt
    {
        return CallError::TimedOut(timeout);
    }
    let text = format!("{error:#}");
    if text.contains("forcing trap when growing memory")
        || text.contains("memory") && text.contains("limit")
    {
        return CallError::MemoryLimit;
    }
    CallError::Trap(text)
}

/// The per-instance data host functions see.
pub struct State {
    pub(crate) plugin: String,
    pub(crate) grants: Permissions,
    wasi: WasiCtx,
    http: WasiHttpCtx,
    network: NetworkHooks,
    table: ResourceTable,
    limits: StoreLimits,
    pub(crate) services: Arc<PluginServices>,
    /// The input of the running `install-handler` call (#55).
    pub(crate) install_input: Option<std::path::PathBuf>,
    /// The language Caskade shows text in (#219).
    pub(crate) language: crate::locales::Language,
}

impl WasiHttpView for State {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.network,
        }
    }
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// One running backend.
pub struct Instance {
    store: Store<State>,
    instance: wasmtime::component::Instance,
    bindings: Plugin,
    timeout: Duration,
    deadline: u64,
}

impl Instance {
    /// Runs one export under the time box.
    async fn bounded<T>(
        &mut self,
        call: impl AsyncFnOnce(&Plugin, &mut Store<State>) -> wasmtime::Result<T>,
    ) -> Result<T, CallError> {
        self.store.set_epoch_deadline(self.deadline);
        // The epoch stops guest code; the outer timeout covers host awaits.
        let slack = self.timeout + Duration::from_millis(500);
        match tokio::time::timeout(slack, call(&self.bindings, &mut self.store)).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(classify(error, self.timeout)),
            Err(_) => Err(CallError::TimedOut(self.timeout)),
        }
    }

    /// Lets `host.install-input-read` read `input` until it's cleared.
    pub fn set_install_input(&mut self, input: Option<std::path::PathBuf>) {
        self.store.data_mut().install_input = input;
    }

    /// Calls `func` of the provider `interface` (e.g. `library-source`), or
    /// `None` when the component doesn't export that provider (#126).
    pub async fn provider<P, R>(
        &mut self,
        interface: &str,
        func: &str,
        params: P,
    ) -> Result<Option<R>, CallError>
    where
        P: wasmtime::component::ComponentNamedList
            + wasmtime::component::Lower
            + Send
            + Sync
            + 'static,
        R: wasmtime::component::ComponentNamedList
            + wasmtime::component::Lift
            + Send
            + Sync
            + 'static,
    {
        let name = format!("caskade:plugin/{interface}@{}", crate::HOST_API);
        let Some(export) = self.instance.get_export_index(&mut self.store, None, &name) else {
            return Ok(None);
        };
        let Some(export) = self
            .instance
            .get_export_index(&mut self.store, Some(&export), func)
        else {
            return Ok(None);
        };
        let typed = self
            .instance
            .get_typed_func::<P, R>(&mut self.store, &export)
            .map_err(|error| {
                CallError::Trap(format!("{interface}.{func} has the wrong type: {error}"))
            })?;
        self.bounded(async |_, store| typed.call_async(store, params).await)
            .await
            .map(Some)
    }

    pub async fn handle_request(
        &mut self,
        method: &str,
        payload: &str,
    ) -> Result<Result<String, GuestError>, CallError> {
        self.bounded(async |bindings, store| {
            bindings
                .caskade_plugin_requests()
                .call_handle_request(store, method, payload)
                .await
        })
        .await
    }

    pub async fn on_event(&mut self, name: &str, payload: &str) -> Result<(), CallError> {
        self.bounded(async |bindings, store| {
            bindings
                .caskade_plugin_hooks()
                .call_on_event(store, name, payload)
                .await
        })
        .await
    }

    pub async fn intercept(
        &mut self,
        point: &str,
        payload: &str,
    ) -> Result<Result<InterceptResult, GuestError>, CallError> {
        self.bounded(async |bindings, store| {
            bindings
                .caskade_plugin_hooks()
                .call_intercept(store, point, payload)
                .await
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_backends_that_go_online_get_the_network_budget() {
        let limits = Limits::default();
        let offline = Permissions::default();
        let online = Permissions {
            network: vec!["example.com".into()],
            ..Permissions::default()
        };
        assert_eq!(limits.call_timeout_for(&offline), Duration::from_secs(2));
        assert_eq!(limits.call_timeout_for(&online), Duration::from_secs(30));
        // The network budget never shortens the default one.
        let short = Limits {
            network_call_timeout: Duration::from_millis(1),
            ..Limits::default()
        };
        assert_eq!(short.call_timeout_for(&online), Duration::from_secs(2));
        assert_eq!(deadline(Duration::from_secs(30)), 3000);
    }

    #[test]
    fn stores_cap_memories_and_tables_not_only_their_size() {
        use wasmtime::ResourceLimiter;
        let mut limits = store_limits(&Limits::default());
        assert_eq!(limits.memories(), 2);
        assert_eq!(limits.tables(), 8);
        assert_eq!(limits.instances(), 16);
        assert!(limits.table_growing(0, 100_000, None).unwrap());
        assert!(limits.table_growing(0, 100_001, None).is_err());
        assert!(limits.memory_growing(0, 64 << 20, None).unwrap());
        assert!(limits.memory_growing(0, (64 << 20) + 1, None).is_err());
    }
}

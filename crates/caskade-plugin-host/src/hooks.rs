//! Backend plugins on the hook bus (#125). A plugin's
//! `on-event` and `intercept` exports run like built-in subscribers and
//! interceptors: ordered, time-boxed and isolated by the #109 bus.
//!
//! - **Events** named in `contributes.hooks` reach `on-event` as
//!   `{"version": 1, "data": …}`, where `data` is the event's JSON (the
//!   `value` of [`Event`]'s serde form). A plugin whose backend isn't running
//!   only receives them when its `activation` lists `onEvent:<name>` (or
//!   `onStartup`); the first such event starts it.
//! - **Interceptors** for the points in [`POINTS`] run the plugin's
//!   `intercept` export over the payload's JSON. A `launch.before` step that
//!   changes what runs needs the `process` permission, and `library.import`
//!   needs `library: write`; a refused rewrite is skipped and counts as a
//!   failure. `install.after` needs `process` or an `fs` grant, since a
//!   plugin without either can't change an installed game (#55).
//! - `onStartup` plugins start when attached, `onView:<id>` ones when the
//!   UI opens that view ([`PluginHooks::view_opened`]).
//! - **Circuit breaker:** [`FAILURE_LIMIT`] consecutive traps, timeouts,
//!   limit hits or refused rewrites disable the plugin and emit
//!   [`Event::PluginDisabled`].

use crate::{
    HostError, PluginHost, PluginStatus,
    capabilities::{import_rewrite_allowed, launch_rewrite_allowed, library_allowed},
    lock,
    manifest::Permissions,
    runtime::InterceptResult,
    settings::SETTINGS_CHANGED,
};
use async_trait::async_trait;
use caskade_core::{
    bus::{
        Bus, Event, EventFilter, EventHandler, Intercept, InterceptPoint, Interceptor,
        LibraryImport, SubscriptionId,
    },
    install::hooks::{InstallAfter, InstallMoved, InstallPlanned},
    launch::{LaunchAfter, LaunchBefore},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    marker::PhantomData,
    sync::{Arc, Mutex, Weak},
};

/// Interceptor points a plugin can declare in `contributes.hooks`.
pub const POINTS: [&str; 6] = [
    LaunchBefore::NAME,
    LaunchAfter::NAME,
    LibraryImport::NAME,
    InstallPlanned::NAME,
    InstallAfter::NAME,
    InstallMoved::NAME,
];

/// Consecutive failures that disable a plugin.
pub(crate) const FAILURE_LIMIT: u32 = 3;

/// The plugins currently on the bus.
pub struct PluginHooks {
    host: PluginHost,
    bus: Bus,
    attached: Mutex<BTreeMap<String, SubscriptionId>>,
    /// One sync at a time, so a plugin is never attached twice.
    syncing: tokio::sync::Mutex<()>,
    failures: Mutex<BTreeMap<String, u32>>,
    this: Weak<PluginHooks>,
}

/// The bus id of a plugin's subscriptions and interceptors.
fn bus_id(plugin: &str) -> String {
    format!("plugin:{plugin}")
}

impl PluginHooks {
    pub fn new(host: PluginHost, bus: Bus) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            host,
            bus,
            attached: Mutex::default(),
            syncing: tokio::sync::Mutex::default(),
            failures: Mutex::default(),
            this: this.clone(),
        })
        .listening()
    }

    /// Re-syncs whenever the host reports a plugin enabled, disabled,
    /// installed or removed.
    fn listening(self: Arc<Self>) -> Arc<Self> {
        self.bus.subscribe(
            "plugin-hooks",
            EventFilter::Names(vec!["plugins.changed"]),
            Arc::new(Resync(Arc::downgrade(&self))),
        );
        self
    }

    /// Puts every enabled plugin on the bus and takes every other one off;
    /// call after discovery and after enabling, disabling or installing.
    /// Plugins already attached are attached again, so a changed manifest
    /// takes effect.
    pub async fn sync(&self) {
        let _one = self.syncing.lock().await;
        let enabled: Vec<String> = self
            .host
            .list()
            .await
            .into_iter()
            .filter(|plugin| matches!(plugin.status, PluginStatus::Enabled | PluginStatus::Active))
            .map(|plugin| plugin.id)
            .collect();
        let attached: Vec<String> = lock(&self.attached).keys().cloned().collect();
        for id in attached {
            self.detach(&id);
        }
        for id in enabled {
            self.attach(&id).await;
        }
    }

    /// Whether the plugin's hooks are on the bus.
    pub fn attached(&self, id: &str) -> bool {
        lock(&self.attached).contains_key(id)
    }

    async fn attach(&self, id: &str) {
        let Some(manifest) = self.host.manifest(id) else {
            return;
        };
        let grants = self.host.granted(id).await;
        let hooks = &manifest.contributes.hooks;
        let events: Vec<String> = hooks
            .iter()
            .filter(|name| !POINTS.contains(&name.as_str()))
            .cloned()
            .collect();
        let subscription = self.bus.subscribe(
            bus_id(id),
            EventFilter::All,
            Arc::new(PluginEvents {
                hooks: self.this.clone(),
                plugin: id.to_owned(),
                names: events,
                activation: manifest.activation.clone(),
            }),
        );
        lock(&self.attached).insert(id.to_owned(), subscription);
        for point in hooks.iter().filter(|name| POINTS.contains(&name.as_str())) {
            match point.as_str() {
                LaunchBefore::NAME => {
                    self.step::<LaunchBefore>(id, &grants, |grants, before, after| {
                        launch_rewrite_allowed(grants.process, before, after)
                    })
                }
                LaunchAfter::NAME => self.step::<LaunchAfter>(id, &grants, |_, _, _| Ok(())),
                LibraryImport::NAME if library_allowed(grants.library, true) => self
                    .step::<LibraryImport>(id, &grants, |grants, before, after| {
                        import_rewrite_allowed(grants.process, before, after)
                    }),
                LibraryImport::NAME => tracing::warn!(
                    plugin = id,
                    "library.import needs `library: write`; interceptor not registered"
                ),
                // The installer only accepts reordered, dropped or selected
                // candidates, whoever intercepts.
                InstallPlanned::NAME => self.step::<InstallPlanned>(id, &grants, |_, _, _| Ok(())),
                InstallAfter::NAME if grants.process || !grants.fs.is_empty() => {
                    self.step::<InstallAfter>(id, &grants, |_, _, _| Ok(()))
                }
                InstallAfter::NAME => tracing::warn!(
                    plugin = id,
                    "install.after needs `process` or an `fs` grant; interceptor not registered"
                ),
                InstallMoved::NAME => self.step::<InstallMoved>(id, &grants, |_, _, _| Ok(())),
                _ => {}
            }
        }
        if manifest.activation.iter().any(|event| event == "onStartup") {
            self.start(id).await;
        }
        tracing::debug!(plugin = id, "plugin hooks attached");
    }

    fn step<P>(&self, id: &str, grants: &Permissions, check: Check)
    where
        P: InterceptPoint,
        P::Payload: Serialize + DeserializeOwned,
    {
        self.bus.intercept::<P>(
            bus_id(id),
            0,
            Arc::new(PluginStep::<P> {
                hooks: self.this.clone(),
                plugin: id.to_owned(),
                grants: grants.clone(),
                check,
                point: PhantomData,
            }),
        );
    }

    /// Takes a plugin's subscriptions and interceptors off the bus.
    pub fn detach(&self, id: &str) {
        if let Some(subscription) = lock(&self.attached).remove(id) {
            self.bus.unsubscribe(subscription);
        }
        self.bus.remove_interceptors(&bus_id(id));
        lock(&self.failures).remove(id);
    }

    /// The UI opened `view` of `plugin`: starts its backend when the
    /// manifest lists `onView:<view>`.
    pub async fn view_opened(&self, plugin: &str, view: &str) {
        let event = format!("onView:{view}");
        if self.attached(plugin)
            && !self.host.is_active(plugin)
            && self
                .host
                .manifest(plugin)
                .is_some_and(|manifest| manifest.activation.contains(&event))
        {
            self.start(plugin).await;
        }
    }

    async fn start(&self, id: &str) {
        let result = self.host.activate(id).await;
        self.record(id, result.as_ref().map(|_| ())).await;
    }

    /// Counts consecutive crashes; a success resets the count.
    async fn record(&self, id: &str, result: Result<(), &HostError>) {
        let failed = match result {
            Ok(()) => false,
            // The plugin answered with its own error: it is working.
            Err(HostError::Plugin { .. }) => false,
            Err(_) => true,
        };
        self.count(id, failed, result.err().map(ToString::to_string))
            .await;
    }

    async fn count(&self, id: &str, failed: bool, reason: Option<String>) {
        let tripped = {
            let mut failures = lock(&self.failures);
            if !failed {
                failures.remove(id);
                return;
            }
            let count = failures.entry(id.to_owned()).or_default();
            *count += 1;
            *count >= FAILURE_LIMIT
        };
        if !tripped {
            return;
        }
        let reason = format!(
            "stopped after {FAILURE_LIMIT} failures in a row: {}",
            reason.unwrap_or_default()
        );
        tracing::warn!(plugin = id, %reason, "plugin disabled by its circuit breaker");
        self.detach(id);
        if let Err(error) = self.host.set_enabled(id, false).await {
            tracing::warn!(plugin = id, %error, "could not disable the failing plugin");
        }
        self.bus.emit(Event::PluginDisabled {
            plugin: id.to_owned(),
            reason,
        });
    }
}

struct Resync(Weak<PluginHooks>);

#[async_trait]
impl EventHandler for Resync {
    async fn on_event(&self, _event: &Event) {
        if let Some(hooks) = self.0.upgrade() {
            // Outside the subscriber's time box: starting a plugin may compile it.
            tokio::spawn(async move { hooks.sync().await });
        }
    }
}

/// Delivers the bus events a plugin declared.
struct PluginEvents {
    hooks: Weak<PluginHooks>,
    plugin: String,
    names: Vec<String>,
    activation: Vec<String>,
}

impl PluginEvents {
    /// Whether `event` starts the plugin when its backend isn't running.
    fn wakes(&self, event: &str) -> bool {
        self.activation
            .iter()
            .any(|listed| listed == "onStartup" || *listed == format!("onEvent:{event}"))
    }
}

#[async_trait]
impl EventHandler for PluginEvents {
    fn accepts(&self, event: &Event) -> bool {
        let name = event.name();
        // A plugin's own settings changes come from the settings service
        // (#128) with their values; the bus event covers every namespace.
        name != SETTINGS_CHANGED
            && self.names.iter().any(|wanted| wanted == name)
            // A plugin's own events don't come back to it.
            && !matches!(event, Event::PluginEvent { plugin, .. } if *plugin == self.plugin)
    }

    async fn prepare(&self, event: &Event) -> Result<(), String> {
        let hooks = self.hooks.upgrade().ok_or("plugin host stopped")?;
        if (self.wakes(event.name()) || hooks.host.is_active(&self.plugin))
            && let Err(error) = hooks.host.activate(&self.plugin).await
        {
            hooks.record(&self.plugin, Err(&error)).await;
            return Err(error.to_string());
        }
        Ok(())
    }

    /// Only called for events [`Self::accepts`] let through.
    async fn on_event(&self, event: &Event) {
        let name = event.name();
        let Some(hooks) = self.hooks.upgrade() else {
            return;
        };
        if !self.wakes(name) && !hooks.host.is_active(&self.plugin) {
            return;
        }
        let data = serde_json::to_value(event)
            .ok()
            .and_then(|value| value.get("value").cloned())
            .unwrap_or(serde_json::Value::Null);
        let payload = serde_json::json!({"version": 1, "data": data}).to_string();
        let plugin = self.plugin.clone();
        // Its own task, so the bus's time box can't cancel the call midway:
        // a slow handler is still counted and can trip the circuit breaker.
        let delivery = tokio::spawn(caskade_jobs::inherit_async(async move {
            let result = hooks.host.event(&plugin, name, &payload).await;
            if let Err(error) = &result {
                tracing::warn!(plugin = %plugin, event = name, %error, "plugin event handler failed");
            }
            hooks.record(&plugin, result.as_ref().map(|_| ())).await;
        }));
        let _ = delivery.await;
    }
}

/// What a step may change, given the plugin's grants: `(before, after)`.
type Check = fn(&Permissions, &serde_json::Value, &serde_json::Value) -> Result<(), String>;

/// One plugin's step in point `P`'s chain.
struct PluginStep<P> {
    hooks: Weak<PluginHooks>,
    plugin: String,
    grants: Permissions,
    check: Check,
    point: PhantomData<fn() -> P>,
}

#[async_trait]
impl<P> Interceptor<P> for PluginStep<P>
where
    P: InterceptPoint,
    P::Payload: Serialize + DeserializeOwned,
{
    async fn prepare(&self) -> Result<(), String> {
        let hooks = self.hooks.upgrade().ok_or("plugin host stopped")?;
        if let Err(error) = hooks.host.activate(&self.plugin).await {
            hooks.record(&self.plugin, Err(&error)).await;
            return Err(error.to_string());
        }
        Ok(())
    }

    async fn intercept(&self, payload: P::Payload) -> Result<Intercept<P::Payload>, String> {
        let hooks = self.hooks.upgrade().ok_or("plugin host stopped")?;
        let (plugin, grants, check) = (self.plugin.clone(), self.grants.clone(), self.check);
        // Its own task: when the bus's time box gives up on this step first,
        // the call still finishes, is counted, and can trip the circuit
        // breaker (the host's own time limit ends it).
        tokio::spawn(caskade_jobs::inherit_async(async move {
            step::<P>(&hooks, &plugin, &grants, check, payload).await
        }))
        .await
        .map_err(|error| error.to_string())?
    }
}

/// One interceptor call and its outcome for the circuit breaker.
async fn step<P>(
    hooks: &PluginHooks,
    plugin: &str,
    grants: &Permissions,
    check: Check,
    payload: P::Payload,
) -> Result<Intercept<P::Payload>, String>
where
    P: InterceptPoint,
    P::Payload: Serialize + DeserializeOwned,
{
    let before = serde_json::to_value(&payload).map_err(|error| error.to_string())?;
    let answer = hooks
        .host
        .intercept(plugin, P::NAME, &before.to_string())
        .await;
    let decided = match answer {
        Ok(InterceptResult::Cancel(reason)) => Ok(Intercept::Cancel(reason)),
        Ok(InterceptResult::Continue(text)) => serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|error| format!("the plugin returned invalid JSON: {error}"))
            .and_then(|after| {
                check(grants, &before, &after)?;
                serde_json::from_value(after)
                    .map(Intercept::Continue)
                    .map_err(|error| format!("the plugin returned a bad {}: {error}", P::NAME))
            }),
        Err(error) => {
            let message = error.to_string();
            hooks.record(plugin, Err(&error)).await;
            return Err(message);
        }
    };
    match &decided {
        Ok(_) => hooks.count(plugin, false, None).await,
        Err(reason) => hooks.count(plugin, true, Some(reason.clone())).await,
    }
    decided
}

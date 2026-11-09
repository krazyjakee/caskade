//! The in-process hook bus: typed events,
//! ordered interceptors and provider registries. Built-in code and WASM
//! plugins (adapted by `caskade-plugin-host`) use the same contracts.
//!
//! Subscribers and interceptors enter the shared resource queue before their
//! execution time box starts. A slow, failing or panicking one cannot break
//! the core, and waiting for resources never counts as a plugin failure.

mod events;
mod intercept;
mod providers;

pub use events::{Event, EventFilter, GameSession, NewUnlocks, SourceCount};
pub use intercept::{Intercept, InterceptPoint, InterceptReport, Interceptor, Outcome};
pub use providers::{
    DownloadRequest, InstallHandler, InstallInput, LibrarySource, PLUGIN_SOURCE_PREFIX, UrlResolver,
};

use crate::{read, write};
use async_trait::async_trait;
use caskade_discovery::DetectedGame;
use std::{
    any::TypeId,
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::broadcast;

/// `library.import`: every discovered entry before it is stored. An
/// interceptor may rewrite it (rename, fix ids) or cancel it to drop it.
pub struct LibraryImport;

impl InterceptPoint for LibraryImport {
    type Payload = DetectedGame;
    const NAME: &'static str = "library.import";
}

/// Observes events.
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// Dynamic filters are evaluated before queueing, so irrelevant events
    /// consume neither a resource slot nor an activity notification.
    fn accepts(&self, _event: &Event) -> bool {
        true
    }
    /// Load a cold backend before starting its execution time box.
    async fn prepare(&self, _event: &Event) -> Result<(), String> {
        Ok(())
    }
    async fn on_event(&self, event: &Event);
}

pub(super) const PREPARATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Time boxes and buffer sizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusOptions {
    pub subscriber_timeout: Duration,
    pub interceptor_timeout: Duration,
    /// Events buffered for slow stream listeners (the UI bridge).
    pub stream_capacity: usize,
}

impl Default for BusOptions {
    fn default() -> Self {
        Self {
            subscriber_timeout: Duration::from_secs(2),
            interceptor_timeout: Duration::from_secs(2),
            stream_capacity: 256,
        }
    }
}

/// Identifies a subscription for `unsubscribe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

struct Subscriber {
    id: SubscriptionId,
    name: String,
    filter: EventFilter,
    handler: Arc<dyn EventHandler>,
}

/// Who handled an event, for `emit_and_wait`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryReport {
    pub delivered: Vec<String>,
    /// `(subscriber, why)` for failed, panicked or timed-out handlers.
    pub failed: Vec<(String, String)>,
}

#[derive(Default)]
struct Registries {
    library_sources: Vec<Arc<dyn LibrarySource>>,
    install_handlers: Vec<Arc<dyn InstallHandler>>,
    url_resolvers: Vec<Arc<dyn UrlResolver>>,
}

struct Inner {
    options: BusOptions,
    next: AtomicU64,
    subscribers: RwLock<Vec<Subscriber>>,
    chains: RwLock<HashMap<TypeId, Box<dyn intercept::AnyChain>>>,
    providers: RwLock<Registries>,
    stream: broadcast::Sender<Event>,
}

/// The hook bus; cheap to clone, shared by every core service.
#[derive(Clone)]
pub struct Bus {
    inner: Arc<Inner>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new(BusOptions::default())
    }
}

impl std::fmt::Debug for Bus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bus")
            .field("options", &self.inner.options)
            .finish_non_exhaustive()
    }
}

impl Bus {
    pub fn new(options: BusOptions) -> Self {
        let (stream, _) = broadcast::channel(options.stream_capacity.max(1));
        Self {
            inner: Arc::new(Inner {
                options,
                next: AtomicU64::new(0),
                subscribers: RwLock::default(),
                chains: RwLock::default(),
                providers: RwLock::default(),
                stream,
            }),
        }
    }

    fn next(&self) -> u64 {
        self.inner.next.fetch_add(1, Ordering::Relaxed)
    }

    // Events.

    pub fn subscribe(
        &self,
        name: impl Into<String>,
        filter: EventFilter,
        handler: Arc<dyn EventHandler>,
    ) -> SubscriptionId {
        let id = SubscriptionId(self.next());
        write(&self.inner.subscribers).push(Subscriber {
            id,
            name: name.into(),
            filter,
            handler,
        });
        id
    }

    pub fn unsubscribe(&self, id: SubscriptionId) {
        write(&self.inner.subscribers).retain(|subscriber| subscriber.id != id);
    }

    /// Every event as a stream (the UI bridge). A listener that falls more
    /// than `stream_capacity` events behind skips the oldest.
    pub fn stream(&self) -> broadcast::Receiver<Event> {
        self.inner.stream.subscribe()
    }

    fn deliveries(
        &self,
        event: &Event,
        inherit: bool,
    ) -> Vec<(String, tokio::task::JoinHandle<Result<(), String>>)> {
        let _ = self.inner.stream.send(event.clone());
        let timeout = self.inner.options.subscriber_timeout;
        read(&self.inner.subscribers)
            .iter()
            .filter(|subscriber| {
                subscriber.filter.matches(event) && subscriber.handler.accepts(event)
            })
            .map(|subscriber| {
                let (handler, event) = (subscriber.handler.clone(), event.clone());
                let delivery = async move {
                    caskade_jobs::global()
                        .run(crate::tr!("Updating plugins"), async move {
                            tokio::time::timeout(PREPARATION_TIMEOUT, handler.prepare(&event))
                                .await
                                .map_err(|_| {
                                    "background hook preparation timed out".to_owned()
                                })??;
                            tokio::time::timeout(timeout, handler.on_event(&event))
                                .await
                                .map_err(|_| format!("timed out after {} ms", timeout.as_millis()))
                        })
                        .await
                };
                let task = if inherit {
                    tokio::spawn(caskade_jobs::inherit_async(delivery))
                } else {
                    tokio::spawn(delivery)
                };
                (subscriber.name.clone(), task)
            })
            .collect()
    }

    /// Delivers `event` to the stream and to every matching subscriber, each
    /// in its own time-boxed task, without waiting. Must be called inside a
    /// tokio runtime.
    pub fn emit(&self, event: Event) {
        let name = event.name();
        for (subscriber, task) in self.deliveries(&event, false) {
            tokio::spawn(async move {
                match task.await {
                    Ok(Ok(())) => {}
                    Ok(Err(why)) => {
                        tracing::warn!(event = name, %subscriber, %why, "event handler failed")
                    }
                    Err(_) => tracing::warn!(event = name, %subscriber, "event handler panicked"),
                }
            });
        }
    }

    /// [`emit`](Self::emit), then waits for every handler (or its time box).
    pub async fn emit_and_wait(&self, event: Event) -> DeliveryReport {
        let mut report = DeliveryReport::default();
        for (subscriber, task) in self.deliveries(&event, true) {
            match task.await {
                Ok(Ok(())) => report.delivered.push(subscriber),
                Ok(Err(why)) => report.failed.push((subscriber, why)),
                Err(_) => report.failed.push((subscriber, "panicked".into())),
            }
        }
        report
    }

    // Interceptors.

    /// Adds an interceptor to point `P`. Lower `priority` runs first; equal
    /// priorities run in registration order.
    pub fn intercept<P: InterceptPoint>(
        &self,
        id: impl Into<String>,
        priority: i32,
        interceptor: Arc<dyn Interceptor<P>>,
    ) {
        let seq = self.next();
        let mut chains = write(&self.inner.chains);
        let chain = chains
            .entry(TypeId::of::<P>())
            .or_insert_with(|| Box::new(intercept::Chain::<P> { steps: Vec::new() }));
        intercept::downcast::<P>(chain).insert(intercept::Registration {
            id: id.into(),
            priority,
            seq,
            interceptor,
        });
    }

    /// Removes every interceptor registered as `id`, on every point (e.g. a
    /// plugin that was disabled). Returns how many steps were removed.
    pub fn remove_interceptors(&self, id: &str) -> usize {
        write(&self.inner.chains)
            .values_mut()
            .map(|chain| chain.remove(id))
            .sum()
    }

    /// Runs point `P`'s chain over `payload`.
    pub async fn run<P: InterceptPoint>(&self, payload: P::Payload) -> InterceptReport<P::Payload> {
        let steps = {
            let mut chains = write(&self.inner.chains);
            chains
                .get_mut(&TypeId::of::<P>())
                .map(|chain| intercept::downcast::<P>(chain).snapshot())
                .unwrap_or_default()
        };
        intercept::run::<P>(steps, payload, self.inner.options.interceptor_timeout).await
    }

    // Providers.

    pub fn register_library_source(&self, source: Arc<dyn LibrarySource>) {
        write(&self.inner.providers).library_sources.push(source);
    }

    pub fn register_install_handler(&self, handler: Arc<dyn InstallHandler>) {
        write(&self.inner.providers).install_handlers.push(handler);
    }

    pub fn register_url_resolver(&self, resolver: Arc<dyn UrlResolver>) {
        write(&self.inner.providers).url_resolvers.push(resolver);
    }

    pub fn library_sources(&self) -> Vec<Arc<dyn LibrarySource>> {
        read(&self.inner.providers).library_sources.clone()
    }

    pub fn install_handlers(&self) -> Vec<Arc<dyn InstallHandler>> {
        read(&self.inner.providers).install_handlers.clone()
    }

    pub fn url_resolvers(&self) -> Vec<Arc<dyn UrlResolver>> {
        read(&self.inner.providers).url_resolvers.clone()
    }
}

#[cfg(test)]
mod tests;

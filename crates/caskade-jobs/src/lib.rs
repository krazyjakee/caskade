//! One FIFO resource queue shared by discovery, installation, artwork and plugins.
//! Heavy CPU/disk stages run one at a time, including on single-core hosts.
//! Waiting async jobs consume no blocking threads. A detached blocking worker
//! retains its slot until it actually stops; cancellation cannot oversubscribe it.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    sync::{Arc, Condvar, Mutex, OnceLock},
};
use tokio::sync::{Notify, watch};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: u32,
    pub label: String,
    pub queued: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub revision: u32,
    pub tasks: Vec<Task>,
}

#[derive(Default)]
struct State {
    next_id: u32,
    active: Option<u32>,
    waiting: VecDeque<u32>,
    snapshot: Snapshot,
}

struct Inner {
    state: Mutex<State>,
    changed: Notify,
    ready: Condvar,
    updates: watch::Sender<Snapshot>,
}

#[derive(Clone)]
pub struct Queue(Arc<Inner>);

impl Default for Queue {
    fn default() -> Self {
        Self(Arc::new(Inner {
            state: Mutex::default(),
            changed: Notify::new(),
            ready: Condvar::new(),
            updates: watch::channel(Snapshot::default()).0,
        }))
    }
}

/// An activity's lifetime, also used for queued and active resource slots.
/// Dropping a waiting ticket removes it without ever starting its work.
pub struct Activity {
    queue: Queue,
    id: u32,
}

impl Activity {
    /// Relabel the activity in place, e.g. to show its progress.
    pub fn set_label(&self, label: impl Into<String>) {
        let label = label.into();
        let mut state = self.queue.0.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(task) = state
            .snapshot
            .tasks
            .iter_mut()
            .find(|task| task.id == self.id)
        else {
            return;
        };
        if task.label == label {
            return;
        }
        task.label = label;
        self.queue.publish(&mut state);
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        let mut state = self.queue.0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.snapshot.tasks.retain(|task| task.id != self.id);
        state.waiting.retain(|id| *id != self.id);
        if state.active == Some(self.id) {
            state.active = None;
        }
        self.queue.publish(&mut state);
        self.queue.0.changed.notify_waiters();
        self.queue.0.ready.notify_all();
    }
}

tokio::task_local! { static CURRENT: Arc<Activity>; }
thread_local! { static BLOCKING: RefCell<Option<Arc<Activity>>> = const { RefCell::new(None) }; }

impl Queue {
    pub fn snapshot(&self) -> Snapshot {
        self.0.updates.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.0.updates.subscribe()
    }

    fn publish(&self, state: &mut State) {
        state.snapshot.revision = state.snapshot.revision.wrapping_add(1);
        self.0.updates.send_replace(state.snapshot.clone());
    }

    fn ticket(&self, label: impl Into<String>, queued: bool) -> Activity {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.next_id = state.next_id.wrapping_add(1);
        let id = state.next_id;
        state.snapshot.tasks.push(Task {
            id,
            label: label.into(),
            queued,
        });
        if queued {
            state.waiting.push_back(id);
        }
        self.publish(&mut state);
        Activity {
            queue: self.clone(),
            id,
        }
    }

    /// Report an orchestration or a download already limited by its own queue.
    /// CPU/disk work within it must still use `blocking`, `run` or `sync`.
    pub fn activity(&self, label: impl Into<String>) -> Activity {
        self.ticket(label, false)
    }

    fn start(&self, state: &mut State, id: u32) -> bool {
        if state.active.is_some() || state.waiting.front() != Some(&id) {
            return false;
        }
        state.waiting.pop_front();
        state.active = Some(id);
        if let Some(task) = state.snapshot.tasks.iter_mut().find(|task| task.id == id) {
            task.queued = false;
        }
        self.publish(state);
        true
    }

    async fn acquire(&self, label: impl Into<String>) -> Arc<Activity> {
        let ticket = Arc::new(self.ticket(label, true));
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
                if self.start(&mut state, ticket.id) {
                    return ticket;
                }
            }
            changed.await;
        }
    }

    fn current(&self) -> Option<Arc<Activity>> {
        CURRENT
            .try_with(Arc::clone)
            .ok()
            .or_else(|| BLOCKING.with(|current| current.borrow().clone()))
            .filter(|ticket| Arc::ptr_eq(&ticket.queue.0, &self.0))
    }

    /// Queue an async stage. Nested stages reuse the caller's resource slot.
    pub async fn run<T>(&self, label: impl Into<String>, future: impl Future<Output = T>) -> T {
        let ticket = match self.current() {
            Some(ticket) => ticket,
            None => self.acquire(label).await,
        };
        CURRENT.scope(ticket, future).await
    }

    /// Queue blocking work before asking Tokio for a thread. Its permit lives
    /// inside the worker, so aborting the async caller never releases it early.
    pub async fn blocking<T, F>(
        &self,
        label: impl Into<String>,
        work: F,
    ) -> Result<T, tokio::task::JoinError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let ticket = match self.current() {
            Some(ticket) => ticket,
            None => self.acquire(label).await,
        };
        tokio::task::spawn_blocking(move || in_blocking_scope(ticket, work)).await
    }

    /// The same queue for dedicated synchronous watcher threads.
    pub fn sync<T>(&self, label: impl Into<String>, work: impl FnOnce() -> T) -> T {
        let ticket = match self.current() {
            Some(ticket) => ticket,
            None => {
                let ticket = Arc::new(self.ticket(label, true));
                let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
                while !self.start(&mut state, ticket.id) {
                    state = self.0.ready.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                ticket
            }
        };
        in_blocking_scope(ticket, work)
    }
}

fn in_blocking_scope<T>(ticket: Arc<Activity>, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<Activity>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            BLOCKING.with(|current| current.replace(self.0.take()));
        }
    }
    let _restore = Restore(BLOCKING.with(|current| current.replace(Some(ticket))));
    work()
}

pub fn global() -> &'static Queue {
    static QUEUE: OnceLock<Queue> = OnceLock::new();
    QUEUE.get_or_init(Queue::default)
}

/// Keep a parent's slot alive on child source threads, even when the scan's
/// time budget expires before a slow source has cooperatively stopped.
pub fn inherit<T>(work: impl FnOnce() -> T) -> impl FnOnce() -> T {
    let ticket = global().current();
    move || match ticket {
        Some(ticket) => in_blocking_scope(ticket, work),
        None => work(),
    }
}

/// Propagate a slot to an awaited or detached child task. Capture happens
/// before spawning, and the child retains the slot until it actually stops.
pub fn inherit_async<T>(future: impl Future<Output = T>) -> impl Future<Output = T> {
    let ticket = global().current();
    async move {
        match ticket {
            Some(ticket) => CURRENT.scope(ticket, future).await,
            None => future.await,
        }
    }
}

pub async fn blocking<T, F>(label: impl Into<String>, work: F) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    global().blocking(label, work).await
}

#[cfg(test)]
mod tests;

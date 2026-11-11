//! Install requests (#101), as the webview's `api/installRequests.ts`: a
//! link, path or magnet link pasted or dropped, a file dropped, a deep
//! link, a share link or a game another device sends all wait, inert, for
//! the user to confirm or dismiss. Nothing resolves, downloads or runs
//! before then. New ones arrive as `InstallRequested` (`Key::Jobs`).
//!
//! An [`InstallInbox`] is the running app's requests ([`AppInbox`]) or, in
//! fixtures and tests, [`FakeInbox`].

use std::sync::{Arc, Mutex};

use caskade_app::commands::install_requests::{
    self as commands, InputKind, InstallRequest, InstallRequested, InstallRequests,
};
use caskade_app::events::{Event, Events};
use caskade_core::install::{InstallService, JobSnapshot};
use caskade_core::store_accounts::async_trait;

use super::FakeQueue;

type Result<T> = std::result::Result<T, String>;

/// What the install confirmation does with requests.
#[async_trait]
pub trait InstallInbox: Send + Sync {
    /// The requests waiting, oldest first.
    fn pending(&self) -> Vec<InstallRequest>;
    /// Asks to install `value`: a path, an HTTP(S) URL, a magnet link or a
    /// share link.
    async fn request(&self, value: String) -> Result<InstallRequest>;
    /// Asks to install a dropped file or folder: the core works out what it
    /// installs (a shortcut's target, an executable's folder) first.
    async fn dropped(&self, path: String) -> Result<InstallRequest>;
    /// Starts it, on `root_id` (the default without one).
    async fn confirm(&self, id: &str, root_id: Option<String>) -> Result<JobSnapshot>;
    /// Adds the collection a share request links to (#247): a list, so no
    /// job runs.
    async fn add_collection(&self, id: &str) -> Result<()>;
    fn dismiss(&self, id: &str) -> Result<()>;
}

/// The running app's install requests.
pub struct AppInbox {
    pub requests: InstallRequests,
    pub install: Option<InstallService>,
    pub accounts: caskade_core::accounts::AccountService,
    pub shares: Option<caskade_share::Shares>,
    pub events: Events,
}

impl AppInbox {
    pub fn of(services: &caskade_app::start::Services, events: Events) -> Option<Self> {
        Some(Self {
            requests: services.install_requests.clone()?,
            install: services.install.clone(),
            accounts: services.accounts.clone(),
            shares: services.shares.clone(),
            events,
        })
    }
}

#[async_trait]
impl InstallInbox for AppInbox {
    fn pending(&self) -> Vec<InstallRequest> {
        commands::pending_install_requests(&self.requests)
    }
    async fn request(&self, value: String) -> Result<InstallRequest> {
        commands::request_install_input(
            &self.events,
            &self.requests,
            Some(&self.accounts),
            self.shares.as_ref(),
            value,
        )
        .await
    }
    async fn dropped(&self, path: String) -> Result<InstallRequest> {
        commands::request_dropped_install(&self.events, &self.requests, path).await
    }
    async fn confirm(&self, id: &str, root_id: Option<String>) -> Result<JobSnapshot> {
        let install = self
            .install
            .as_ref()
            .ok_or_else(|| caskade_core::tr!("Storage service is unavailable"))?;
        commands::confirm_install_request(&self.requests, install, id.to_owned(), root_id).await
    }
    async fn add_collection(&self, id: &str) -> Result<()> {
        caskade_app::commands::shares::confirm_share_collection(&self.requests, &self.accounts, id)
            .await
            .map(drop)
            .map_err(|error| error.summary)
    }
    fn dismiss(&self, id: &str) -> Result<()> {
        commands::dismiss_install_request(&self.requests, id.to_owned())
    }
}

/// Requests a test offers, and what the confirmation did with them:
/// `request <value>`, `dropped <path>`, `confirm <id> <root>`, `collection
/// <id>`, `dismiss <id>`. A confirmed request's job joins the fake queue.
pub struct FakeInbox {
    pending: Mutex<Vec<InstallRequest>>,
    calls: Mutex<Vec<String>>,
    next: Mutex<u32>,
    queue: Arc<FakeQueue>,
    events: Events,
}

impl FakeInbox {
    pub fn new(events: Events, queue: Arc<FakeQueue>) -> Self {
        Self {
            pending: Mutex::default(),
            calls: Mutex::default(),
            next: Mutex::new(0),
            queue,
            events,
        }
    }

    /// Queues `request`, as a deep link or another device would, and
    /// announces it.
    pub fn offer(&self, request: InstallRequest) {
        FakeQueue::lock(&self.pending).push(request.clone());
        self.events
            .send(Event::InstallRequested(InstallRequested(request)));
    }

    pub fn calls(&self) -> Vec<String> {
        FakeQueue::lock(&self.calls).clone()
    }

    pub fn clear(&self) {
        FakeQueue::lock(&self.pending).clear();
        FakeQueue::lock(&self.calls).clear();
    }

    fn called(&self, call: String) {
        FakeQueue::lock(&self.calls).push(call);
    }

    fn queue(&self, kind: InputKind, value: String) -> InstallRequest {
        let request = self.queue_quietly(kind, value);
        self.offer(request.clone());
        request
    }

    /// A new request, not yet offered.
    fn queue_quietly(&self, kind: InputKind, value: String) -> InstallRequest {
        let id = {
            let mut next = FakeQueue::lock(&self.next);
            *next += 1;
            format!("request-{next}")
        };
        InstallRequest {
            request_id: id,
            kind,
            value,
            plan: None,
            peer: None,
            share: None,
        }
    }

    fn take(&self, id: &str) -> Result<InstallRequest> {
        let mut pending = FakeQueue::lock(&self.pending);
        let at = pending
            .iter()
            .position(|request| request.request_id == id)
            .ok_or_else(|| caskade_core::tr!("unknown or expired request"))?;
        Ok(pending.remove(at))
    }
}

#[async_trait]
impl InstallInbox for FakeInbox {
    fn pending(&self) -> Vec<InstallRequest> {
        FakeQueue::lock(&self.pending).clone()
    }
    async fn request(&self, value: String) -> Result<InstallRequest> {
        self.called(format!("request {value}"));
        let kind = if value.starts_with('/') {
            InputKind::Path
        } else {
            InputKind::Url
        };
        Ok(self.queue(kind, value))
    }
    async fn dropped(&self, path: String) -> Result<InstallRequest> {
        self.called(format!("dropped {path}"));
        let plan = caskade_core::install::dropped::DropPlan {
            dropped: path.clone(),
            kind: caskade_core::install::dropped::DropKind::File,
            input: path.clone(),
            title: None,
            launch: None,
        };
        let mut request = self.queue_quietly(InputKind::Path, path);
        request.plan = Some(plan);
        self.offer(request.clone());
        Ok(request)
    }
    async fn confirm(&self, id: &str, root_id: Option<String>) -> Result<JobSnapshot> {
        self.called(format!(
            "confirm {id} {}",
            root_id.as_deref().unwrap_or("default")
        ));
        let request = self.take(id)?;
        let title = request
            .plan
            .as_ref()
            .and_then(|plan| plan.title.clone())
            .or_else(|| request.peer.as_ref().map(|peer| peer.title.clone()))
            .unwrap_or_else(|| request.value.clone());
        let job: JobSnapshot = serde_json::from_value(serde_json::json!({
            "id": format!("job-{id}"),
            "revision": 1,
            "kind": "install",
            "status": "queued",
            "title": title,
            "input": request.value,
            "rootId": root_id,
            "phase": null,
            "progress": null,
            "warnings": [],
            "question": null,
            "gameId": null,
            "error": null,
            "candidates": [],
            "selectedCandidateIndex": null,
            "targetDir": null,
            "dependencies": [],
            "verification": null,
            "failure": null,
            "createdAtMs": 1,
            "updatedAtMs": 1
        }))
        .map_err(|error| error.to_string())?;
        self.queue.put(job.clone());
        Ok(job)
    }
    async fn add_collection(&self, id: &str) -> Result<()> {
        self.called(format!("collection {id}"));
        self.take(id).map(drop)
    }
    fn dismiss(&self, id: &str) -> Result<()> {
        self.called(format!("dismiss {id}"));
        self.take(id).map(drop)
    }
}

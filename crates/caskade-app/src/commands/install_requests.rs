//! Shared ingress for file drops, DOM paste/link drops and deep links.
//! Merely requesting never resolves a URL, downloads bytes or executes input.
use caskade_core::install::{InstallService, JobSnapshot, dropped::DropPlan};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{Event, Events};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub request_id: String,
    pub kind: InputKind,
    pub value: String,
    /// How a dropped file or folder installs; `None` for other requests.
    pub plan: Option<DropPlan>,
    /// A game another of the user's devices sends here (#235).
    #[serde(default)]
    pub peer: Option<PeerSend>,
    /// A game someone shared through a link (#246), as its sharer says.
    /// The link stays here: it's a credential.
    #[serde(default)]
    pub share: Option<caskade_share::ShareInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Path,
    Url,
    /// A copy from another of the user's devices (#235).
    Peer,
    /// A game someone shared through a link (#246).
    Share,
}

/// A game another device sends to this one: copied from it once confirmed
/// (#233's copy, started from here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PeerSend {
    pub device_id: String,
    pub device_name: String,
    /// The release on that device.
    pub entry_id: String,
    /// The game here it is, when the devices have matched it.
    pub game_id: Option<String>,
    pub title: String,
    pub size_bytes: Option<u64>,
}

impl PeerSend {
    pub fn target(&self) -> caskade_core::install::PeerCopyTarget {
        caskade_core::install::PeerCopyTarget {
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            entry_id: self.entry_id.clone(),
            // A game id that isn't here makes the copy a game of its own.
            game_id: self.game_id.clone().unwrap_or_default(),
            title: self.title.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InstallRequested(pub InstallRequest);

/// A deep link turned down because the signed-in account may not install
/// (#205), with the reason for people. Nothing was queued. Pastes and drops
/// get the reason as their command's error instead.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InstallRefused(pub String);

struct Pending {
    request: InstallRequest,
    created: Instant,
    claimed: bool,
    /// A share request's link (#246).
    link: Option<String>,
}

#[derive(Clone, Default)]
pub struct InstallRequests {
    pending: Arc<Mutex<BTreeMap<String, Pending>>>,
    /// Where share requests are received from, once set up (#246).
    shares: Arc<std::sync::OnceLock<caskade_share::Shares>>,
}

/// How long an unconfirmed request waits.
const EXPIRY: Duration = Duration::from_secs(600);

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

struct Claim {
    queue: InstallRequests,
    request: InstallRequest,
    link: Option<String>,
    complete: bool,
}

impl Drop for Claim {
    fn drop(&mut self) {
        if !self.complete
            && let Some(p) = locked(&self.queue.pending).get_mut(&self.request.request_id)
        {
            p.claimed = false;
        }
    }
}

/// [`validate`] on the blocking pool.
async fn validated(value: String) -> Result<(InputKind, String), String> {
    crate::runtime::spawn_blocking(move || validate(value))
        .await
        .map_err(|e| e.to_string())?
}

/// Whether `value` is a UNC (`\\host\share`), verbatim (`\\?\`) or device
/// (`\\.\`) path. They all start with two separators, either slash.
fn is_network_or_device_path(value: &str) -> bool {
    let mut chars = value.chars();
    let separator = |c: Option<char>| matches!(c, Some('\\' | '/'));
    if separator(chars.next()) && separator(chars.next()) {
        return true;
    }
    #[cfg(windows)]
    if let Some(std::path::Component::Prefix(prefix)) =
        std::path::Path::new(value).components().next()
        && !matches!(prefix.kind(), std::path::Prefix::Disk(_))
    {
        return true;
    }
    false
}

pub fn pending_install_requests(state: &InstallRequests) -> Vec<InstallRequest> {
    state.pending()
}

pub fn dismiss_install_request(state: &InstallRequests, request_id: String) -> Result<(), String> {
    state.dismiss(&request_id)
}

pub async fn confirm_install_request(
    state: &InstallRequests,
    service: &InstallService,
    request_id: String,
    root_id: Option<String>,
) -> Result<JobSnapshot, String> {
    state.confirm(&request_id, root_id, service).await
}

impl InstallRequests {
    /// Receives confirmed share requests through `shares` (#246).
    pub fn set_shares(&self, shares: caskade_share::Shares) {
        let _ = self.shares.set(shares);
    }
    /// The queue, without unclaimed requests that expired.
    fn live(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Pending>> {
        let mut pending = locked(&self.pending);
        pending.retain(|_, p| p.claimed || p.created.elapsed() < EXPIRY);
        pending
    }
    /// Queues input that [`validate`] already checked.
    fn queue(
        &self,
        kind: InputKind,
        value: String,
        plan: Option<DropPlan>,
    ) -> Result<InstallRequest, String> {
        let mut pending = self.live();
        if let Some(existing) = pending
            .values()
            .find(|p| p.request.kind == kind && p.request.value == value && p.request.plan == plan)
        {
            return Ok(existing.request.clone());
        }
        if pending.len() >= 32 {
            return Err(caskade_core::tr!(
                "too many pending install requests; dismiss one first"
            ));
        }
        let request = InstallRequest {
            request_id: caskade_core::model::new_id(),
            kind,
            value,
            plan,
            peer: None,
            share: None,
        };
        pending.insert(
            request.request_id.clone(),
            Pending {
                request: request.clone(),
                created: Instant::now(),
                claimed: false,
                link: None,
            },
        );
        Ok(request)
    }
    pub fn pending(&self) -> Vec<InstallRequest> {
        self.live()
            .values()
            .filter(|p| !p.claimed)
            .map(|p| p.request.clone())
            .collect()
    }
    pub fn dismiss(&self, id: &str) -> Result<(), String> {
        let mut pending = locked(&self.pending);
        if pending.get(id).is_some_and(|p| p.claimed) {
            return Err(caskade_core::tr!("request is already being confirmed"));
        }
        pending
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| caskade_core::tr!("unknown or expired request"))
    }
    fn claim(&self, id: &str) -> Result<Claim, String> {
        let mut pending = locked(&self.pending);
        let p = pending
            .get_mut(id)
            .ok_or_else(|| caskade_core::tr!("unknown or expired request"))?;
        if p.claimed || p.created.elapsed() >= EXPIRY {
            return Err(caskade_core::tr!(
                "request is expired or already being confirmed"
            ));
        }
        p.claimed = true;
        Ok(Claim {
            queue: self.clone(),
            request: p.request.clone(),
            link: p.link.clone(),
            complete: false,
        })
    }
    pub async fn confirm(
        &self,
        id: &str,
        root_id: Option<String>,
        service: &InstallService,
    ) -> Result<JobSnapshot, String> {
        let mut claim = self.claim(id)?;
        if let Some(link) = claim.link.clone() {
            // Nothing was fetched until now (#246).
            let shares = self.shares.get().cloned();
            let job = shares
                .ok_or_else(|| caskade_core::tr!("receiving shares isn't available"))?
                .receive(&link, root_id)
                .await
                .map_err(|e| e.to_string())?;
            locked(&self.pending).remove(id);
            claim.complete = true;
            return Ok(job);
        }
        if let Some(peer) = claim.request.peer.clone() {
            // Nothing was written here until now (#235).
            let job = service
                .copy_from_device(peer.target(), root_id)
                .await
                .map_err(|e| e.to_string())?;
            locked(&self.pending).remove(id);
            claim.complete = true;
            return Ok(job);
        }
        let hints = claim
            .request
            .plan
            .as_ref()
            .map(DropPlan::hints)
            .unwrap_or_default();
        let job = service
            .install_with(claim.request.value.clone(), root_id, hints)
            .await
            .map_err(|e| e.to_string())?;
        locked(&self.pending).remove(id);
        claim.complete = true;
        Ok(job)
    }
    /// Adds the collection share request `id` links to to `account`'s
    /// collections (#248): a list, so nothing downloads and no job runs.
    pub async fn confirm_collection(
        &self,
        id: &str,
        account: &str,
    ) -> Result<caskade_core::model::Collection, String> {
        let mut claim = self.claim(id)?;
        let link = claim
            .link
            .clone()
            .ok_or_else(|| caskade_core::tr!("that request isn't a share"))?;
        let shares = self
            .shares
            .get()
            .cloned()
            .ok_or_else(|| caskade_core::tr!("receiving shares isn't available"))?;
        let collection = shares
            .receive_collection(&link, account)
            .await
            .map_err(|e| e.to_string())?;
        locked(&self.pending).remove(id);
        claim.complete = true;
        Ok(collection)
    }
    /// Used by the deep-link owner (#57). A path is checked on the
    /// blocking pool, never on the caller's thread: a stale network mount
    /// can hang a stat for minutes.
    pub async fn request_and_emit(
        &self,
        events: &Events,
        value: String,
    ) -> Result<InstallRequest, String> {
        let (kind, value) = validated(value).await?;
        self.queue_and_emit(events, kind, value, None)
    }
    /// Queues a game someone shared through `link`, as `info` says, to be
    /// confirmed (#246). The same link twice is one request.
    pub fn queue_share(
        &self,
        events: &Events,
        link: String,
        info: caskade_share::ShareInfo,
    ) -> Result<InstallRequest, String> {
        let request = {
            let mut pending = self.live();
            if let Some(existing) = pending
                .values()
                .find(|p| p.link.as_deref() == Some(link.as_str()))
            {
                existing.request.clone()
            } else {
                if pending.len() >= 32 {
                    return Err(caskade_core::tr!(
                        "too many pending install requests; dismiss one first"
                    ));
                }
                let request = InstallRequest {
                    request_id: caskade_core::model::new_id(),
                    kind: InputKind::Share,
                    value: info.title.clone(),
                    plan: None,
                    peer: None,
                    share: Some(info),
                };
                pending.insert(
                    request.request_id.clone(),
                    Pending {
                        request: request.clone(),
                        created: Instant::now(),
                        claimed: false,
                        link: Some(link),
                    },
                );
                request
            }
        };
        events.emit(Event::InstallRequested(InstallRequested(request.clone())))?;
        Ok(request)
    }
    /// Queues a game another device sends here, to be confirmed (#235).
    pub fn queue_peer(&self, events: &Events, peer: PeerSend) -> Result<InstallRequest, String> {
        let request = {
            let mut pending = self.live();
            if let Some(existing) = pending
                .values()
                .find(|p| p.request.peer.as_ref() == Some(&peer))
            {
                existing.request.clone()
            } else {
                if pending.len() >= 32 {
                    return Err(caskade_core::tr!(
                        "too many pending install requests; dismiss one first"
                    ));
                }
                let request = InstallRequest {
                    request_id: caskade_core::model::new_id(),
                    kind: InputKind::Peer,
                    value: peer.title.clone(),
                    plan: None,
                    peer: Some(peer),
                    share: None,
                };
                pending.insert(
                    request.request_id.clone(),
                    Pending {
                        request: request.clone(),
                        created: Instant::now(),
                        claimed: false,
                        link: None,
                    },
                );
                request
            }
        };
        events.emit(Event::InstallRequested(InstallRequested(request.clone())))?;
        Ok(request)
    }
    pub fn queue_and_emit(
        &self,
        events: &Events,
        kind: InputKind,
        value: String,
        plan: Option<DropPlan>,
    ) -> Result<InstallRequest, String> {
        let request = self.queue(kind, value, plan)?;
        events.emit(Event::InstallRequested(InstallRequested(request.clone())))?;
        Ok(request)
    }
}

fn validate(value: String) -> Result<(InputKind, String), String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 32_768 || value.chars().any(char::is_control) {
        return Err(caskade_core::tr!("input must be one path or HTTP(S) URL"));
    }
    if let Ok(url) = url::Url::parse(value)
        && matches!(url.scheme(), "http" | "https")
    {
        if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
            return Err(caskade_core::tr!("invalid download URL"));
        }
        return Ok((InputKind::Url, url.to_string()));
    }
    // A magnet link (#159) names a BitTorrent v1 or hybrid info hash.
    if let Ok(url) = url::Url::parse(value)
        && url.scheme() == "magnet"
    {
        if !url
            .query_pairs()
            .any(|(key, value)| key == "xt" && value.starts_with("urn:btih:"))
        {
            return Err(caskade_core::tr!(
                "a magnet link needs a BitTorrent info hash (xt=urn:btih:…)"
            ));
        }
        return Ok((InputKind::Url, url.to_string()));
    }
    let path = std::path::Path::new(value);
    // Checked before anything touches the filesystem: on Windows, statting
    // `\\host\share` connects to that host over SMB.
    if is_network_or_device_path(value) {
        return Err(caskade_core::tr!(
            "choose a local path, not a network or device path"
        ));
    }
    if !path.is_absolute() {
        return Err(caskade_core::tr!(
            "choose an absolute local path or HTTP(S) URL"
        ));
    }
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(caskade_core::tr!("input is not a file or folder"));
    }
    Ok((
        InputKind::Path,
        path.into_os_string()
            .into_string()
            .map_err(|_| caskade_core::tr!("path is not UTF-8"))?,
    ))
}

/// Queues `value` (a path, an HTTP(S) URL or a share link) for
/// confirmation and tells the UI; nothing is resolved or downloaded until
/// it's confirmed. A share link (#246) asks its sharer first, which needs
/// `shares` and an account that may install.
pub async fn request_install_input(
    events: &Events,
    state: &InstallRequests,
    accounts: Option<&caskade_core::accounts::AccountService>,
    shares: Option<&caskade_share::Shares>,
    value: String,
) -> Result<InstallRequest, String> {
    if caskade_share::is_link(&value) {
        crate::permissions::require(accounts, caskade_core::accounts::Permission::Install)?;
        let shares = shares.ok_or_else(|| caskade_core::tr!("receiving shares isn't available"))?;
        return super::shares::queue_share_link(events, shares, state, value).await;
    }
    state.request_and_emit(events, value).await
}

/// Queues a dropped file or folder: its plan (what installs it) is worked
/// out off the async runtime (`caskade_core::install::dropped`). It then
/// waits for confirmation like any other request.
pub async fn request_dropped_install(
    events: &Events,
    state: &InstallRequests,
    path: String,
) -> Result<InstallRequest, String> {
    if is_network_or_device_path(&path) {
        return Err(caskade_core::tr!(
            "choose a local path, not a network or device path"
        ));
    }
    let (plan, (kind, value)) = crate::runtime::spawn_blocking(move || {
        let plan = caskade_core::install::dropped::plan(std::path::Path::new(&path))
            .map_err(|e| e.to_string())?;
        let input = validate(plan.input.clone())?;
        Ok::<_, String>((plan, input))
    })
    .await
    .map_err(|e| e.to_string())??;
    state.queue_and_emit(events, kind, value, Some(plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    impl InstallRequests {
        fn request(&self, value: String) -> Result<InstallRequest, String> {
            let (kind, value) = validate(value)?;
            self.queue(kind, value, None)
        }
    }
    #[test]
    fn bounded_deduplicated_requests_expire_and_claims_are_exclusive() {
        let queue = InstallRequests::default();
        let request = queue
            .request("https://example.invalid/game.zip".into())
            .unwrap();
        assert_eq!(request, queue.request(request.value.clone()).unwrap());
        let claim = queue.claim(&request.request_id).unwrap();
        assert!(queue.claim(&request.request_id).is_err());
        assert!(queue.dismiss(&request.request_id).is_err());
        drop(claim);
        assert_eq!(queue.pending(), vec![request.clone()]);
        locked(&queue.pending)
            .get_mut(&request.request_id)
            .unwrap()
            .created = Instant::now() - Duration::from_secs(601);
        assert!(queue.pending().is_empty());
        assert!(queue.claim(&request.request_id).is_err());
        for i in 0..32 {
            queue
                .request(format!("https://example.invalid/{i}.zip"))
                .unwrap();
        }
        assert!(
            queue
                .request("https://example.invalid/overflow.zip".into())
                .is_err()
        );
        for value in [
            "javascript:alert(1)",
            "file:///tmp/game",
            "https://user:secret@example.invalid/x",
            "relative.zip",
            "https://example.invalid/a\nhttps://example.invalid/b",
            "magnet:?dn=no-hash",
        ] {
            assert!(validate(value.into()).is_err(), "{value}");
        }
        let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Game";
        assert_eq!(
            validate(magnet.into()).unwrap(),
            (InputKind::Url, magnet.to_owned())
        );
    }
    #[test]
    fn network_and_device_paths_are_refused_before_any_lookup() {
        for value in [
            r"\\host\share\game.exe",
            "//host/share/game.exe",
            r"\\?\C:\games\game.exe",
            r"\\?\UNC\host\share\game.exe",
            r"\\.\pipe\game",
            r"/\host\share",
        ] {
            assert!(is_network_or_device_path(value), "{value}");
            let error = validate(value.into()).unwrap_err();
            assert!(error.contains("network or device"), "{value}: {error}");
        }
        for value in [
            "/tmp/game.zip",
            r"C:\games\game.zip",
            "/",
            "relative/game.zip",
        ] {
            assert!(!is_network_or_device_path(value), "{value}");
        }
        let dir = tempfile::tempdir().unwrap();
        let (kind, _) = validate(dir.path().to_string_lossy().into_owned()).unwrap();
        assert!(matches!(kind, InputKind::Path));
    }
}

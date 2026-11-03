//! Who may open a connection to this device: the admission policy the
//! endpoint asks once per connection. All in memory, so a stranger's dial
//! costs no disk access.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::SystemTime;

use caskade_sync::{Admission, EndpointId, Grant, Purpose};

/// Decides who a share admits (#246): the grant belongs to a person, not a
/// paired device, so whoever keeps the shares answers.
pub type ShareAdmission = std::sync::Arc<dyn Fn(EndpointId, &Grant) -> bool + Send + Sync>;

#[derive(Default)]
pub(super) struct Gate {
    state: Mutex<State>,
    /// A forgotten device not told yet tried to connect: tell it now.
    knocked: tokio::sync::Notify,
    /// Shares' admission, once one-off sharing is set up.
    share: std::sync::OnceLock<ShareAdmission>,
}

#[derive(Default)]
struct State {
    /// Paired devices and the grant each presents.
    paired: HashMap<EndpointId, Grant>,
    /// Pairing links offered and not used yet, with when they expire.
    links: Vec<(Grant, SystemTime)>,
    /// A code pairing is in progress, in either direction.
    pairing: bool,
    /// Devices forgotten here and not told yet.
    forgotten: HashSet<EndpointId>,
}

impl Gate {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_paired(&self, paired: HashMap<EndpointId, Grant>, forgotten: HashSet<EndpointId>) {
        let mut state = self.state();
        state.paired = paired;
        state.forgotten = forgotten;
    }

    pub fn pair(&self, peer: EndpointId, grant: Grant) {
        let mut state = self.state();
        state.forgotten.remove(&peer);
        state.paired.insert(peer, grant);
    }

    /// Forgotten here; `tell` when the device still has to be told.
    pub fn forget(&self, peer: &EndpointId, tell: bool) {
        let mut state = self.state();
        state.paired.remove(peer);
        if tell {
            state.forgotten.insert(*peer);
        } else {
            state.forgotten.remove(peer);
        }
    }

    /// Resolves when a forgotten device not told yet tries to connect.
    pub async fn knocked(&self) {
        self.knocked.notified().await;
    }

    pub fn offer_link(&self, grant: Grant, expires: SystemTime) {
        let mut state = self.state();
        let now = SystemTime::now();
        state.links.retain(|(_, expires)| now < *expires);
        state.links.push((grant, expires));
    }

    /// Takes the code-pairing slot; `false` when a pairing is in progress.
    /// Who a share admits is `admission`'s to say (#246).
    pub fn set_share_admission(&self, admission: ShareAdmission) {
        let _ = self.share.set(admission);
    }

    pub fn reserve_pairing(&self) -> bool {
        !std::mem::replace(&mut self.state().pairing, true)
    }

    pub fn release_pairing(&self) {
        self.state().pairing = false;
    }
}

impl Admission for Gate {
    fn admits(&self, peer: EndpointId, purpose: Purpose, grant: Option<&Grant>) -> bool {
        match purpose {
            // Only a paired device, presenting the grant this device gave it.
            Purpose::Sync | Purpose::Copy | Purpose::Forget | Purpose::Stream | Purpose::Play => {
                let state = self.state();
                let admitted = match (state.paired.get(&peer), grant) {
                    (Some(expected), Some(grant)) => expected == grant,
                    _ => false,
                };
                if !admitted && state.forgotten.contains(&peer) {
                    self.knocked.notify_one();
                }
                admitted
            }
            Purpose::Pair => match grant {
                // A pairing link: spent here, on first use, whatever follows.
                Some(grant) => {
                    let mut state = self.state();
                    let now = SystemTime::now();
                    let found = state
                        .links
                        .iter()
                        .position(|(offered, expires)| offered == grant && now < *expires);
                    found.map(|index| state.links.remove(index)).is_some()
                }
                // A code pairing: one at a time; both users still confirm.
                None => self.reserve_pairing(),
            },
            // A share's own grant, as whoever keeps the shares says (#246);
            // none set up, none admitted.
            Purpose::Share => match (grant, self.share.get()) {
                (Some(grant), Some(admits)) => admits(peer, grant),
                _ => false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn peer() -> EndpointId {
        caskade_sync::Identity::ephemeral().id()
    }

    #[test]
    fn paired_devices_need_their_own_grant() {
        let gate = Gate::default();
        let (a, b) = (peer(), peer());
        let grant = Grant::generate();
        gate.pair(a, grant);
        assert!(gate.admits(a, Purpose::Sync, Some(&grant)));
        assert!(gate.admits(a, Purpose::Forget, Some(&grant)));
        assert!(gate.admits(a, Purpose::Stream, Some(&grant)));
        assert!(gate.admits(a, Purpose::Play, Some(&grant)));
        assert!(!gate.admits(b, Purpose::Play, Some(&grant)), "not paired");
        assert!(!gate.admits(a, Purpose::Sync, None));
        assert!(!gate.admits(a, Purpose::Sync, Some(&Grant::generate())));
        assert!(
            !gate.admits(b, Purpose::Sync, Some(&grant)),
            "another device"
        );
        assert!(!gate.admits(a, Purpose::Share, Some(&grant)));
        // Shares ask their own admission, never the paired devices'.
        let shared = Grant::generate();
        let expected = shared;
        gate.set_share_admission(std::sync::Arc::new(move |_, grant| *grant == expected));
        assert!(gate.admits(b, Purpose::Share, Some(&shared)));
        assert!(!gate.admits(a, Purpose::Share, Some(&grant)));
        assert!(!gate.admits(b, Purpose::Share, None));
        gate.forget(&a, true);
        assert!(!gate.admits(a, Purpose::Sync, Some(&grant)), "forgotten");
    }

    #[test]
    fn a_pairing_link_opens_once_and_not_after_it_expires() {
        let gate = Gate::default();
        let grant = Grant::generate();
        gate.offer_link(grant, SystemTime::now() + Duration::from_secs(600));
        assert!(gate.admits(peer(), Purpose::Pair, Some(&grant)));
        assert!(!gate.admits(peer(), Purpose::Pair, Some(&grant)), "used");

        let expired = Grant::generate();
        gate.offer_link(expired, SystemTime::now() - Duration::from_secs(1));
        assert!(!gate.admits(peer(), Purpose::Pair, Some(&expired)));
    }

    #[test]
    fn one_code_pairing_at_a_time() {
        let gate = Gate::default();
        assert!(gate.admits(peer(), Purpose::Pair, None));
        assert!(!gate.admits(peer(), Purpose::Pair, None));
        gate.release_pairing();
        assert!(gate.admits(peer(), Purpose::Pair, None));
    }
}

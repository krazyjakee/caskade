//! Finding devices on the local network, with no internet, over mDNS.
//!
//! iroh-mdns-address-lookup 0.4.0 (released with iroh 1.0.0, and the version
//! iroh-blobs 0.103 tests against) announces this endpoint and resolves
//! others under Caskade's own service name, so only Caskade devices answer.
//! It's added to the endpoint as an address lookup, so dialling a nearby
//! device by its id alone finds its local addresses.
//!
//! The announcement carries this device's [`DeviceInfo`] so a nearby list
//! can name devices before pairing. It goes only into the mDNS record: the
//! endpoint's own user data stays empty, so n0's DNS (which publishes this
//! endpoint's addresses on the internet) never sees a device name.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt as _;
use iroh::address_lookup::{AddressLookup, EndpointData, Error as LookupError, Item};
use iroh::{Endpoint, EndpointId};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_future::boxed::BoxStream;
use tokio::sync::broadcast;

use crate::device::DeviceInfo;
use crate::link::Links;

/// The mDNS service Caskade devices announce under.
pub const SERVICE: &str = "caskade";

/// A Caskade device announcing itself on this network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nearby {
    pub id: EndpointId,
    pub info: DeviceInfo,
    /// Its addresses on this network.
    pub addrs: Vec<SocketAddr>,
}

/// The nearby list changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NearbyChanged {
    Found(Nearby),
    Lost(EndpointId),
}

/// mDNS for one endpoint: the lookup, the announcement and the nearby list.
#[derive(Debug, Clone)]
pub struct Lan {
    lookup: Arc<Announcing>,
    nearby: Arc<Mutex<BTreeMap<EndpointId, Nearby>>>,
    events: broadcast::Sender<NearbyChanged>,
    _task: Arc<n0_future::task::AbortOnDropHandle<()>>,
}

impl Lan {
    /// Starts mDNS under `service` on `endpoint`, announcing `info`.
    pub(crate) async fn start(
        endpoint: &Endpoint,
        service: &str,
        info: DeviceInfo,
        links: Links,
    ) -> Result<Self, crate::Error> {
        let mdns = MdnsAddressLookup::builder()
            .service_name(service)
            .build(endpoint.id())
            .map_err(crate::Error::interrupted)?;
        let mut discovered = mdns.subscribe().await;
        let lookup = Arc::new(Announcing {
            mdns,
            state: Mutex::new(Published { info, last: None }),
        });
        endpoint
            .address_lookup()
            .map_err(crate::Error::interrupted)?
            .add(lookup.clone());

        let nearby: Arc<Mutex<BTreeMap<EndpointId, Nearby>>> = Arc::default();
        let events = broadcast::channel(64).0;
        let (list, sender, own) = (nearby.clone(), events.clone(), endpoint.id());
        let task = n0_future::task::spawn(async move {
            while let Some(event) = discovered.next().await {
                let change = match event {
                    DiscoveryEvent::Discovered { endpoint_info, .. } => {
                        let id = endpoint_info.endpoint_id;
                        let info = endpoint_info
                            .user_data()
                            .and_then(DeviceInfo::from_user_data);
                        match info {
                            Some(info) if id != own => {
                                links.announced(endpoint_info.ip_addrs().map(|addr| addr.ip()));
                                let found = Nearby {
                                    id,
                                    info,
                                    addrs: endpoint_info.ip_addrs().copied().collect(),
                                };
                                let mut list = list.lock().unwrap_or_else(|e| e.into_inner());
                                if list.get(&id) == Some(&found) {
                                    continue;
                                }
                                list.insert(id, found.clone());
                                NearbyChanged::Found(found)
                            }
                            _ => continue,
                        }
                    }
                    DiscoveryEvent::Expired { endpoint_id } => {
                        let mut list = list.lock().unwrap_or_else(|e| e.into_inner());
                        if list.remove(&endpoint_id).is_none() {
                            continue;
                        }
                        NearbyChanged::Lost(endpoint_id)
                    }
                    _ => continue,
                };
                let _ = sender.send(change);
            }
        });
        Ok(Self {
            lookup,
            nearby,
            events,
            _task: Arc::new(n0_future::task::AbortOnDropHandle::new(task)),
        })
    }

    /// The Caskade devices announcing themselves on this network now.
    pub fn nearby(&self) -> Vec<Nearby> {
        let list = self.nearby.lock().unwrap_or_else(|e| e.into_inner());
        list.values().cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<NearbyChanged> {
        self.events.subscribe()
    }

    /// Announces new device info (after a rename).
    pub fn announce(&self, info: DeviceInfo) {
        self.lookup.announce(info);
    }
}

#[derive(Debug)]
struct Published {
    info: DeviceInfo,
    last: Option<EndpointData>,
}

/// Wraps the mDNS lookup to add the device info to what it publishes, and
/// only there.
#[derive(Debug)]
struct Announcing {
    mdns: MdnsAddressLookup,
    state: Mutex<Published>,
}

impl Announcing {
    fn announce(&self, info: DeviceInfo) {
        let data = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.info = info;
            state.last.clone()
        };
        if let Some(data) = data {
            self.publish(&data);
        }
    }
}

impl AddressLookup for Announcing {
    fn publish(&self, data: &EndpointData) {
        let announced = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.last = Some(data.clone());
            data.clone().with_user_data(state.info.to_user_data())
        };
        self.mdns.publish(&announced);
    }

    fn resolve(&self, id: EndpointId) -> Option<BoxStream<Result<Item, LookupError>>> {
        self.mdns.resolve(id)
    }
}

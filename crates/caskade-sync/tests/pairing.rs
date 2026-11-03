//! Pairing by code and by link (#226), two or three endpoints on loopback.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use caskade_sync::pairing::{accept_link, pair_with_link};
use caskade_sync::{
    Admission, CodePairing, Config, DeviceInfo, DeviceKind, EndpointId, Error, Grant,
    LINK_LIFETIME, Paired, PairingLink, Purpose, Session, Sessions, SyncEndpoint,
};
use tokio::sync::{mpsc, oneshot};

/// Admits code pairing one at a time, and pairing links once each.
#[derive(Default)]
struct Gate {
    busy: Mutex<bool>,
    links: Mutex<Vec<(Grant, SystemTime)>>,
}

impl Gate {
    fn offer(&self, grant: Grant, expires: SystemTime) {
        self.links.lock().unwrap().push((grant, expires));
    }

    fn release(&self) {
        *self.busy.lock().unwrap() = false;
    }
}

impl Admission for Gate {
    fn admits(&self, _: EndpointId, purpose: Purpose, grant: Option<&Grant>) -> bool {
        if purpose != Purpose::Pair {
            return false;
        }
        match grant {
            Some(grant) => {
                let mut links = self.links.lock().unwrap();
                let now = SystemTime::now();
                let found = links
                    .iter()
                    .position(|(offered, expires)| offered == grant && now < *expires);
                // Spent on first use, whether it completes or not.
                found.map(|index| links.remove(index)).is_some()
            }
            None => {
                let mut busy = self.busy.lock().unwrap();
                !std::mem::replace(&mut *busy, true)
            }
        }
    }
}

/// What the accepting side's handler reports.
enum Inbound {
    Code {
        code: String,
        peer_name: String,
        decide: oneshot::Sender<bool>,
        done: oneshot::Receiver<Result<Paired, Error>>,
    },
    Link(Result<Paired, Error>),
}

struct Handler {
    me: DeviceInfo,
    gate: Arc<Gate>,
    tx: mpsc::UnboundedSender<Inbound>,
}

#[async_trait]
impl Sessions for Handler {
    async fn opened(&self, session: Session) {
        if session.grant().is_some() {
            let result = accept_link(session, &self.me, Grant::generate()).await;
            let _ = self.tx.send(Inbound::Link(result));
            return;
        }
        let pairing = match CodePairing::begin(session, &self.me).await {
            Ok(pairing) => pairing,
            Err(_) => {
                self.gate.release();
                return;
            }
        };
        let (decide, decision) = oneshot::channel();
        let (done_tx, done) = oneshot::channel();
        let _ = self.tx.send(Inbound::Code {
            code: pairing.code().to_owned(),
            peer_name: pairing.peer_info().name.clone(),
            decide,
            done,
        });
        let result = pairing
            .finish(async { decision.await.unwrap_or(false) }, Grant::generate())
            .await;
        self.gate.release();
        let _ = done_tx.send(result);
    }
}

struct Device {
    endpoint: SyncEndpoint,
    gate: Arc<Gate>,
    inbound: mpsc::UnboundedReceiver<Inbound>,
    me: DeviceInfo,
}

async fn device(name: &str) -> Device {
    let gate = Arc::new(Gate::default());
    let (tx, inbound) = mpsc::unbounded_channel();
    let me = DeviceInfo::new(name, DeviceKind::Desktop);
    let handler = Handler {
        me: me.clone(),
        gate: gate.clone(),
        tx,
    };
    let endpoint = SyncEndpoint::bind(Config::loopback(), gate.clone(), Arc::new(handler))
        .await
        .unwrap();
    Device {
        endpoint,
        gate,
        inbound,
        me,
    }
}

/// `from` dials `to` and both run the code exchange up to the codes.
async fn start_code(from: &mut Device, to: &mut Device) -> (CodePairing, Inbound) {
    let session = from
        .endpoint
        .connect(to.endpoint.addr(), Purpose::Pair, None)
        .await
        .unwrap();
    let pairing = CodePairing::begin(session, &from.me).await.unwrap();
    let inbound = tokio::time::timeout(Duration::from_secs(10), to.inbound.recv())
        .await
        .unwrap()
        .unwrap();
    (pairing, inbound)
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_by_code_needs_both_confirmations() {
    let mut desk = device("Desk").await;
    let mut tv = device("Living room").await;
    let (pairing, inbound) = start_code(&mut desk, &mut tv).await;
    let Inbound::Code {
        code,
        peer_name,
        decide,
        done,
    } = inbound
    else {
        panic!("a code pairing");
    };
    assert_eq!(pairing.code(), code, "both devices show the same code");
    assert_eq!(code.len(), 6);
    assert_eq!(peer_name, "Desk");
    assert_eq!(pairing.peer_info().name, "Living room");

    // The desk confirms first; nothing completes until the TV does.
    let (confirm_desk, desk_decision) = oneshot::channel::<bool>();
    let desk_side = tokio::spawn(pairing.finish(
        async { desk_decision.await.unwrap_or(false) },
        Grant::generate(),
    ));
    confirm_desk.send(true).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!desk_side.is_finished(), "waits for the other device");
    decide.send(true).unwrap();

    let desk_paired = desk_side.await.unwrap().unwrap();
    let tv_paired = done.await.unwrap().unwrap();
    assert_eq!(desk_paired.peer, tv.endpoint.id());
    assert_eq!(tv_paired.peer, desk.endpoint.id());
    assert_eq!(desk_paired.info.name, "Living room");
    assert_eq!(tv_paired.info.name, "Desk");
    // Each holds the grant the other will check.
    assert_eq!(desk_paired.grant_out, tv_paired.grant_in);
    assert_eq!(tv_paired.grant_out, desk_paired.grant_in);
    assert_ne!(desk_paired.grant_in, desk_paired.grant_out);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_on_either_device_ends_it_on_both() {
    for cancel_on_acceptor in [true, false] {
        let mut desk = device("Desk").await;
        let mut tv = device("TV").await;
        let (pairing, inbound) = start_code(&mut desk, &mut tv).await;
        let Inbound::Code { decide, done, .. } = inbound else {
            panic!("a code pairing");
        };
        let (desk_decide, desk_decision) = oneshot::channel::<bool>();
        let desk_side = tokio::spawn(pairing.finish(
            async { desk_decision.await.unwrap_or(false) },
            Grant::generate(),
        ));
        if cancel_on_acceptor {
            desk_decide.send(true).unwrap();
            decide.send(false).unwrap();
        } else {
            decide.send(true).unwrap();
            desk_decide.send(false).unwrap();
        }
        let desk_result = desk_side.await.unwrap();
        let tv_result = done.await.unwrap();
        assert!(
            matches!(desk_result, Err(Error::Cancelled)),
            "{desk_result:?}"
        );
        // Both see the cancel, whichever side's close lands first (#39).
        assert!(matches!(tv_result, Err(Error::Cancelled)), "{tv_result:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_third_device_cannot_join_a_pairing_in_progress() {
    let mut desk = device("Desk").await;
    let mut tv = device("TV").await;
    let intruder = device("Intruder").await;
    let (pairing, inbound) = start_code(&mut desk, &mut tv).await;

    // While the TV is pairing with the desk, another code pairing is refused.
    let refused = intruder
        .endpoint
        .connect(tv.endpoint.addr(), Purpose::Pair, None)
        .await;
    assert!(matches!(refused, Err(Error::Refused)), "{refused:?}");

    // And the pairing in progress completes with the desk only.
    let Inbound::Code { decide, done, .. } = inbound else {
        panic!("a code pairing");
    };
    decide.send(true).unwrap();
    let desk_paired = pairing
        .finish(async { true }, Grant::generate())
        .await
        .unwrap();
    let tv_paired = done.await.unwrap().unwrap();
    assert_eq!(tv_paired.peer, desk.endpoint.id());
    assert_eq!(desk_paired.peer, tv.endpoint.id());
    assert!(
        tv.inbound.try_recv().is_err(),
        "the intruder never got a code"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pairing_link_works_once() {
    let mut desk = device("Desk").await;
    let laptop = device("Laptop").await;
    let grant = Grant::generate();
    let expires = SystemTime::now() + LINK_LIFETIME;
    desk.gate.offer(grant, expires);
    let addr = desk.endpoint.shareable_addr(Duration::from_secs(1)).await;
    let link = PairingLink::parse(&PairingLink::new(addr, grant, expires).to_url()).unwrap();

    let laptop_paired = pair_with_link(&laptop.endpoint, &link, &laptop.me, Grant::generate())
        .await
        .unwrap();
    let Inbound::Link(desk_paired) = desk.inbound.recv().await.unwrap() else {
        panic!("a link pairing");
    };
    let desk_paired = desk_paired.unwrap();
    assert_eq!(laptop_paired.peer, desk.endpoint.id());
    assert_eq!(laptop_paired.info.name, "Desk");
    assert_eq!(desk_paired.peer, laptop.endpoint.id());
    assert_eq!(desk_paired.grant_in, laptop_paired.grant_out);
    assert_eq!(laptop_paired.grant_in, desk_paired.grant_out);

    // Used: refused, with a message that says so.
    let other = device("Other").await;
    let again = pair_with_link(&other.endpoint, &link, &other.me, Grant::generate()).await;
    assert!(matches!(again, Err(Error::LinkRefused)), "{again:?}");
    assert!(
        Error::LinkRefused
            .to_string()
            .contains("already used or has expired")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_pairing_link_is_refused() {
    let desk = device("Desk").await;
    let laptop = device("Laptop").await;
    let addr = desk.endpoint.shareable_addr(Duration::from_secs(1)).await;

    // The link itself says it expired: refused without dialling.
    let grant = Grant::generate();
    let past = SystemTime::now() - Duration::from_secs(1);
    desk.gate.offer(grant, past);
    let expired = PairingLink::new(addr.clone(), grant, past);
    let result = pair_with_link(&laptop.endpoint, &expired, &laptop.me, Grant::generate()).await;
    assert!(matches!(result, Err(Error::LinkExpired)), "{result:?}");
    assert!(Error::LinkExpired.to_string().contains("expired"));

    // The minting device's own record has expired, whatever the link says.
    let grant = Grant::generate();
    desk.gate
        .offer(grant, SystemTime::now() - Duration::from_secs(1));
    let claims_valid = PairingLink::new(addr, grant, SystemTime::now() + LINK_LIFETIME);
    let result = pair_with_link(
        &laptop.endpoint,
        &claims_valid,
        &laptop.me,
        Grant::generate(),
    )
    .await;
    assert!(matches!(result, Err(Error::LinkRefused)), "{result:?}");
}

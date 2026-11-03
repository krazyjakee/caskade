//! Pairing streaming through sync with two in-process devices on loopback
//! (#267): a paired device's host enters the client's code itself; a host
//! on another device says it isn't the one; a device not paired through
//! sync can't hand a code over at all.

use super::*;
use crate::{
    AppPaths,
    accounts::AccountService,
    bus::Bus,
    db::LibraryStore,
    devices::{MemoryKeyStore, Reach},
    settings::SettingsService,
};
use std::sync::Mutex;

/// A host: `id`, ready, with the requests that arrived, recording the
/// codes entered.
#[derive(Default)]
struct FakeHost {
    id: String,
    ready: bool,
    requests: Mutex<Vec<PairRequest>>,
    accepted: Mutex<Vec<(String, String, String)>>,
}

#[async_trait]
impl PairingHost for FakeHost {
    async fn ready(&self) -> bool {
        self.ready
    }

    async fn host_id(&self) -> Option<String> {
        Some(self.id.clone())
    }

    async fn pair_requests(&self) -> Result<Vec<PairRequest>> {
        Ok(self.requests.lock().unwrap().clone())
    }

    async fn accept_pairing(&self, id: &str, code: &str, name: &str) -> Result<()> {
        self.accepted
            .lock()
            .unwrap()
            .push((id.to_owned(), code.to_owned(), name.to_owned()));
        Ok(())
    }
}

struct Device {
    devices: Devices,
    _dir: tempfile::TempDir,
}

async fn within(what: &str, check: impl AsyncFn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !check().await {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn device(name: &str) -> Device {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(dir.path()))
        .unwrap();
    let devices = Devices::with_reach(
        store,
        accounts,
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.set_settings(settings);
    devices.start();
    within("the endpoint starts", || async {
        devices.status().await.this_device.listening
    })
    .await;
    devices.rename(name).await.unwrap();
    Device { devices, _dir: dir }
}

async fn pair(a: &Device, b: &Device) {
    let link = a.devices.pairing_link().await.unwrap();
    b.devices.accept_pairing_link(&link.url).await.unwrap();
    let b_id = b.devices.status().await.this_device.id.unwrap();
    within("the link pairs both", || async {
        a.devices
            .status()
            .await
            .devices
            .iter()
            .any(|d| d.id == b_id)
    })
    .await;
}

fn fast() -> LinkTiming {
    LinkTiming {
        request: Duration::from_secs(3),
        answer: Duration::from_secs(8),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_paired_through_sync_has_its_code_entered_by_the_host() {
    let den = device("Den").await;
    let deck = device("Steam Deck").await;
    pair(&den, &deck).await;
    let host = Arc::new(FakeHost {
        id: "18D1CF0D".into(),
        ready: true,
        ..FakeHost::default()
    });
    den.devices
        .set_stream_handler(Arc::new(HostHandover::with(host.clone(), fast())));

    // The request arrives a moment after the code is handed over.
    let arriving = {
        let host = host.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            host.requests.lock().unwrap().push(PairRequest {
                id: "r1".into(),
                name: "Steam Deck".into(),
                address: "127.0.0.1".into(),
            });
        })
    };
    let linked = LinkedPairing::with(deck.devices.clone(), fast());
    assert_eq!(linked.device_name().await, "Steam Deck");
    assert!(
        linked
            .hand_over("18d1cf0d", "Steam Deck", "4831", "Steam Deck")
            .await
    );
    arriving.await.unwrap();
    assert_eq!(
        *host.accepted.lock().unwrap(),
        [("r1".to_owned(), "4831".to_owned(), "Steam Deck".to_owned())]
    );

    // Another host, or one not letting others play: not this device.
    assert!(
        !linked
            .hand_over("OTHER", "Steam Deck", "1111", "Steam Deck")
            .await
    );
    assert_eq!(host.accepted.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_not_paired_through_sync_still_needs_the_code() {
    let den = device("Den").await;
    let stranger = device("Stranger").await;
    let host = Arc::new(FakeHost {
        id: "18D1CF0D".into(),
        ready: true,
        requests: Mutex::new(vec![PairRequest {
            id: "r1".into(),
            name: "Stranger".into(),
            address: "127.0.0.1".into(),
        }]),
        ..FakeHost::default()
    });
    den.devices
        .set_stream_handler(Arc::new(HostHandover::with(host.clone(), fast())));
    // It knows no paired device to hand the code to.
    let linked = LinkedPairing::with(stranger.devices.clone(), fast());
    assert!(
        !linked
            .hand_over("18D1CF0D", "Stranger", "4831", "Stranger")
            .await
    );
    // And the host's device refuses its connection outright.
    let den_id = den.devices.status().await.this_device.id.unwrap();
    assert!(
        stranger
            .devices
            .connect(&den_id, ConnectionPurpose::Stream)
            .await
            .is_err()
    );
    assert!(host.accepted.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_host_answers_only_for_itself_and_only_while_it_hosts() {
    let host = Arc::new(FakeHost {
        id: "18D1CF0D".into(),
        ready: false,
        ..FakeHost::default()
    });
    let handover = HostHandover::with(host.clone(), fast());
    let pair = |host: &str| Message::Pair {
        host: host.into(),
        request: "Deck".into(),
        code: "4831".into(),
        name: "Deck".into(),
    };
    assert_eq!(handover.answer(pair("18D1CF0D")).await, Message::NotHere);
    let ready = Arc::new(FakeHost {
        id: "18D1CF0D".into(),
        ready: true,
        ..FakeHost::default()
    });
    let handover = HostHandover::with(
        ready.clone(),
        LinkTiming {
            request: Duration::from_millis(300),
            answer: Duration::from_secs(1),
        },
    );
    assert_eq!(handover.answer(pair("OTHER")).await, Message::NotHere);
    // That host, but the request never came.
    assert!(matches!(
        handover.answer(pair("18D1CF0D")).await,
        Message::Refused { .. }
    ));
    assert_eq!(handover.answer(Message::Accepted).await, Message::NotHere);
}

#[test]
fn messages_read_as_one_line_each() {
    let pair = Message::Pair {
        host: "18D1".into(),
        request: "Steam Deck".into(),
        code: "4831".into(),
        name: "Steam Deck".into(),
    };
    let line = serde_json::to_string(&pair).unwrap();
    assert_eq!(
        line,
        r#"{"type":"pair","host":"18D1","request":"Steam Deck","code":"4831","name":"Steam Deck"}"#
    );
    assert_eq!(serde_json::from_str::<Message>(&line).unwrap(), pair);
    assert_eq!(
        serde_json::to_string(&Message::NotHere).unwrap(),
        r#"{"type":"notHere"}"#
    );
}

//! Finding hosts with a fake mDNS responder and `/serverinfo` from
//! wiremock, and a paired host's apps joining library games. The XML and
//! the settings lines are Sunshine v2026.914's and moonlight-qt 6.2.0's.

use super::*;
use crate::{
    AppPaths,
    managed::{ManagedTools, ToolCatalog},
    model::Game,
};
use hickory_proto::{
    op::{Message, MessageType, OpCode},
    rr::{
        Name, Record,
        rdata::{A, PTR, SRV},
    },
};
use std::net::{Ipv4Addr, SocketAddr};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const SERVERINFO: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<root status_code="200"><hostname>Den</hostname><appversion>7.1.431.-1</appversion><GfeVersion>3.23.0.74</GfeVersion><uniqueid>A466B6A4-3EC7-8C2E-629E-021AC1F5E209</uniqueid><HttpsPort>47984</HttpsPort><ExternalPort>47989</ExternalPort><mac>00:00:00:00:00:00</mac><LocalIP>127.0.0.1</LocalIP><PairStatus>0</PairStatus><currentgame>0</currentgame><state>SUNSHINE_SERVER_FREE</state></root>"#;

const PAIRED: &str = r"[General]
uniqueid=37460f2e7934c368

[hosts]
1\apps\1\appcollector=false
1\apps\1\id=1285314453
1\apps\1\name=Hades
1\apps\2\id=88
1\apps\2\name=Celeste
1\apps\size=2
1\hostname=Den
1\localaddress=
1\mac=@ByteArray(\x12\x34\x56\x78\x9a\xbc)
1\manualaddress=127.0.0.1
1\uuid=A466B6A4-3EC7-8C2E-629E-021AC1F5E209
size=1
";

#[test]
fn serverinfo_names_the_host_and_says_when_it_is_busy() {
    let info = server_info(SERVERINFO).unwrap();
    assert_eq!(info.name, "Den");
    assert_eq!(info.id, "A466B6A4-3EC7-8C2E-629E-021AC1F5E209");
    assert!(!info.busy);
    let busy = SERVERINFO
        .replace("SUNSHINE_SERVER_FREE", "SUNSHINE_SERVER_BUSY")
        .replace("<currentgame>0", "<currentgame>881448767");
    assert!(server_info(&busy).unwrap().busy);
    assert!(server_info("<root><hostname>x</hostname></root>").is_none());
}

#[test]
fn paired_hosts_and_their_apps_come_from_the_clients_state() {
    let hosts = paired_hosts(PAIRED);
    assert_eq!(hosts.len(), 1);
    let den = &hosts[0];
    assert_eq!(den.id, "A466B6A4-3EC7-8C2E-629E-021AC1F5E209");
    assert_eq!(den.name, "Den");
    assert_eq!(den.address.as_deref(), Some("127.0.0.1"));
    assert!(den.wakeable);
    assert_eq!(den.mac, Some([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]));
    let names: Vec<&str> = den.apps.iter().map(|app| app.name.as_str()).collect();
    assert_eq!(names, ["Hades", "Celeste"]);
    assert!(
        !paired_hosts(&PAIRED.replace(r"@ByteArray(\x12\x34\x56\x78\x9a\xbc)", "@ByteArray()"))[0]
            .wakeable
    );
}

#[test]
fn qt_byte_arrays_read_as_bytes() {
    // Moonlight's record of a real host's MAC: a tab among the escapes.
    assert_eq!(
        qt_raw_bytes(r"@ByteArray(\xa4\xb1\xc1\t\xdb\x98)"),
        Some(vec![0xa4, 0xb1, 0xc1, 0x09, 0xdb, 0x98])
    );
    // A hex escape runs to the next non-hex character.
    assert_eq!(
        qt_raw_bytes(r#""@ByteArray(\x1A\0z)""#),
        Some(vec![0x1a, 0, b'z'])
    );
    assert_eq!(qt_raw_bytes("@ByteArray()"), Some(vec![]));
    assert_eq!(qt_raw_bytes("plain"), None);
    let record = add_srvcert(PAIRED);
    assert_eq!(
        paired_hosts(&record)[0].server_cert_pem.as_deref(),
        Some("-----BEGIN CERTIFICATE-----\nAB\n-----END CERTIFICATE-----\n")
    );
}

/// [`PAIRED`] with a host certificate as Moonlight writes it.
fn add_srvcert(text: &str) -> String {
    text.replace(
        "1\\uuid=",
        "1\\srvcert=@ByteArray(-----BEGIN CERTIFICATE-----\\nAB\\n-----END CERTIFICATE-----\\n)\n1\\uuid=",
    )
}

/// An mDNS answer for host `instance` at `address`, HTTP on `port`.
fn answer(instance: &str, address: Ipv4Addr, port: u16) -> Vec<u8> {
    let service = Name::from_ascii(mdns::SERVICE).unwrap();
    let full = Name::from_ascii(format!("{instance}.{}", mdns::SERVICE)).unwrap();
    let target = Name::from_ascii(format!("{instance}.local.")).unwrap();
    let mut message = Message::new(0, MessageType::Response, OpCode::Query);
    message.answers.push(Record::from_rdata(
        service,
        120,
        RData::PTR(PTR(full.clone())),
    ));
    message.additionals.push(Record::from_rdata(
        full,
        120,
        RData::SRV(SRV::new(0, 0, port, target.clone())),
    ));
    message
        .additionals
        .push(Record::from_rdata(target, 120, RData::A(A(address))));
    message.to_vec().unwrap()
}

use hickory_proto::rr::RData;

#[test]
fn mdns_answers_make_hosts() {
    let found = mdns::hosts(&[(
        answer("krazy", Ipv4Addr::new(192, 168, 1, 7), 47989),
        IpAddr::from([192, 168, 1, 7]),
    )]);
    assert_eq!(
        found,
        [mdns::Found {
            instance: "krazy".into(),
            address: IpAddr::from([192, 168, 1, 7]),
            port: 47989,
        }]
    );
    // A question, or noise, is no host.
    assert!(mdns::hosts(&[(mdns::query().unwrap(), IpAddr::from([127, 0, 0, 1]))]).is_empty());
    assert!(mdns::hosts(&[(vec![1, 2, 3], IpAddr::from([127, 0, 0, 1]))]).is_empty());
}

/// Answers every mDNS question with `answer` while `on` is set.
async fn responder(answer: Vec<u8>, on: Arc<std::sync::atomic::AtomicBool>) -> SocketAddr {
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buffer = vec![0_u8; 2048];
        while let Ok((_, from)) = socket.recv_from(&mut buffer).await {
            if on.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = socket.send_to(&answer, from).await;
            }
        }
    });
    address
}

struct Fixture {
    temp: tempfile::TempDir,
    store: LibraryStore,
    hosts: StreamHosts,
    serverinfo: MockServer,
    on: Arc<std::sync::atomic::AtomicBool>,
}

async fn fixture(paired: Option<&str>) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let serverinfo = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/serverinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SERVERINFO))
        .mount(&serverinfo)
        .await;
    let on = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let mdns = responder(
        answer("krazy", Ipv4Addr::LOCALHOST, serverinfo.address().port()),
        on.clone(),
    )
    .await;
    let tools = ManagedTools::new(
        store.clone(),
        Bus::default(),
        ToolCatalog::default(),
        &paths,
    );
    let state = temp.path().join("client-state");
    std::fs::create_dir_all(state.join(super::SETTINGS_DIR)).unwrap();
    if let Some(paired) = paired {
        std::fs::write(
            state.join(super::SETTINGS_DIR).join(super::SETTINGS_FILE),
            paired,
        )
        .unwrap();
    }
    let hosts = StreamHosts::with(
        store.clone(),
        Bus::default(),
        settings,
        StreamingClient::new(tools),
        mdns,
        HostTiming {
            listen: Duration::from_millis(300),
            answer: Duration::from_millis(500),
            every: Duration::from_millis(100),
            apps_every: Duration::from_secs(3600),
        },
    )
    .unwrap()
    .with_client_state(state);
    Fixture {
        temp,
        store,
        hosts,
        serverinfo,
        on,
    }
}

#[tokio::test]
async fn a_host_on_the_network_appears_and_one_that_leaves_is_unreachable() {
    let f = fixture(None).await;
    let started = Instant::now();
    f.hosts.refresh().await.unwrap();
    let hosts = f.hosts.hosts();
    assert_eq!(hosts.len(), 1, "{hosts:?}");
    assert_eq!(hosts[0].name, "Den", "named by /serverinfo, not mDNS");
    assert_eq!(hosts[0].reach, HostReach::Ready);
    assert!(!hosts[0].paired);
    assert!(started.elapsed() < Duration::from_secs(3), "within seconds");

    // It leaves: no answers, no /serverinfo.
    f.on.store(false, std::sync::atomic::Ordering::SeqCst);
    f.serverinfo.reset().await;
    f.hosts.refresh().await.unwrap();
    let hosts = f.hosts.hosts();
    assert_eq!(hosts[0].reach, HostReach::Unreachable);
    let _ = &f.temp;
}

/// This device's own host found on the network, as a VM run showed: it's
/// the same host that answers on loopback, so it's left out by its id.
#[tokio::test]
async fn a_host_is_not_listed_on_its_own_device() {
    let f = fixture(None).await;
    let hosts = f.hosts.with_own_port(f.serverinfo.address().port());
    hosts.refresh().await.unwrap();
    assert_eq!(hosts.hosts(), [], "this device is no other device");

    // Stopping it doesn't bring it back as a device that can't be reached.
    f.on.store(false, std::sync::atomic::Ordering::SeqCst);
    f.serverinfo.reset().await;
    hosts.refresh().await.unwrap();
    assert_eq!(hosts.hosts(), []);
    let _ = &f.temp;
}

/// Names aren't identities: another device called the same still shows.
#[tokio::test]
async fn another_device_with_this_devices_name_still_shows() {
    let f = fixture(None).await;
    let own = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/serverinfo"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(SERVERINFO.replace(
                "A466B6A4-3EC7-8C2E-629E-021AC1F5E209",
                "11111111-2222-3333-4444-555555555555",
            )),
        )
        .mount(&own)
        .await;
    let hosts = f.hosts.with_own_port(own.address().port());
    hosts.refresh().await.unwrap();
    let hosts = hosts.hosts();
    assert_eq!(hosts.len(), 1, "{hosts:?}");
    assert_eq!(hosts[0].name, "Den");
    assert_eq!(hosts[0].id, "A466B6A4-3EC7-8C2E-629E-021AC1F5E209");
    let _ = &f.temp;
}

/// Paired with itself before this was filtered, and in the client's own
/// spelling of the id: its games don't stream from itself either.
#[tokio::test]
async fn a_host_paired_with_itself_is_left_out_with_its_games() {
    let lower = PAIRED.replace(
        "A466B6A4-3EC7-8C2E-629E-021AC1F5E209",
        "a466b6a4-3ec7-8c2e-629e-021ac1f5e209",
    );
    let f = fixture(Some(&lower)).await;
    let hosts = f.hosts.with_own_port(f.serverinfo.address().port());
    hosts.refresh().await.unwrap();
    assert_eq!(hosts.hosts(), []);
    assert!(hosts.paired().await.is_empty());
    let _ = &f.temp;
}

/// A restart with hosting off: nothing answers on loopback, and a device
/// paired with itself stays away all the same.
#[tokio::test]
async fn a_host_paired_with_itself_stays_left_out_after_a_restart() {
    let f = fixture(Some(PAIRED)).await;
    let inner = &f.hosts.inner;
    let restarted = StreamHosts::with(
        f.store.clone(),
        Bus::default(),
        inner.settings.clone(),
        inner.client.clone(),
        inner.mdns,
        inner.timing,
    )
    .unwrap()
    .with_own_port(f.serverinfo.address().port())
    .with_client_state(f.temp.path().join("client-state"));
    let hosts = f.hosts.with_own_port(f.serverinfo.address().port());
    hosts.refresh().await.unwrap();
    assert_eq!(hosts.hosts(), []);

    f.on.store(false, std::sync::atomic::Ordering::SeqCst);
    f.serverinfo.reset().await;
    restarted.refresh().await.unwrap();
    assert_eq!(restarted.hosts(), []);
    assert!(restarted.paired().await.is_empty());
    let _ = &f.temp;
}

#[tokio::test]
async fn a_typed_address_is_kept_and_asked() {
    let f = fixture(None).await;
    f.on.store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(f.hosts.add_address("not an address!").await.is_err());
    // A host mDNS doesn't reach, typed in: here, Den's test port is
    // reachable only through mDNS, so a typed host that doesn't answer
    // isn't listed.
    f.hosts.add_address("192.0.2.1").await.unwrap();
    f.hosts.add_address("192.0.2.1").await.unwrap();
    let settings: StreamingSettings = f.hosts.inner.settings.read_or_default(SETTINGS).await;
    assert_eq!(settings.addresses, ["192.0.2.1"]);
    f.hosts.refresh().await.unwrap();
    assert!(f.hosts.hosts().is_empty());
}

#[tokio::test]
async fn looking_again_asks_a_paired_host_for_its_games_at_once() {
    let f = fixture(Some(PAIRED)).await;
    let asked = || {
        let times = crate::lock(&f.hosts.inner.apps_at);
        times.values().next().copied().expect("the host was asked")
    };
    f.hosts.refresh().await.unwrap();
    let first = asked();
    f.hosts.refresh().await.unwrap();
    assert_eq!(asked(), first, "asked again within apps_every");
    f.hosts.look_again().await.unwrap();
    assert!(asked() > first, "a paired device waited for apps_every");
}

#[tokio::test]
async fn a_paired_hosts_games_join_the_library_and_leave_with_it() {
    let f = fixture(Some(PAIRED)).await;
    let hades = Game::new("HADES");
    f.store.upsert_game(&hades).await.unwrap();
    let steam = crate::model::GameEntry::new(&hades.id, "steam".to_owned().into(), "1145360");
    f.store.upsert_entry(&steam).await.unwrap();
    f.hosts.refresh().await.unwrap();
    let den = &f.hosts.hosts()[0];
    assert!(den.paired);
    assert_eq!(den.games, 2);

    // Hades joins the library's Hades (title case aside); Celeste becomes a
    // streaming-only game.
    let entries: Vec<_> = f
        .store
        .entries_for_game(&hades.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|entry| entry.source_id.as_str() != "steam")
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].source_id.as_str(),
        "stream:A466B6A4-3EC7-8C2E-629E-021AC1F5E209"
    );
    assert_eq!(entries[0].external_id, "1285314453");
    assert_eq!(entries[0].state, crate::model::InstallState::Partial);
    // It streams the host's app by name (#264).
    let launch = Some(crate::model::LaunchSpec::Stream {
        host: "A466B6A4-3EC7-8C2E-629E-021AC1F5E209".into(),
        app: "Hades".into(),
    });
    assert_eq!(entries[0].launch, launch);
    let all = f.store.list_games(&Default::default()).await.unwrap();
    let titles: Vec<&str> = all.items.iter().map(|game| game.title.as_str()).collect();
    assert!(titles.contains(&"Celeste"), "{titles:?}");

    // Again: nothing changes.
    let again = f
        .store
        .sync_stream_apps(
            "A466B6A4-3EC7-8C2E-629E-021AC1F5E209",
            &paired_hosts(PAIRED)[0].apps,
        )
        .await
        .unwrap();
    assert_eq!(again, crate::db::StreamSync::default());

    // A release from before streaming launches catches up.
    let mut old = entries[0].clone();
    old.launch = None;
    f.store.upsert_entry(&old).await.unwrap();
    let caught_up = f
        .store
        .sync_stream_apps(
            "A466B6A4-3EC7-8C2E-629E-021AC1F5E209",
            &paired_hosts(PAIRED)[0].apps,
        )
        .await
        .unwrap();
    assert_eq!(caught_up.updated, std::slice::from_ref(&hades.id));
    let entry = f.store.get_entry(&old.id).await.unwrap().unwrap();
    assert_eq!(entry.launch, launch);

    // The host stops offering both: Celeste's streaming-only game goes; the
    // library's Hades stays with its own release.
    let fewer = f
        .store
        .sync_stream_apps("A466B6A4-3EC7-8C2E-629E-021AC1F5E209", &[])
        .await
        .unwrap();
    assert_eq!(fewer.removed.len(), 1);
    assert_eq!(fewer.updated, std::slice::from_ref(&hades.id));
    assert!(f.store.game(&hades.id).await.unwrap().is_some());
    let left = f.store.entries_for_game(&hades.id).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].source_id.as_str(), "steam");

    // Unreachable now, but wakeable: asleep.
    f.on.store(false, std::sync::atomic::Ordering::SeqCst);
    f.serverinfo.reset().await;
    f.hosts.refresh().await.unwrap();
    assert_eq!(f.hosts.hosts()[0].reach, HostReach::Asleep);
}

#[tokio::test]
async fn hidden_games_and_title_only_records_are_not_joined() {
    let f = fixture(None).await;
    let mut hidden = Game::new("Hades");
    hidden.hidden = true;
    f.store.upsert_game(&hidden).await.unwrap();
    let release = crate::model::GameEntry::new(&hidden.id, "steam".to_owned().into(), "1");
    f.store.upsert_entry(&release).await.unwrap();
    // A title-only record (#174) isn't joined either: it would go with the app.
    let wished = Game::new("Celeste");
    f.store.upsert_game(&wished).await.unwrap();
    let apps = [
        StreamApp {
            id: "1".into(),
            name: "Hades".into(),
        },
        StreamApp {
            id: "2".into(),
            name: "Celeste".into(),
        },
    ];
    let sync = f.store.sync_stream_apps("host", &apps).await.unwrap();
    assert_eq!(sync.added.len(), 2, "games of their own");
    assert_eq!(f.store.entries_for_game(&hidden.id).await.unwrap().len(), 1);
    assert!(
        f.store
            .entries_for_game(&wished.id)
            .await
            .unwrap()
            .is_empty()
    );
    let gone = f.store.sync_stream_apps("host", &[]).await.unwrap();
    assert_eq!(gone.removed.len(), 2);
    assert!(
        f.store.game(&wished.id).await.unwrap().is_some(),
        "the record stays"
    );
}

#[tokio::test]
async fn a_host_on_a_synced_device_names_its_games_by_that_devices_copies() {
    let store = LibraryStore::in_memory().await.unwrap();
    // Two games of one title: this device's own, and the one the host's
    // device has installed (its copy, through sync).
    let here = Game::new("Hades");
    store.upsert_game(&here).await.unwrap();
    store
        .upsert_entry(&crate::model::GameEntry::new(
            &here.id,
            "steam".to_owned().into(),
            "1145360",
        ))
        .await
        .unwrap();
    let there = Game::new("Hades");
    store.upsert_game(&there).await.unwrap();
    store
        .upsert_entry(&crate::model::GameEntry::new(
            &there.id,
            "peer:DEVICE1".to_owned().into(),
            "entry-1",
        ))
        .await
        .unwrap();
    let apps = [crate::db::StreamApp {
        id: "7".into(),
        name: "Hades".into(),
    }];
    let named = store
        .sync_stream_apps_from("HOST1", &apps, Some("DEVICE1"))
        .await
        .unwrap();
    assert_eq!(named.updated, std::slice::from_ref(&there.id));
    // Without the device, the title decides, as before.
    let titled = store.sync_stream_apps("HOST2", &apps).await.unwrap();
    assert_eq!(titled.updated, std::slice::from_ref(&here.id));
}

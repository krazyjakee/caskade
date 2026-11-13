//! Getting a stream ready against a fake host: plain `/serverinfo` for
//! reachability, and the paired client's HTTPS requests behind a TLS front
//! with the host's certificate.

use super::*;
use crate::{
    client::tests::client_with,
    pairing::{HostRecord, add_host, qt_byte_array},
    testing::{Cert, tls_front},
};
use caskade_core::{AppPaths, db::LibraryStore};
use std::sync::Mutex as StdMutex;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate, matchers::path_regex};

const SERVER_CERT: &str = include_str!("../pairing/server-cert.pem");
const SERVER_KEY: &str = include_str!("../pairing/server-key.pem");
const CLIENT_CERT: &str = include_str!("../pairing/client-cert.pem");
const CLIENT_KEY: &str = include_str!("../pairing/client-key.pem");
const CLIENT_ID: &str = "37460f2e7934c368";
const HOST_ID: &str = "8D2C1E3B-0000-4000-8000-0123456789AB";
const MOONLIGHT: &str = "#!/bin/sh\nexit 0\n";

/// A host: plays `playing`, offers Celeste (881) and Hades (882), and
/// knows this device unless `forgot`.
#[derive(Clone, Default)]
struct Host {
    playing: Arc<StdMutex<Option<String>>>,
    forgot: Arc<StdMutex<bool>>,
    cancels: Arc<StdMutex<u32>>,
}

impl Respond for Host {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let paired_client = request
            .url
            .query_pairs()
            .any(|(key, value)| key == "uniqueid" && value == CLIENT_ID);
        if paired_client && *self.forgot.lock().unwrap() {
            return ResponseTemplate::new(401).set_body_string(
                r#"<root status_code="401" status_message="The client is not authorized."/>"#,
            );
        }
        let xml = |body: String| {
            ResponseTemplate::new(200).set_body_string(format!(
                r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200">{body}</root>"#
            ))
        };
        match request.url.path() {
            "/serverinfo" => {
                let playing = self.playing.lock().unwrap().clone();
                xml(format!(
                    "<hostname>Den</hostname><uniqueid>{HOST_ID}</uniqueid><PairStatus>{}</PairStatus><currentgame>{}</currentgame><state>{}</state>",
                    u8::from(paired_client),
                    playing.as_deref().unwrap_or("0"),
                    if playing.is_some() { "SUNSHINE_SERVER_BUSY" } else { "SUNSHINE_SERVER_FREE" },
                ))
            }
            "/applist" => xml(
                "<App><AppTitle>Celeste</AppTitle><ID>881</ID></App><App><AppTitle>Hades</AppTitle><ID>882</ID></App>"
                    .into(),
            ),
            "/cancel" => {
                *self.cancels.lock().unwrap() += 1;
                *self.playing.lock().unwrap() = None;
                xml("<cancel>1</cancel>".into())
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

struct Setup {
    _temp: tempfile::TempDir,
    _github: MockServer,
    _server: MockServer,
    host: Host,
    streams: Streams,
    settings: SettingsService,
    state: std::path::PathBuf,
}

fn identity() -> String {
    format!(
        "[General]\ncertificate={}\nkey={}\nuniqueid={CLIENT_ID}\n",
        qt_byte_array(CLIENT_CERT),
        qt_byte_array(CLIENT_KEY)
    )
}

async fn setup() -> Setup {
    setup_with(None).await
}

async fn setup_with(wake_to: Option<std::net::SocketAddr>) -> Setup {
    let (temp, client, github) = client_with(false, MOONLIGHT).await;
    let state = client
        .ensure(&CancellationToken::new())
        .await
        .unwrap()
        .state;
    let host = Host::default();
    let server = MockServer::start().await;
    Mock::given(path_regex("^/(serverinfo|applist|cancel)$"))
        .respond_with(host.clone())
        .mount(&server)
        .await;
    let front = tls_front(*server.address(), &Cert::from_pem(SERVER_CERT, SERVER_KEY)).await;
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store);
    settings
        .register_core(&AppPaths::under(temp.path()))
        .unwrap();
    let streams = Streams::with(
        client,
        None,
        settings.clone(),
        StreamTiming {
            wake: Duration::from_millis(400),
            poll: Duration::from_millis(50),
            ..StreamTiming::default()
        },
        (server.address().port(), front.port()),
    );
    let streams = match wake_to {
        Some(target) => streams.with_wake_targets(vec![target]),
        None => streams,
    };
    let setup = Setup {
        _temp: temp,
        _github: github,
        _server: server,
        host,
        streams,
        settings,
        state,
    };
    setup.record("127.0.0.1", None);
    setup
}

impl Setup {
    /// This device paired with the host at `address`, with `mac`.
    fn record(&self, address: &str, mac: Option<&str>) {
        let mut text = add_host(
            &identity(),
            &HostRecord {
                uuid: HOST_ID.into(),
                name: "Den".into(),
                address: address.into(),
                server_cert_pem: SERVER_CERT.into(),
            },
        );
        if let Some(mac) = mac {
            text = text.replace("1\\mac=@ByteArray()", &format!("1\\mac={mac}"));
        }
        self.write(&text);
    }

    fn write(&self, text: &str) {
        let dir = self.state.join(SETTINGS_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(SETTINGS_FILE), text).unwrap();
    }

    async fn prepare(&self, app: &str, stop_host: bool) -> Result<StreamStart, LaunchError> {
        self.streams.prepare(HOST_ID, app, stop_host).await
    }

    fn cancels(&self) -> u32 {
        *self.host.cancels.lock().unwrap()
    }
}

fn args(start: &StreamStart) -> Vec<String> {
    start
        .command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

fn refusal(result: Result<StreamStart, LaunchError>) -> String {
    match result {
        Err(LaunchError::Refused(text)) => text,
        Err(other) => panic!("expected a refusal, got {other}"),
        Ok(_) => panic!("expected a refusal"),
    }
}

#[tokio::test]
async fn a_ready_host_streams_the_game_full_screen_with_the_users_quality() {
    let setup = setup().await;
    let start = setup.prepare("Celeste", false).await.unwrap();
    assert_eq!(
        args(&start),
        [
            "stream",
            "127.0.0.1",
            "Celeste",
            "--display-mode",
            "fullscreen",
            "--no-quit-after"
        ]
    );
    // The client starts in its own state folder.
    assert_eq!(start.command.get_current_dir(), Some(setup.state.as_path()));
    assert!(start.hold.is_some());
    drop(start);

    setup
        .settings
        .set(
            "core.streaming",
            serde_json::json!({
                "hosting": false,
                "quality": {"resolution": "1920x1080", "fps": 60, "bitrateKbps": 20000}
            }),
            None,
        )
        .await
        .unwrap();
    // A title's case may differ from the host's.
    let start = setup.prepare("celeste", false).await.unwrap();
    assert_eq!(
        args(&start)[2..],
        [
            "Celeste",
            "--display-mode",
            "fullscreen",
            "--no-quit-after",
            "--resolution",
            "1920x1080",
            "--fps",
            "60",
            "--bitrate",
            "20000"
        ]
    );
    assert_eq!(setup.cancels(), 0);
}

#[test]
fn the_clients_start_window_is_a_pixel_in_the_middle_of_caskades() {
    let at = |x, y, width, height| WindowPlacement {
        x,
        y,
        width,
        height,
    };
    assert_eq!(start_window(&at(0, 0, 1920, 1080)), "1x1+960+540");
    assert_eq!(start_window(&at(-1920, 24, 1600, 900)), "1x1");
    assert_eq!(start_window(&at(1920, 0, 2560, 1440)), "1x1+3200+720");
}

#[tokio::test]
async fn the_clients_lines_read_as_caskades_own_words() {
    let setup = setup().await;
    let _start = setup.prepare("Celeste", false).await.unwrap();
    let read = |line: &str| setup.streams.read(HOST_ID, "Celeste", line);
    assert_eq!(
        read("00:00:06 - Qt Info: Launch response: \"<root status_code=\\\"200\\\">\""),
        ClientLine::Streaming
    );
    assert_eq!(
        read("00:00:11 - Qt Critical: Failed to find application Celeste"),
        ClientLine::Failed("Den no longer offers Celeste.".into())
    );
    assert_eq!(
        read(
            "00:00:00 - Qt Critical: Computer Den has not been paired. Please open Moonlight to pair before streaming."
        ),
        ClientLine::Failed(
            "Den no longer knows this device. Pair them again in Settings → Streaming.".into()
        )
    );
    assert_eq!(
        read(
            "00:00:02 - Qt Critical: Host returned error: Failed to start the specified application (Error 0)"
        ),
        ClientLine::Failed("Den couldn't start Celeste.".into())
    );
    assert_eq!(
        read("00:00:03 - Qt Critical: Something else"),
        ClientLine::Failed("Streaming from Den failed.".into())
    );
    assert_eq!(
        read("00:00:01 - Qt Info: Server GPU: \"\""),
        ClientLine::Other
    );
}

#[tokio::test]
async fn a_host_that_doesnt_know_this_device_asks_to_pair_again() {
    let setup = setup().await;
    *setup.host.forgot.lock().unwrap() = true;
    assert_eq!(
        refusal(setup.prepare("Celeste", false).await),
        "Den no longer knows this device. Pair them again in Settings → Streaming."
    );
    // Not paired here at all.
    setup.write(&identity());
    assert_eq!(
        refusal(setup.prepare("Celeste", false).await),
        "The other device no longer knows this device. Pair them again in Settings → Streaming."
    );
}

#[tokio::test]
async fn a_game_the_host_no_longer_offers_is_named() {
    let setup = setup().await;
    assert_eq!(
        refusal(setup.prepare("Celeste 2", false).await),
        "Den no longer offers Celeste 2."
    );
}

#[tokio::test]
async fn a_busy_host_asks_first_then_stops_its_game() {
    let setup = setup().await;
    *setup.host.playing.lock().unwrap() = Some("882".into());
    let Err(LaunchError::HostBusy(busy)) = setup.prepare("Celeste", false).await else {
        panic!("a busy host should ask first");
    };
    assert_eq!(busy.device, "Den");
    assert_eq!(busy.playing.as_deref(), Some("Hades"));
    assert_eq!(setup.cancels(), 0);
    setup.prepare("Celeste", true).await.unwrap();
    assert_eq!(setup.cancels(), 1);
    assert_eq!(*setup.host.playing.lock().unwrap(), None);

    // Playing this very game: the stream resumes it, no question.
    *setup.host.playing.lock().unwrap() = Some("881".into());
    setup.prepare("Celeste", false).await.unwrap();
    assert_eq!(setup.cancels(), 1);
}

#[tokio::test]
async fn finishing_stops_only_this_streams_game() {
    let setup = setup().await;
    drop(setup.prepare("Celeste", false).await.unwrap());
    *setup.host.playing.lock().unwrap() = Some("882".into());
    setup.streams.finish(HOST_ID, "Celeste").await;
    assert_eq!(setup.cancels(), 0, "another game stays");

    drop(setup.prepare("Celeste", true).await.unwrap());
    assert_eq!(setup.cancels(), 1);
    *setup.host.playing.lock().unwrap() = Some("881".into());
    setup.streams.finish(HOST_ID, "Celeste").await;
    assert_eq!(setup.cancels(), 2);
    assert_eq!(*setup.host.playing.lock().unwrap(), None);
    // Once finished, again is nothing to do.
    setup.streams.finish(HOST_ID, "Celeste").await;
    assert_eq!(setup.cancels(), 2);
}

#[tokio::test]
async fn an_asleep_host_is_woken_and_one_that_cant_be_says_so() {
    let listener = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let setup = setup_with(Some(listener.local_addr().unwrap())).await;
    // Nothing answers at its address.
    setup.record("127.0.0.2", None);
    assert_eq!(
        refusal(setup.prepare("Celeste", false).await),
        "Den isn't answering. Check that it's switched on and on this network."
    );
    setup.record("127.0.0.2", Some(r"@ByteArray(\x12\x34\x56\x78\x9a\xbc)"));
    assert_eq!(
        refusal(setup.prepare("Celeste", false).await),
        "Den didn't wake up. Check that it's plugged in and on this network."
    );
    let mut packet = [0u8; 200];
    let (read, _) = listener.recv_from(&mut packet).await.unwrap();
    assert_eq!(
        packet[..read],
        caskade_core::remote_play::magic_packet([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc])
    );
}

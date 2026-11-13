//! The pairing exchange against a fake host that plays the server's half
//! with its own RSA certificate (the fixtures in this folder), and the
//! client state file's host records.

use super::*;
use crate::testing::{Cert, tls_front};
use std::sync::{Arc, Mutex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate, matchers::path_regex};

const SERVER_CERT: &str = include_str!("server-cert.pem");
const SERVER_KEY: &str = include_str!("server-key.pem");
const CLIENT_CERT: &str = include_str!("client-cert.pem");
const CLIENT_KEY: &str = include_str!("client-key.pem");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Answer,
    Decline,
    /// Nobody enters the code.
    Silent,
}

#[derive(Default)]
struct State {
    key: Option<[u8; 16]>,
    client_cert: String,
    server_secret: Vec<u8>,
    unpaired: bool,
    stages: Vec<String>,
}

/// The host's half of the exchange, with the code an admin entered.
#[derive(Clone)]
struct FakeHost {
    pin: &'static str,
    mode: Mode,
    state: Arc<Mutex<State>>,
}

fn xml(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><root status_code=\"200\">{body}</root>"
    ))
}

impl Respond for FakeHost {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let query: std::collections::HashMap<String, String> =
            request.url.query_pairs().into_owned().collect();
        let mut state = self.state.lock().unwrap();
        if request.url.path() == "/unpair" {
            state.unpaired = true;
            return xml("");
        }
        if let Some(phrase) = query.get("phrase") {
            state.stages.push(phrase.clone());
            if phrase == "pairchallenge" {
                return xml("<paired>1</paired>");
            }
            match self.mode {
                Mode::Decline => {
                    return ResponseTemplate::new(400)
                        .set_body_string("Pairing request cancelled by operator");
                }
                Mode::Silent => {
                    return xml("<paired>1</paired>").set_delay(std::time::Duration::from_secs(30));
                }
                Mode::Answer => {}
            }
            let salt = unhex(&query["salt"]).unwrap();
            state.key = Some(aes_key(&salt, self.pin));
            state.client_cert = String::from_utf8(unhex(&query["clientcert"]).unwrap()).unwrap();
            return xml(&format!(
                "<paired>1</paired><plaincert>{}</plaincert>",
                hex(SERVER_CERT.as_bytes())
            ));
        }
        let key = state.key.unwrap();
        let (server_signature, _) = signature_and_key(&der(SERVER_CERT).unwrap()).unwrap();
        if let Some(challenge) = query.get("clientchallenge") {
            state.stages.push("clientchallenge".into());
            let challenge = crypt(&key, &unhex(challenge).unwrap(), false).unwrap();
            let secret: [u8; 16] = random().unwrap();
            state.server_secret = secret.to_vec();
            let response = Sha256::digest([&challenge[..], &server_signature, &secret].concat());
            let server_challenge: [u8; 16] = random().unwrap();
            let answer = crypt(&key, &[&response[..], &server_challenge].concat(), true).unwrap();
            return xml(&format!(
                "<paired>1</paired><challengeresponse>{}</challengeresponse>",
                hex(&answer)
            ));
        }
        if query.contains_key("serverchallengeresp") {
            state.stages.push("serverchallengeresp".into());
            let mut secret = state.server_secret.clone();
            secret.extend(sign(SERVER_KEY, &state.server_secret).unwrap());
            return xml(&format!(
                "<paired>1</paired><pairingsecret>{}</pairingsecret>",
                hex(&secret)
            ));
        }
        if let Some(secret) = query.get("clientpairingsecret") {
            state.stages.push("clientpairingsecret".into());
            let secret = unhex(secret).unwrap();
            let (_, client_key) = signature_and_key(&der(&state.client_cert).unwrap()).unwrap();
            let ok = verify(&client_key, &secret[..16], &secret[16..]);
            return xml(if ok {
                "<paired>1</paired>"
            } else {
                "<paired>0</paired>"
            });
        }
        ResponseTemplate::new(404)
    }
}

async fn host(pin: &'static str, mode: Mode) -> (FakeHost, Endpoint, MockServer) {
    let fake = FakeHost {
        pin,
        mode,
        state: Arc::default(),
    };
    let server = MockServer::start().await;
    Mock::given(path_regex("^/(pair|unpair)$"))
        .respond_with(fake.clone())
        .mount(&server)
        .await;
    let front = tls_front(*server.address(), &Cert::from_pem(SERVER_CERT, SERVER_KEY)).await;
    let endpoint = Endpoint {
        http: format!("{}/", server.uri()).parse().unwrap(),
        https: format!("https://{front}/").parse().unwrap(),
    };
    (fake, endpoint, server)
}

fn identity() -> Identity {
    Identity {
        unique_id: "37460f2e7934c368".into(),
        cert_pem: CLIENT_CERT.into(),
        key_pem: CLIENT_KEY.into(),
    }
}

async fn pair(endpoint: &Endpoint, pin: &str, wait: Duration) -> Paired {
    Pairing {
        endpoint,
        identity: &identity(),
        pin,
        wait,
        device_name: "Living room",
    }
    .run()
    .await
    .unwrap()
}

#[tokio::test]
async fn the_right_code_pairs_through_every_stage() {
    let (fake, endpoint, _server) = host("4831", Mode::Answer).await;
    let paired = pair(&endpoint, "4831", Duration::from_secs(5)).await;
    assert_eq!(
        paired,
        Paired::Paired {
            server_cert_pem: SERVER_CERT.into()
        }
    );
    let state = fake.state.lock().unwrap();
    assert_eq!(
        state.stages,
        [
            "getservercert",
            "clientchallenge",
            "serverchallengeresp",
            "clientpairingsecret",
            "pairchallenge"
        ]
    );
    assert_eq!(state.client_cert, CLIENT_CERT);
    assert!(!state.unpaired);
}

#[tokio::test]
async fn a_wrong_code_is_said_and_the_host_forgets_the_attempt() {
    let (fake, endpoint, _server) = host("4831", Mode::Answer).await;
    assert_eq!(
        pair(&endpoint, "1111", Duration::from_secs(5)).await,
        Paired::WrongCode
    );
    assert!(fake.state.lock().unwrap().unpaired);
    // A new attempt with the right code works.
    let (_, endpoint, _server) = host("4831", Mode::Answer).await;
    assert!(matches!(
        pair(&endpoint, "4831", Duration::from_secs(5)).await,
        Paired::Paired { .. }
    ));
}

#[tokio::test]
async fn a_declined_or_unanswered_request_pairs_nothing() {
    let (_, endpoint, _server) = host("4831", Mode::Decline).await;
    assert_eq!(
        pair(&endpoint, "4831", Duration::from_secs(5)).await,
        Paired::Declined
    );
    let (_, endpoint, _server) = host("4831", Mode::Silent).await;
    assert_eq!(
        pair(&endpoint, "4831", Duration::from_millis(300)).await,
        Paired::Expired
    );
}

#[test]
fn codes_are_four_digits() {
    for _ in 0..50 {
        let code = new_code().unwrap();
        assert_eq!(code.len(), 4);
        assert!(code.bytes().all(|b| b.is_ascii_digit()));
    }
}

#[test]
fn moonlights_identity_and_hosts_round_trip_through_its_settings() {
    let settings = format!(
        "[General]\ncertificate={}\nkey={}\nlatestsupportedversion-v1=99.99.99.99\nuniqueid=37460f2e7934c368\n",
        qt_byte_array(CLIENT_CERT),
        qt_byte_array(CLIENT_KEY)
    );
    // As Moonlight writes it: escaped newlines inside one line.
    assert!(settings.contains("certificate=\"@ByteArray(-----BEGIN CERTIFICATE-----\\nMII"));
    assert_eq!(Identity::from_settings(&settings), Some(identity()));
    assert_eq!(Identity::from_settings("[General]\nuniqueid=x\n"), None);

    let den = HostRecord {
        uuid: "A466B6A4-3EC7-8C2E-629E-021AC1F5E209".into(),
        name: "Den".into(),
        address: "192.168.1.7".into(),
        server_cert_pem: SERVER_CERT.into(),
    };
    let with_den = add_host(&settings, &den);
    let study = HostRecord {
        uuid: "B1".into(),
        name: "Study".into(),
        address: "192.168.1.8".into(),
        ..den.clone()
    };
    let both = add_host(&with_den, &study);
    // Pairing again replaces the entry rather than adding another.
    let again = add_host(&both, &den);
    let hosts = crate::hosts::paired_hosts(&again);
    let names: Vec<&str> = hosts.iter().map(|host| host.name.as_str()).collect();
    assert_eq!(names, ["Study", "Den"]);
    assert_eq!(hosts[1].address.as_deref(), Some("192.168.1.7"));
    assert!(again.contains("size=2"));
    let srvcert = again
        .lines()
        .find_map(|line| line.strip_prefix("2\\srvcert="))
        .unwrap();
    assert_eq!(qt_text(srvcert).as_deref(), Some(SERVER_CERT));
    // Moonlight's own lines stay.
    assert!(again.contains("latestsupportedversion-v1=99.99.99.99"));
    assert_eq!(Identity::from_settings(&again), Some(identity()));

    // Forgetting one renumbers the rest.
    let forgotten = forget_host(&again, "b1");
    let hosts = crate::hosts::paired_hosts(&forgotten);
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "Den");
    assert!(forgotten.contains("1\\uuid=A466B6A4") && forgotten.contains("size=1"));
}

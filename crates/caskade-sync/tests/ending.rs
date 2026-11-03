//! Ending a session together (#39): the close never discards a last message
//! the peer hasn't read yet, and a peer that doesn't end its side the same
//! way (an older Caskade, a crash) costs at most `END_WAIT`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use caskade_sync::{
    Admission, Config, END_WAIT, EndpointId, Grant, Purpose, Session, Sessions, SyncEndpoint,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Note {
    n: u32,
}

/// Admits its one peer for sync.
struct One(Mutex<Option<EndpointId>>, Grant);

impl Admission for One {
    fn admits(&self, peer: EndpointId, purpose: Purpose, grant: Option<&Grant>) -> bool {
        *self.0.lock().unwrap() == Some(peer) && purpose == Purpose::Sync && grant == Some(&self.1)
    }
}

/// Hands each accepted session to the test.
struct Handover(mpsc::UnboundedSender<Session>);

#[async_trait]
impl Sessions for Handover {
    async fn opened(&self, session: Session) {
        let _ = self.0.send(session);
    }
}

struct Nobody;

impl Admission for Nobody {
    fn admits(&self, _: EndpointId, _: Purpose, _: Option<&Grant>) -> bool {
        false
    }
}

/// Two endpoints and a session between them: (client's, server's).
async fn sessions() -> (SyncEndpoint, SyncEndpoint, Session, Session) {
    let grant = Grant::generate();
    let admission = Arc::new(One(Mutex::new(None), grant));
    let (tx, mut accepted) = mpsc::unbounded_channel();
    let server = SyncEndpoint::bind(
        Config::loopback(),
        admission.clone(),
        Arc::new(Handover(tx)),
    )
    .await
    .unwrap();
    let (unused, _) = mpsc::unbounded_channel();
    let client = SyncEndpoint::bind(
        Config::loopback(),
        Arc::new(Nobody),
        Arc::new(Handover(unused)),
    )
    .await
    .unwrap();
    *admission.0.lock().unwrap() = Some(client.id());
    let dialled = client
        .connect(server.addr(), Purpose::Sync, Some(grant))
        .await
        .unwrap();
    let answered = accepted.recv().await.unwrap();
    (client, server, dialled, answered)
}

/// Writes one note, then ends the session; the time `end` took.
fn say_and_end(mut session: Session) -> tokio::task::JoinHandle<Duration> {
    tokio::spawn(async move {
        session.writer().write(&Note { n: 7 }).await.unwrap();
        let started = Instant::now();
        session.end().await;
        started.elapsed()
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peer_still_answering_is_not_cut_off() {
    // As in a pairing that one side cancels while the other confirms: the
    // other side's answer is still on its way when this side ends.
    let (client, server, mut late, ending) = sessions().await;
    let ended = say_and_end(ending);
    tokio::time::sleep(Duration::from_millis(500)).await;
    late.writer()
        .write(&Note { n: 8 })
        .await
        .expect("the side that ended hasn't closed under the answer");
    let note: Note = late.reader().read(64).await.unwrap();
    assert_eq!(note, Note { n: 7 });
    late.end().await;
    assert!(
        ended.await.unwrap() < END_WAIT,
        "it waited for the peer, not the bound"
    );
    client.shutdown().await.unwrap();
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peer_that_closes_without_ending_its_side_costs_nothing() {
    // As a Caskade from before #39 can: it reads, then closes the connection.
    let (client, server, mut abrupt, ending) = sessions().await;
    let ended = say_and_end(ending);
    let note: Note = abrupt.reader().read(64).await.unwrap();
    assert_eq!(note, Note { n: 7 });
    abrupt.close();
    let took = ended.await.unwrap();
    assert!(
        took < Duration::from_secs(1),
        "the close ended the wait: {took:?}"
    );
    client.shutdown().await.unwrap();
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peer_that_never_ends_costs_at_most_the_bound() {
    let (client, server, mut silent, ending) = sessions().await;
    let ended = say_and_end(ending);
    let note: Note = silent.reader().read(64).await.unwrap();
    assert_eq!(note, Note { n: 7 });
    // Holds the session open and says nothing more.
    let took = ended.await.unwrap();
    assert!(
        took >= END_WAIT && took < END_WAIT + Duration::from_secs(1),
        "{took:?}"
    );
    drop(silent);
    client.shutdown().await.unwrap();
    server.shutdown().await.unwrap();
}

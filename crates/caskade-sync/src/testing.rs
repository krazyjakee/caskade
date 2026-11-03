//! For tests (`test-utils`): a relay on this machine, so relay paths are
//! tested without n0's servers or the internet (#250).

use crate::endpoint::Relays;

/// A local relay for [`crate::Reach::relays`]; it stops when the second
/// value is dropped.
pub async fn local_relay() -> (Relays, impl Sized) {
    let (map, _url, server) = iroh::test_utils::run_relay_server()
        .await
        .expect("a local relay starts");
    (Relays::Local(map), server)
}

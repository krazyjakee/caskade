use std::time::Duration;

use iroh::EndpointId;
use iroh::endpoint::{BindError, ConnectError};
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What can go wrong reaching another device and agreeing what a connection
/// is about.
///
/// No variant quotes bytes a peer sent or a grant: the first is text a
/// stranger controls, the second a credential.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("could not start the sync endpoint on this device")]
    Bind {
        #[source]
        source: BindError,
    },

    /// The device couldn't be reached: no path, no relay allowed, or it isn't
    /// listening.
    #[error("could not reach device {peer}")]
    Dial {
        peer: EndpointId,
        #[source]
        source: ConnectError,
    },

    /// The other device refused the connection.
    ///
    /// Carries nothing on purpose: the refusal on the wire is the same bytes
    /// whatever the reason (unknown device, wrong or missing grant, a used
    /// link, a forgotten device), so there is nothing to carry.
    #[error("the other device refused the connection")]
    Refused,

    /// A peer didn't finish its part of the exchange in time.
    #[error("the other device didn't answer within {within:?}")]
    TimedOut { within: Duration },

    /// Bytes arrived that aren't this protocol: too long, not the expected
    /// JSON, or a stream that ended mid-message.
    #[error("the other device sent something that isn't caskade/sync/1")]
    Malformed,

    /// The connection or a stream failed underneath the exchange.
    #[error("the connection to the other device failed")]
    Interrupted {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The OS keychain didn't answer, or refused to store the device key.
    #[error("the system keychain is unavailable")]
    Keychain,

    /// Pairing ended because someone cancelled it, on either device.
    #[error("pairing was cancelled")]
    Cancelled,

    /// The pairing link has expired.
    #[error("this pairing link has expired; make a new one on the other device")]
    LinkExpired,

    /// The device refused the pairing link: it was used already, it expired,
    /// or the device no longer offers it (the refusal doesn't say which).
    #[error(
        "this pairing link was already used or has expired; make a new one on the other device"
    )]
    LinkRefused,

    /// The text isn't a Caskade pairing link.
    #[error("that isn't a Caskade pairing link")]
    NotALink,

    /// The endpoint's accept loop didn't wind down cleanly.
    #[error("the sync endpoint did not shut down cleanly")]
    Shutdown {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl Error {
    pub(crate) fn interrupted<E: std::error::Error + Send + Sync + 'static>(source: E) -> Self {
        Self::Interrupted {
            source: Box::new(source),
        }
    }
}

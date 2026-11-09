//! caskade-core's test cores (`cores`), each with a share service set up
//! before its endpoint starts, as the app does.

use caskade_core::devices::Reach;
use caskade_share::Shares;

use crate::cores::{self, Setup, core_serving};

/// A test core with one-off shares, both ways (#246, #248).
pub type Core = cores::Core<Shares>;

impl Core {
    pub fn shares(&self) -> &Shares {
        &self.extra
    }
}

/// A device on loopback named `name`, with Hades (Steam) discovered.
pub async fn core(name: &str) -> Core {
    core_with(name, Reach::loopback()).await
}

/// A device named `name` reached as `reach` says, with Hades (Steam)
/// discovered.
pub async fn core_with(name: &str, reach: Reach) -> Core {
    core_serving(name, reach, shares).await
}

fn shares(setup: Setup<'_>) -> Shares {
    Shares::new(
        setup.store.clone(),
        setup.devices.clone(),
        setup.transfers.clone(),
        setup.install.clone(),
        setup.cache,
    )
}

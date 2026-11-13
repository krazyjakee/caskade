//! What the running host's web API does for Caskade: pairing requests,
//! paired devices, and the API client itself.

use super::{StreamingHost, not_working};
use crate::api::{HostApi, PairRequest, PairedClient};
use caskade_core::{Error, Result};

/// The API client [`StreamingHost::api`] keeps.
#[derive(Default)]
pub(super) struct KeptApi {
    pub(super) api: Option<HostApi>,
    /// Counts the times it was forgotten.
    pub(super) generation: u64,
}

impl StreamingHost {
    /// This host's streaming id, as clients know it: its `/serverinfo`
    /// `uniqueid`. `None` when it isn't answering.
    pub async fn host_id(&self) -> Option<String> {
        crate::hosts::ask_server_info("127.0.0.1", self.inner.http_port)
            .await
            .map(|info| info.id)
    }

    /// Devices waiting to pair (#263); expired ones are gone from the list.
    pub async fn pair_requests(&self) -> Result<Vec<PairRequest>> {
        self.checked(self.api().await?.pair_requests().await)
    }

    /// Enters the code device `id` shows, naming it `name`.
    pub async fn accept_pairing(&self, id: &str, pin: &str, name: &str) -> Result<()> {
        let pin = pin.trim();
        if pin.len() != 4 || !pin.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Invalid(caskade_core::tr!("The code is four digits")));
        }
        self.checked(self.api().await?.accept_pairing(id, pin, name.trim()).await)
    }

    pub async fn decline_pairing(&self, id: &str) -> Result<()> {
        self.checked(self.api().await?.decline_pairing(id).await)
    }

    /// The devices paired with this host.
    pub async fn paired_devices(&self) -> Result<Vec<PairedClient>> {
        self.checked(self.api().await?.paired_clients().await)
    }

    pub async fn unpair(&self, uuid: &str) -> Result<()> {
        self.checked(self.api().await?.unpair(uuid).await)
    }

    /// Forgets every paired device: each pairs again with a code.
    pub async fn unpair_all(&self) -> Result<()> {
        self.checked(self.api().await?.unpair_all().await)
    }

    /// The app list file of Caskade's own Linux host, which the publisher
    /// writes whole ([`crate::publish::sync_whole`]); `None` on Windows,
    /// where it is in the administrator-only folder, and for a user's own
    /// host, whose other apps aren't Caskade's.
    pub(crate) async fn own_app_list(&self) -> Option<std::path::PathBuf> {
        if self.inner.os == caskade_core::emulation::OperatingSystem::Windows
            || self.existing_mode().await
        {
            return None;
        }
        self.running_files().ok().map(|files| files.apps())
    }

    /// The running host's API (publishing games, pairing).
    /// Made once and kept until hosting changes or a call through it fails:
    /// making it reads the keychain and the certificate.
    pub async fn api(&self) -> Result<HostApi> {
        let generation = {
            let kept = caskade_core::lock(&self.inner.api);
            if let Some(api) = &kept.api {
                return Ok(api.clone());
            }
            kept.generation
        };
        let (credentials, cert) = if self.existing_mode().await {
            let cert = self
                .existing()
                .await
                .and_then(|existing| existing.cert)
                .ok_or_else(|| Error::Invalid(not_working()))?;
            (self.inner.secrets.existing().await?, cert)
        } else {
            (
                self.inner.secrets.get().await?,
                self.running_files()?.cert(),
            )
        };
        let credentials = credentials.ok_or_else(|| Error::Invalid(not_working()))?;
        let api = HostApi::with_cert_file(self.inner.api_base.clone(), credentials, &cert)?;
        let mut kept = caskade_core::lock(&self.inner.api);
        // Hosting changed meanwhile: this one may be for the old host.
        if kept.generation == generation {
            kept.api = Some(api.clone());
        }
        Ok(api)
    }

    /// `result`, with the kept API forgotten when it failed: the host may
    /// have new credentials or a new certificate.
    pub(super) fn checked<T>(&self, result: Result<T>) -> Result<T> {
        if result.is_err() {
            self.forget_api();
        }
        result
    }
}

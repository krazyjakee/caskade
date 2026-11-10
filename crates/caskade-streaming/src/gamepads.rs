//! The gamepad driver a Windows host needs (#260):
//! ViGEmBus 1.22.0, downloaded unelevated into `<data>\streaming\drivers`
//! and kept only when it matches the pinned SHA-256. The administrator step
//! copies it into the administrator-only folder, checks it again there and
//! installs it silently (`elevation::windows::install_gamepad_driver`);
//! an install that was there before is left as it is.

use crate::elevation::windows::{
    GAMEPAD_DRIVER_SHA256, GAMEPAD_DRIVER_URL, gamepad_driver_download,
};
use caskade_core::{Error, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether `bytes` are the pinned installer.
pub fn genuine(bytes: &[u8]) -> bool {
    hex(&Sha256::digest(bytes)) == GAMEPAD_DRIVER_SHA256
}

/// Downloads the installer beside the host's staging folder, unless a
/// genuine copy is already there.
pub async fn fetch(staging: &Path) -> Result<()> {
    fetch_from(GAMEPAD_DRIVER_URL, staging).await
}

/// [`fetch`] from `url` (tests).
pub async fn fetch_from(url: &str, staging: &Path) -> Result<()> {
    let file = gamepad_driver_download(staging);
    if tokio::fs::read(&file)
        .await
        .is_ok_and(|bytes| genuine(&bytes))
    {
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(caskade_core::error::net)?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(caskade_core::error::net)?
        .error_for_status()
        .map_err(caskade_core::error::net)?;
    let bytes = response.bytes().await.map_err(caskade_core::error::net)?;
    if !genuine(&bytes) {
        return Err(Error::Invalid(caskade_core::tr!(
            "the controller driver's download wasn't the expected file"
        )));
    }
    if let Some(dir) = file.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let part = file.with_extension("part");
    tokio::fs::write(&part, &bytes).await?;
    tokio::fs::rename(&part, &file).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[tokio::test]
    async fn only_the_pinned_installer_is_kept() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("data/streaming/host");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/driver.exe"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not the driver".to_vec()))
            .mount(&server)
            .await;
        let url = format!("{}/driver.exe", server.uri());
        assert!(fetch_from(&url, &staging).await.is_err());
        let file = gamepad_driver_download(&staging);
        assert_eq!(
            file,
            temp.path()
                .join("data/streaming/drivers/ViGEmBus_1.22.0_x64_x86_arm64.exe")
        );
        assert!(!file.exists());
        assert!(!genuine(b"not the driver"));
    }
}

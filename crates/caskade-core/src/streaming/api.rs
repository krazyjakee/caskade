//! The host's web API (STREAMING-PLAN §4.2): HTTPS to `localhost:47990`
//! with basic auth, pinned to the certificate the host made
//! (`credentials/cacert.pem`). Nothing else is trusted, not the system's
//! roots and not another certificate on the same port. Requests carry no
//! `Origin` (Sunshine's CSRF check applies only to browsers) and JSON.

use super::secrets::Credentials;
use crate::{Error, Result};
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::CryptoProvider,
    pki_types::{CertificateDer, ServerName, UnixTime, pem::PemObject},
};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc, time::Duration};
use url::Url;

/// Where Caskade's own host answers.
pub const LOCAL: &str = "https://127.0.0.1:47990/";

/// Trusts exactly one certificate: the host's own.
#[derive(Debug)]
struct Pinned {
    cert: CertificateDer<'static>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        // Its own certificate names no host (CN "Sunshine Gamestream Host"),
        // so it is matched as bytes, not by name or chain.
        if end_entity.as_ref() == self.cert.as_ref() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// An app as the host lists it. Fields Caskade doesn't use stay as they
/// were (`rest`), so another app read and listed is never changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl App {
    /// An app that runs `cmd` and lasts as long as it does: Sunshine waits
    /// for every process it started (§4.3).
    pub fn command(name: &str, cmd: &str) -> Self {
        let rest = serde_json::json!({
            "output": "",
            "exclude-global-prep-cmd": false,
            "elevated": false,
            "auto-detach": false,
            "wait-all": true,
            "exit-timeout": 5,
            "prep-cmd": [],
            "detached": [],
            "image-path": "",
        });
        Self {
            name: name.to_owned(),
            cmd: Some(cmd.to_owned()),
            rest: rest.as_object().cloned().unwrap_or_default(),
        }
    }
}

#[derive(Deserialize)]
struct Apps {
    apps: Vec<App>,
}

#[derive(Deserialize)]
struct Config {
    version: Option<String>,
}

/// A client for one host's web API.
#[derive(Clone)]
pub struct HostApi {
    base: Url,
    credentials: Credentials,
    client: reqwest::Client,
}

impl std::fmt::Debug for HostApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostApi").field("base", &self.base).finish()
    }
}

fn net(error: reqwest::Error) -> Error {
    crate::error::net(error)
}

impl HostApi {
    /// A client for `base` that trusts only `cert` (DER).
    pub fn new(base: Url, credentials: Credentials, cert: CertificateDer<'static>) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|error| Error::Invalid(error.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pinned { cert, provider }))
            .with_no_client_auth();
        let client = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(net)?;
        Ok(Self {
            base,
            credentials,
            client,
        })
    }

    /// A client pinned to the PEM certificate at `cert`.
    pub fn with_cert_file(base: Url, credentials: Credentials, cert: &Path) -> Result<Self> {
        let cert = CertificateDer::from_pem_file(cert).map_err(|error| {
            Error::Invalid(crate::tr!(
                "the streaming host's certificate is unreadable: {error}",
                error = error.to_string()
            ))
        })?;
        Self::new(base, credentials, cert)
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.base
            .join(path)
            .map_err(|error| Error::Invalid(error.to_string()))
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let response = request
            .basic_auth(&self.credentials.user, Some(&self.credentials.password))
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(net)?;
        match response.status() {
            status if status.is_success() => Ok(response),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => Err(
                Error::Forbidden(crate::tr!("the streaming host refused Caskade's sign-in")),
            ),
            status => Err(Error::Invalid(crate::tr!(
                "the streaming host answered {status}",
                status = status.to_string()
            ))),
        }
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self.send(self.client.get(self.url(path)?)).await?;
        let body = response.bytes().await.map_err(net)?;
        serde_json::from_slice(&body).map_err(|error| Error::Invalid(error.to_string()))
    }

    /// The host's version (`GET /api/config`).
    pub async fn version(&self) -> Result<String> {
        let config: Config = self.json("api/config").await?;
        config
            .version
            .ok_or_else(|| Error::Invalid(crate::tr!("the streaming host didn't say its version")))
    }

    /// Every app, in the host's order: an app's index is its place here.
    pub async fn apps(&self) -> Result<Vec<App>> {
        Ok(self.json::<Apps>("api/apps").await?.apps)
    }

    /// Adds `app` at the end. The host sorts its list by name, so read it
    /// again before using an index (§4.3).
    pub async fn add_app(&self, app: &App) -> Result<()> {
        let mut body = serde_json::to_value(app)?;
        body["index"] = serde_json::json!(-1);
        self.send(
            self.client
                .post(self.url("api/apps")?)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(serde_json::to_vec(&body)?),
        )
        .await?;
        Ok(())
    }

    pub async fn delete_app(&self, index: usize) -> Result<()> {
        self.send(self.client.delete(self.url(&format!("api/apps/{index}"))?))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

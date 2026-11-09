//! Pairing a client with a host (#263) without any of
//! Moonlight's windows: Caskade speaks the GameStream pairing exchange
//! itself, with the identity (certificate, key, unique id) Moonlight keeps
//! in Caskade's client state, then records the host there as Moonlight
//! does, so Moonlight streams from it as paired.
//!
//! The exchange, against the host's HTTP port and then its HTTPS one:
//! `getservercert` (with a salt and the client's certificate; the host
//! answers once an admin entered the code), `clientchallenge`,
//! `serverchallengeresp`, `clientpairingsecret` and `pairchallenge`. The
//! code and salt make an AES-128 key (SHA-256 for hosts of generation 7
//! and later); each side proves it knows it, and signs a secret with its
//! certificate's key. A mismatch is a wrong code.

use super::api;
use aes::{
    Aes128,
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray},
};
use caskade_core::{Error, Result};
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{self, RsaKeyPair},
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject};
use sha2::{Digest, Sha256};
use std::time::Duration;

/// Moonlight's own identity, from its state file's `[General]`.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    pub unique_id: String,
    pub cert_pem: String,
    pub key_pem: String,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("unique_id", &self.unique_id)
            .finish_non_exhaustive()
    }
}

/// A Qt settings `@ByteArray(...)` value, its escapes undone.
pub fn qt_bytes(value: &str) -> Option<String> {
    let inner = value
        .strip_prefix("\"@ByteArray(")
        .and_then(|v| v.strip_suffix(")\""))
        .or_else(|| {
            value
                .strip_prefix("@ByteArray(")
                .and_then(|v| v.strip_suffix(')'))
        })?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('0') => out.push('\0'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    Some(out)
}

/// `text` as a quoted Qt settings `@ByteArray(...)` value.
pub fn qt_byte_array(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"@ByteArray({escaped})\"")
}

impl Identity {
    /// From a client state file; `None` until Moonlight made one.
    pub fn from_settings(settings: &str) -> Option<Self> {
        let mut general = false;
        let (mut unique_id, mut cert_pem, mut key_pem) = (None, None, None);
        for line in settings.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                general = line == "[General]";
                continue;
            }
            let Some((key, value)) = line.split_once('=').filter(|_| general) else {
                continue;
            };
            match key {
                "uniqueid" => unique_id = Some(value.to_owned()),
                "certificate" => cert_pem = qt_bytes(value),
                "key" => key_pem = qt_bytes(value),
                _ => {}
            }
        }
        Some(Self {
            unique_id: unique_id?,
            cert_pem: cert_pem?,
            key_pem: key_pem?,
        })
    }
}

/// How a pairing ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Paired {
    /// Paired: the host's certificate.
    Paired { server_cert_pem: String },
    /// The code entered on the host wasn't this one.
    WrongCode,
    /// The host declined the request.
    Declined,
    /// Nobody entered the code before the request expired.
    Expired,
}

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::Invalid(error.to_string())
}

fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(xml[start..end].trim())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0_u8; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| invalid("no randomness"))?;
    Ok(bytes)
}

/// AES-128 in ECB over whole blocks, as GameStream uses it.
fn crypt(key: &[u8; 16], data: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    if !data.len().is_multiple_of(16) {
        return Err(invalid("not whole AES blocks"));
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut out = data.to_vec();
    for block in out.chunks_mut(16) {
        let block = GenericArray::from_mut_slice(block);
        if encrypt {
            cipher.encrypt_block(block);
        } else {
            cipher.decrypt_block(block);
        }
    }
    Ok(out)
}

/// The key the code and salt make.
pub fn aes_key(salt: &[u8], pin: &str) -> [u8; 16] {
    let digest = Sha256::digest([salt, pin.as_bytes()].concat());
    let mut key = [0_u8; 16];
    key.copy_from_slice(&digest[..16]);
    key
}

/// A PEM certificate's DER.
fn der(pem: &str) -> Result<CertificateDer<'static>> {
    CertificateDer::from_pem_slice(pem.as_bytes()).map_err(invalid)
}

/// A certificate's signature, and its public key (RSA).
pub fn signature_and_key(der: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let (_, cert) = x509_parser::parse_x509_certificate(der).map_err(invalid)?;
    Ok((
        cert.signature_value.data.to_vec(),
        cert.public_key().subject_public_key.data.to_vec(),
    ))
}

fn sign(key_pem: &str, message: &[u8]) -> Result<Vec<u8>> {
    let key = PrivatePkcs8KeyDer::from_pem_slice(key_pem.as_bytes()).map_err(invalid)?;
    let pair = RsaKeyPair::from_pkcs8(key.secret_pkcs8_der()).map_err(invalid)?;
    let mut signed = vec![0_u8; pair.public().modulus_len()];
    pair.sign(
        &signature::RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        message,
        &mut signed,
    )
    .map_err(invalid)?;
    Ok(signed)
}

fn verify(public_key: &[u8], message: &[u8], signed: &[u8]) -> bool {
    signature::UnparsedPublicKey::new(&signature::RSA_PKCS1_2048_8192_SHA256, public_key)
        .verify(message, signed)
        .is_ok()
}

/// Where a host answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// `http://host:47989/`.
    pub http: url::Url,
    /// `https://host:47984/`.
    pub https: url::Url,
}

impl Endpoint {
    pub fn new(address: &str) -> Result<Self> {
        let host = match address.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V6(ip)) => format!("[{ip}]"),
            _ => address.to_owned(),
        };
        Ok(Self {
            http: format!("http://{host}:47989/").parse().map_err(invalid)?,
            https: format!("https://{host}:47984/").parse().map_err(invalid)?,
        })
    }
}

/// One pairing exchange.
pub struct Pairing<'a> {
    pub endpoint: &'a Endpoint,
    pub identity: &'a Identity,
    pub pin: &'a str,
    /// How long the host may take to have the code entered: its requests
    /// expire after five minutes.
    pub wait: Duration,
    /// What the host shows the request as ([`device_name`]): this device's
    /// name. A paired device hands the code over by it (#267).
    pub device_name: &'a str,
}

/// The OS's name for this computer.
pub fn this_host_name() -> Option<String> {
    sysinfo::System::host_name()
}

/// A device name as a pairing request carries it: printable, at most 64
/// characters, never empty.
pub fn device_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>()
        .trim()
        .to_owned();
    if name.is_empty() {
        "Caskade".to_owned()
    } else {
        name
    }
}

impl Pairing<'_> {
    fn params(&self, extra: &[(&str, String)]) -> Vec<(String, String)> {
        let mut params = vec![
            ("uniqueid".to_owned(), self.identity.unique_id.clone()),
            ("uuid".to_owned(), uuid::Uuid::new_v4().simple().to_string()),
            ("devicename".to_owned(), device_name(self.device_name)),
            ("updateState".to_owned(), "1".to_owned()),
        ];
        params.extend(
            extra
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone())),
        );
        params
    }

    async fn ask(
        &self,
        client: &reqwest::Client,
        base: &url::Url,
        extra: &[(&str, String)],
    ) -> Result<String> {
        let url = base.join("pair").map_err(invalid)?;
        let response = client
            .get(url)
            .query(&self.params(extra))
            .send()
            .await
            .map_err(caskade_core::error::net)?;
        let status = response.status();
        let body = response.text().await.map_err(caskade_core::error::net)?;
        if !status.is_success() {
            return Err(Error::Invalid(format!("{status}: {body}")));
        }
        Ok(body)
    }

    /// Runs the exchange.
    pub async fn run(&self) -> Result<Paired> {
        // A connection for each stage: the host closes idle ones, and a
        // reused one fails mid-exchange.
        let plain = reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(caskade_core::error::net)?;
        let salt: [u8; 16] = random()?;
        let key = aes_key(&salt, self.pin);
        // 1: the host's certificate, once someone entered the code there.
        let first = tokio::time::timeout(
            self.wait,
            self.ask(
                &plain,
                &self.endpoint.http,
                &[
                    ("phrase", "getservercert".into()),
                    ("salt", hex(&salt)),
                    ("clientcert", hex(self.identity.cert_pem.as_bytes())),
                ],
            ),
        )
        .await;
        let first = match first {
            Err(_) => return Ok(Paired::Expired),
            Ok(Err(error)) if error.to_string().contains("400") => return Ok(Paired::Declined),
            Ok(answer) => answer?,
        };
        let Some(server_pem) = tag(&first, "plaincert")
            .filter(|_| tag(&first, "paired") == Some("1"))
            .and_then(unhex)
            .and_then(|bytes| String::from_utf8(bytes).ok())
        else {
            return Ok(Paired::Declined);
        };
        let server_der = der(&server_pem)?;
        let (server_signature, server_key) = signature_and_key(&server_der)?;
        let (client_signature, _) = signature_and_key(&der(&self.identity.cert_pem)?)?;

        // 2: our challenge; the host proves it has the key.
        let challenge: [u8; 16] = random()?;
        let answer = self
            .ask(
                &plain,
                &self.endpoint.http,
                &[("clientchallenge", hex(&crypt(&key, &challenge, true)?))],
            )
            .await?;
        let response = tag(&answer, "challengeresponse")
            .and_then(unhex)
            .ok_or_else(|| invalid("the host's challenge answer is unreadable"))?;
        let response = crypt(&key, &response, false)?;
        if response.len() < 48 {
            return Err(invalid("the host's challenge answer is too short"));
        }
        let (server_response, server_challenge) = (&response[..32], &response[32..48]);

        // 3: we prove we have the key.
        let client_secret: [u8; 16] = random()?;
        let proof = Sha256::digest([server_challenge, &client_signature, &client_secret].concat());
        let answer = self
            .ask(
                &plain,
                &self.endpoint.http,
                &[("serverchallengeresp", hex(&crypt(&key, &proof, true)?))],
            )
            .await?;
        let secret = tag(&answer, "pairingsecret")
            .and_then(unhex)
            .ok_or_else(|| invalid("the host's pairing secret is unreadable"))?;
        if secret.len() <= 16 {
            return Err(invalid("the host's pairing secret is too short"));
        }
        let (server_secret, server_signed) = secret.split_at(16);
        if !verify(&server_key, server_secret, server_signed) {
            self.unpair(&plain).await;
            return Err(invalid(caskade_core::tr!(
                "the device that answered isn't the one asked to pair"
            )));
        }
        let expected = Sha256::digest([&challenge[..], &server_signature, server_secret].concat());
        if expected.as_slice() != server_response {
            self.unpair(&plain).await;
            return Ok(Paired::WrongCode);
        }

        // 4: our signed secret.
        let mut pairing_secret = client_secret.to_vec();
        pairing_secret.extend(sign(&self.identity.key_pem, &client_secret)?);
        let answer = self
            .ask(
                &plain,
                &self.endpoint.http,
                &[("clientpairingsecret", hex(&pairing_secret))],
            )
            .await?;
        if tag(&answer, "paired") != Some("1") {
            self.unpair(&plain).await;
            return Ok(Paired::WrongCode);
        }

        // 5: the same over TLS, each side with its certificate.
        let tls = api::pinned_tls(
            server_der,
            Some((
                der(&self.identity.cert_pem)?,
                PrivateKeyDer::Pkcs8(
                    PrivatePkcs8KeyDer::from_pem_slice(self.identity.key_pem.as_bytes())
                        .map_err(invalid)?,
                ),
            )),
        )?;
        let secure = reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .use_preconfigured_tls(tls)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .no_proxy()
            .build()
            .map_err(caskade_core::error::net)?;
        let answer = self
            .ask(
                &secure,
                &self.endpoint.https,
                &[("phrase", "pairchallenge".into())],
            )
            .await?;
        if tag(&answer, "paired") != Some("1") {
            self.unpair(&plain).await;
            return Ok(Paired::WrongCode);
        }
        Ok(Paired::Paired {
            server_cert_pem: server_pem,
        })
    }

    /// Tells the host to forget a pairing that went wrong.
    async fn unpair(&self, client: &reqwest::Client) {
        if let Ok(url) = self.endpoint.http.join("unpair") {
            let _ = client
                .get(url)
                .query(&self.params(&[]))
                .timeout(Duration::from_secs(5))
                .send()
                .await;
        }
    }
}

/// A new random code for people to type: four digits.
pub fn new_code() -> Result<String> {
    let bytes: [u8; 2] = random()?;
    Ok(format!("{:04}", u16::from_le_bytes(bytes) % 10_000))
}

/// A paired host as Moonlight records it in its state file's `[hosts]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRecord {
    pub uuid: String,
    pub name: String,
    pub address: String,
    pub server_cert_pem: String,
}

/// Records `host` in a client state file, replacing an entry with its
/// uuid; every other line stays.
pub fn add_host(settings: &str, host: &HostRecord) -> String {
    let fields = |index: usize| {
        [
            ("customname", "false".to_owned()),
            ("hostname", host.name.clone()),
            ("ipv6address", String::new()),
            ("ipv6port", "0".into()),
            ("localaddress", String::new()),
            ("localport", "0".into()),
            ("mac", "@ByteArray()".into()),
            ("manualaddress", host.address.clone()),
            ("manualport", "47989".into()),
            ("nvidiasw", "false".into()),
            ("remoteaddress", String::new()),
            ("remoteport", "0".into()),
            ("srvcert", qt_byte_array(&host.server_cert_pem)),
            ("uuid", host.uuid.clone()),
        ]
        .map(|(key, value)| format!("{index}\\{key}={value}"))
    };
    let removed = forget_host(settings, &host.uuid);
    let mut lines: Vec<String> = removed.lines().map(str::to_owned).collect();
    let start = match lines.iter().position(|line| line.trim() == "[hosts]") {
        Some(at) => at + 1,
        None => {
            if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[hosts]".into());
            lines.len()
        }
    };
    let end = lines[start..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |at| start + at);
    let size_at = lines[start..end]
        .iter()
        .position(|line| line.starts_with("size="))
        .map(|at| start + at);
    let size: usize = size_at
        .and_then(|at| lines[at]["size=".len()..].parse().ok())
        .unwrap_or(0);
    let index = size + 1;
    let new: Vec<String> = fields(index).to_vec();
    match size_at {
        Some(at) => {
            lines[at] = format!("size={index}");
            for (offset, line) in new.into_iter().enumerate() {
                lines.insert(at + offset, line);
            }
        }
        None => {
            let mut at = end;
            for line in new {
                lines.insert(at, line);
                at += 1;
            }
            lines.insert(at, format!("size={index}"));
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Removes the host with `uuid` from a client state file's `[hosts]`,
/// renumbering the others.
pub fn forget_host(settings: &str, uuid: &str) -> String {
    let mut in_hosts = false;
    let mut out = Vec::new();
    let mut hosts: Vec<(usize, Vec<(String, String)>)> = Vec::new();
    let mut hosts_at = None;
    for line in settings.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_hosts = trimmed == "[hosts]";
            out.push(line.to_owned());
            if in_hosts {
                hosts_at = Some(out.len());
            }
            continue;
        }
        if !in_hosts {
            out.push(line.to_owned());
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            out.push(line.to_owned());
            continue;
        };
        if key == "size" {
            continue;
        }
        match key
            .split_once('\\')
            .and_then(|(n, rest)| Some((n.parse::<usize>().ok()?, rest)))
        {
            Some((index, rest)) => match hosts.iter_mut().find(|(at, _)| *at == index) {
                Some((_, fields)) => fields.push((rest.to_owned(), value.to_owned())),
                None => hosts.push((index, vec![(rest.to_owned(), value.to_owned())])),
            },
            None => out.push(line.to_owned()),
        }
    }
    let Some(at) = hosts_at else {
        return settings.to_owned();
    };
    hosts.sort_by_key(|(index, _)| *index);
    hosts.retain(|(_, fields)| {
        !fields
            .iter()
            .any(|(key, value)| key == "uuid" && value.eq_ignore_ascii_case(uuid))
    });
    let mut block = Vec::new();
    for (new_index, (_, fields)) in hosts.iter().enumerate() {
        for (key, value) in fields {
            block.push(format!("{}\\{key}={value}", new_index + 1));
        }
    }
    block.push(format!("size={}", hosts.len()));
    for (offset, line) in block.into_iter().enumerate() {
        out.insert(at + offset, line);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests;

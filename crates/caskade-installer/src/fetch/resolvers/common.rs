use crate::fetch::{DownloadRequest, FetchError, Result, http};
use futures_util::StreamExt;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResolverConfig {
    pub github_web: Url,
    pub github_api: Url,
    pub dropbox_web: Url,
    pub drive_web: Url,
    pub drive_download: Url,
    pub drive_content: Url,
    pub metadata_max_bytes: u64,
    pub metadata_timeout_secs: u64,
    /// Retries for a metadata request that timed out, could not connect or
    /// got HTTP 408, 429 or 5xx.
    pub metadata_retries: u32,
}
impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            github_web: Url::parse("https://github.com/").unwrap(),
            github_api: Url::parse("https://api.github.com/").unwrap(),
            dropbox_web: Url::parse("https://www.dropbox.com/").unwrap(),
            drive_web: Url::parse("https://drive.google.com/").unwrap(),
            drive_download: Url::parse("https://drive.google.com/uc").unwrap(),
            drive_content: Url::parse("https://drive.usercontent.google.com/").unwrap(),
            metadata_max_bytes: 4 * 1024 * 1024,
            metadata_timeout_secs: 20,
            metadata_retries: 2,
        }
    }
}
#[derive(Clone)]
pub(super) struct MetadataHttp {
    pub config: ResolverConfig,
    client: Client,
}
impl MetadataHttp {
    pub fn new(config: ResolverConfig) -> Result<Self> {
        if config.metadata_max_bytes == 0
            || config.metadata_max_bytes > 16 * 1024 * 1024
            || config.metadata_timeout_secs == 0
            || config.metadata_timeout_secs > 300
            || config.metadata_retries > 10
        {
            return Err(invalid(
                "resolver metadata limits must be nonzero and bounded",
            ));
        }
        for base in [
            &config.github_web,
            &config.github_api,
            &config.dropbox_web,
            &config.drive_web,
            &config.drive_download,
            &config.drive_content,
        ] {
            validate_url(base)?;
            if base.query().is_some() || base.fragment().is_some() {
                return Err(invalid(
                    "resolver base URLs cannot have queries or fragments",
                ));
            }
        }
        for base in [
            &config.github_web,
            &config.github_api,
            &config.dropbox_web,
            &config.drive_web,
            &config.drive_content,
        ] {
            if !base.path().ends_with('/') {
                return Err(invalid("resolver base directory URLs must end in /"));
            }
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(config.metadata_timeout_secs))
            .user_agent(concat!("CaskadeInstaller/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(FetchError::http)?;
        Ok(Self { config, client })
    }
    /// A page, which may be HTML. Resolvers run under the job's cancellation:
    /// the caller drops this future to cancel it, backoff waits included.
    pub async fn get(&self, url: &Url, headers: &BTreeMap<String, String>) -> Result<Response> {
        validate_url(url)?;
        let mut failures = 0;
        loop {
            let result = http::get(
                &self.client,
                url,
                headers,
                None,
                &CancellationToken::new(),
                None,
            )
            .await
            .and_then(|response| {
                if response.status().is_success() {
                    Ok(response)
                } else {
                    Err(FetchError::Status {
                        status: response.status().as_u16(),
                    })
                }
            });
            match result {
                Err(error) if error.retryable() && failures < self.config.metadata_retries => {
                    tokio::time::sleep(Duration::from_millis(500).saturating_mul(1 << failures))
                        .await;
                    failures += 1;
                }
                result => return result,
            }
        }
    }
    /// JSON or text metadata: a web page in its place (a captive portal or a
    /// login page) is an error, not data.
    pub async fn get_data(
        &self,
        url: &Url,
        headers: &BTreeMap<String, String>,
    ) -> Result<Response> {
        let response = self.get(url, headers).await?;
        if http::is_html(response.headers()) {
            return Err(invalid(
                "the server answered with a web page instead of download information; check the network connection",
            ));
        }
        Ok(response)
    }
    pub async fn body(&self, response: Response) -> Result<Vec<u8>> {
        let limit = self.config.metadata_max_bytes;
        if response.content_length().is_some_and(|size| size > limit) {
            return Err(FetchError::TooLarge { limit });
        }
        let mut stream = response.bytes_stream();
        let mut body = vec![];
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(FetchError::http)?;
            if (body.len() as u64).saturating_add(chunk.len() as u64) > limit {
                return Err(FetchError::TooLarge { limit });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}
pub(super) fn invalid(message: impl Into<String>) -> FetchError {
    FetchError::InvalidResponse {
        message: message.into(),
    }
}
pub(super) fn validate_url(url: &Url) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FetchError::UnsupportedScheme);
    }
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid(
            "resolver URL must have a host and no embedded credentials",
        ));
    }
    Ok(())
}
pub(super) fn relative_path<'a>(base: &Url, url: &'a Url) -> Option<&'a str> {
    (base.origin() == url.origin())
        .then(|| url.path().strip_prefix(base.path()))
        .flatten()
}
pub(super) fn component(value: &str) -> Result<String> {
    percent_encoding::percent_decode_str(value)
        .decode_utf8()
        .map(|s| s.into_owned())
        .map_err(|_| invalid("URL path is not valid UTF-8"))
}
pub(super) fn carry_fragment(source: &Url, request: &mut DownloadRequest) -> Result<()> {
    validate_url(source)?;
    validate_url(&request.url)?;
    if source.scheme() == "https" && request.url.scheme() == "http" {
        return Err(FetchError::InsecureRedirect);
    }
    request.url.set_fragment(source.fragment());
    Ok(())
}

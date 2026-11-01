use super::common::{
    MetadataHttp, ResolverConfig, carry_fragment, component, invalid, relative_path, validate_url,
};
use crate::{
    fetch::{
        DownloadRequest, Resolution, ResolveFuture, Result, UrlResolver, disposition_name, http,
    },
    host::HostCaps,
};
use scraper::{Html, Selector};
use std::collections::BTreeMap;
use url::Url;

#[derive(Clone)]
pub struct GoogleDriveResolver {
    http: MetadataHttp,
}
impl GoogleDriveResolver {
    pub fn new(config: ResolverConfig) -> Result<Self> {
        Ok(Self::from_http(MetadataHttp::new(config)?))
    }
    pub(super) fn from_http(http: MetadataHttp) -> Self {
        Self { http }
    }
    fn file_id(&self, url: &Url) -> Result<String> {
        let path = relative_path(&self.http.config.drive_web, url)
            .ok_or_else(|| invalid("not a configured Drive URL"))?;
        let parts = path.trim_end_matches('/').split('/').collect::<Vec<_>>();
        let id = if parts.len() >= 3 && parts[0] == "file" && parts[1] == "d" {
            component(parts[2])?
        } else if matches!(path, "open" | "uc") {
            let ids = url
                .query_pairs()
                .filter(|(key, _)| key == "id")
                .map(|(_, value)| value.into_owned())
                .collect::<Vec<_>>();
            if ids.len() != 1 {
                return Err(invalid("Drive URL needs one file ID"));
            }
            ids[0].clone()
        } else {
            return Err(invalid("Drive URL is not a public file share"));
        };
        if id.is_empty()
            || id.len() > 256
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(invalid("invalid Drive file ID"));
        }
        Ok(id)
    }
    fn allowed(&self, url: &Url) -> bool {
        url.origin() == self.http.config.drive_download.origin()
            && url.path() == self.http.config.drive_download.path()
            || relative_path(&self.http.config.drive_content, url).is_some()
    }
    fn confirmation(
        &self,
        text: &str,
        current: &Url,
        id: &str,
        cookies: &BTreeMap<String, String>,
    ) -> Result<Url> {
        let html = Html::parse_document(text);
        let forms = Selector::parse("form#download-form").expect("static selector");
        let forms = html.select(&forms).collect::<Vec<_>>();
        if forms.len() == 1 {
            let form = forms[0];
            if form
                .value()
                .attr("method")
                .is_some_and(|method| !method.eq_ignore_ascii_case("get"))
            {
                return Err(invalid("Drive confirmation must use GET"));
            }
            let mut target = current
                .join(
                    form.value()
                        .attr("action")
                        .ok_or_else(|| invalid("Drive confirmation has no action"))?,
                )
                .map_err(|_| invalid("invalid Drive confirmation action"))?;
            validate_url(&target)?;
            if !self.allowed(&target) {
                return Err(invalid(
                    "Drive confirmation points outside the configured download service",
                ));
            }
            let inputs = Selector::parse("input[name]").expect("static selector");
            let mut values = BTreeMap::new();
            let mut total = 0usize;
            for input in form.select(&inputs) {
                let name = input.value().attr("name").unwrap_or_default();
                if !matches!(
                    name,
                    "id" | "export" | "confirm" | "uuid" | "at" | "authuser" | "resourcekey"
                ) {
                    continue;
                }
                let value = input.value().attr("value").unwrap_or_default();
                total = total.saturating_add(name.len() + value.len());
                if total > 8192 || values.insert(name.to_owned(), value.to_owned()).is_some() {
                    return Err(invalid(
                        "Drive confirmation fields are duplicated or oversized",
                    ));
                }
            }
            if values.get("id").map(String::as_str) != Some(id) || !values.contains_key("confirm") {
                return Err(invalid(
                    "Drive confirmation does not identify the requested file",
                ));
            }
            target.query_pairs_mut().clear().extend_pairs(values);
            copy_resource_key(current, &mut target);
            return Ok(target);
        }
        // Older Drive warning pages carry the confirmation token in a cookie.
        if forms.is_empty() && cookies.len() == 1 {
            let mut target = self.http.config.drive_download.clone();
            target
                .query_pairs_mut()
                .append_pair("export", "download")
                .append_pair("id", id)
                .append_pair("confirm", cookies.values().next().unwrap());
            copy_resource_key(current, &mut target);
            return Ok(target);
        }
        // An older warning page may use an explicit download link instead.
        let selector = Selector::parse("a#uc-download-link[href]").expect("static selector");
        let links = html.select(&selector).collect::<Vec<_>>();
        if forms.is_empty() && links.len() == 1 {
            let mut target = current
                .join(links[0].value().attr("href").unwrap_or_default())
                .map_err(|_| invalid("invalid Drive confirmation link"))?;
            let ids = target
                .query_pairs()
                .filter(|(k, _)| k == "id")
                .map(|(_, v)| v.into_owned())
                .collect::<Vec<_>>();
            let confirmations = target.query_pairs().filter(|(k, _)| k == "confirm").count();
            if self.allowed(&target) && ids == [id] && confirmations == 1 {
                copy_resource_key(current, &mut target);
                return Ok(target);
            }
        }
        Err(invalid(
            "Drive file requires sign-in, is unavailable, or has no supported download confirmation",
        ))
    }
}
impl UrlResolver for GoogleDriveResolver {
    fn id(&self) -> &str {
        "google-drive"
    }
    fn claims(&self, url: &Url) -> bool {
        relative_path(&self.http.config.drive_web, url)
            .is_some_and(|path| path.starts_with("file/d/") || matches!(path, "open" | "uc"))
    }
    fn resolve<'a>(&'a self, url: &'a Url, _: &'a HostCaps) -> ResolveFuture<'a> {
        Box::pin(async move {
            let id = self.file_id(url)?;
            let mut target = self.http.config.drive_download.clone();
            target
                .query_pairs_mut()
                .append_pair("export", "download")
                .append_pair("id", &id);
            if let Some((_, value)) = url.query_pairs().find(|(key, _)| key == "resourcekey") {
                if value.len() > 1024 {
                    return Err(invalid("Drive resource key is oversized"));
                }
                target.query_pairs_mut().append_pair("resourcekey", &value);
            }
            let response = self.http.get(&target, &BTreeMap::new()).await?;
            let current = response.url().clone();
            let cookies = warning_cookies(response.headers())?;
            let is_html = http::is_html(response.headers());
            let file_name = http::header(response.headers(), "content-disposition")
                .as_deref()
                .map(disposition_name)
                .transpose()?
                .flatten();
            let size = response.content_length();
            let mut request = if is_html {
                let bytes = self.http.body(response).await?;
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| invalid("Drive confirmation is not UTF-8"))?;
                DownloadRequest::new(self.confirmation(text, &current, &id, &cookies)?)
            } else {
                let mut request = DownloadRequest::new(current.clone());
                request.file_name = file_name;
                request.size = size;
                request
            };
            if !cookies.is_empty() && self.allowed(&request.url) {
                request.headers.insert(
                    "cookie".into(),
                    cookies
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join("; "),
                );
            }
            carry_fragment(url, &mut request)?;
            Ok(Resolution::Downloads(vec![request]))
        })
    }
}
fn copy_resource_key(source: &Url, target: &mut Url) {
    if !target.query_pairs().any(|(key, _)| key == "resourcekey")
        && let Some((_, value)) = source.query_pairs().find(|(key, _)| key == "resourcekey")
    {
        target.query_pairs_mut().append_pair("resourcekey", &value);
    }
}
fn warning_cookies(headers: &reqwest::header::HeaderMap) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for value in headers.get_all(reqwest::header::SET_COOKIE) {
        let value = value
            .to_str()
            .map_err(|_| invalid("invalid Drive warning cookie"))?;
        let pair = value.split(';').next().unwrap_or_default();
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if !name.starts_with("download_warning") {
            continue;
        }
        if name.len() > 128
            || value.len() > 1024
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || value
                .bytes()
                .any(|b| b.is_ascii_control() || b == b' ' || b == b'"')
        {
            return Err(invalid("invalid Drive warning cookie"));
        }
        if values.insert(name.into(), value.into()).is_some() || values.len() > 8 {
            return Err(invalid("too many Drive warning cookies"));
        }
    }
    Ok(values)
}

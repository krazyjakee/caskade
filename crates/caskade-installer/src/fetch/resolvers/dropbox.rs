use super::common::{MetadataHttp, ResolverConfig, carry_fragment, invalid, relative_path};
use crate::{
    fetch::{DownloadRequest, Resolution, ResolveFuture, Result, UrlResolver},
    host::HostCaps,
};
use url::Url;

#[derive(Clone)]
pub struct DropboxResolver {
    http: MetadataHttp,
}
impl DropboxResolver {
    pub fn new(config: ResolverConfig) -> Result<Self> {
        Ok(Self::from_http(MetadataHttp::new(config)?))
    }
    pub(super) fn from_http(http: MetadataHttp) -> Self {
        Self { http }
    }
}
impl UrlResolver for DropboxResolver {
    fn id(&self) -> &str {
        "dropbox"
    }
    fn claims(&self, url: &Url) -> bool {
        let mut normalized = url.clone();
        if self.http.config.dropbox_web.host_str() == Some("www.dropbox.com")
            && url.host_str() == Some("dropbox.com")
        {
            let _ = normalized.set_host(Some("www.dropbox.com"));
        }
        relative_path(&self.http.config.dropbox_web, &normalized)
            .is_some_and(|path| path.starts_with("s/") || path.starts_with("scl/fi/"))
    }
    fn resolve<'a>(&'a self, url: &'a Url, _: &'a HostCaps) -> ResolveFuture<'a> {
        Box::pin(async move {
            if !self.claims(url) {
                return Err(invalid("not a configured Dropbox share URL"));
            }
            let mut target = url.clone();
            let pairs: Vec<_> = url
                .query_pairs()
                .filter(|(key, _)| key != "dl" && key != "raw")
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            target
                .query_pairs_mut()
                .clear()
                .extend_pairs(pairs)
                .append_pair("dl", "1");
            let mut request = DownloadRequest::new(target);
            carry_fragment(url, &mut request)?;
            Ok(Resolution::Downloads(vec![request]))
        })
    }
}

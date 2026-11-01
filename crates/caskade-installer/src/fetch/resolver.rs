use std::sync::Arc;

use url::Url;

use crate::host::HostCaps;

use super::{DownloadRequest, FetchError, Resolution, ResolveFuture, Result, UrlResolver};

#[derive(Debug, Default)]
pub struct Direct;
impl UrlResolver for Direct {
    fn id(&self) -> &str {
        "direct"
    }
    fn claims(&self, url: &Url) -> bool {
        matches!(url.scheme(), "http" | "https")
    }
    fn resolve<'a>(&'a self, url: &'a Url, _: &'a HostCaps) -> ResolveFuture<'a> {
        Box::pin(async move {
            if !self.claims(url) {
                return Err(FetchError::UnsupportedScheme);
            }
            Ok(Resolution::Downloads(vec![DownloadRequest::new(
                url.clone(),
            )]))
        })
    }
}

#[derive(Default, Clone)]
pub struct ResolverRegistry {
    resolvers: Vec<Arc<dyn UrlResolver>>,
    builtins: Vec<Arc<dyn UrlResolver>>,
}
impl ResolverRegistry {
    pub fn with_builtins(config: super::ResolverConfig) -> Result<Self> {
        let mut registry = Self::default();
        registry.configure_builtins(config)?;
        Ok(registry)
    }
    pub(super) fn configure_builtins(&mut self, config: super::ResolverConfig) -> Result<()> {
        let builtins = super::resolvers::builtins(config)?;
        if let Some(duplicate) = builtins.iter().find(|builtin| {
            self.resolvers
                .iter()
                .any(|custom| custom.id() == builtin.id())
        }) {
            return Err(FetchError::DuplicateResolver {
                id: duplicate.id().into(),
            });
        }
        self.builtins = builtins;
        Ok(())
    }
    pub fn register(&mut self, resolver: Arc<dyn UrlResolver>) -> Result<()> {
        if resolver.id().trim().is_empty() {
            return Err(FetchError::InvalidRequest {
                message: "empty resolver ID".into(),
            });
        }
        if resolver.id() == "direct"
            || self
                .resolvers
                .iter()
                .chain(self.builtins.iter())
                .any(|existing| existing.id() == resolver.id())
        {
            return Err(FetchError::DuplicateResolver {
                id: resolver.id().into(),
            });
        }
        self.resolvers.push(resolver);
        Ok(())
    }
    pub async fn resolve(&self, url: &Url, host: &HostCaps) -> Result<Resolution> {
        for resolver in self.resolvers.iter().chain(self.builtins.iter()) {
            if resolver.claims(url) {
                return resolver.resolve(url, host).await;
            }
        }
        Direct.resolve(url, host).await
    }
}

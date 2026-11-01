use super::common::{
    MetadataHttp, ResolverConfig, carry_fragment, component, invalid, relative_path, validate_url,
};
use crate::{
    DownloadAsset, OperatingSystem, Question,
    fetch::{
        DownloadRequest, FetchError, Resolution, ResolveFuture, Result, UrlResolver, safe_file_name,
    },
    host::HostCaps,
};
use scraper::{Html, Selector};
use serde::Deserialize;
use std::collections::BTreeMap;
use url::Url;

#[derive(Clone)]
pub struct GitHubResolver {
    http: MetadataHttp,
}
#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    draft: bool,
    assets: Vec<Asset>,
}
/// Which release a URL names.
enum Wanted {
    Latest,
    Tag(String),
}
#[derive(Debug, Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: Url,
    /// Unknown when the asset comes from the release page.
    size: Option<u64>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    digest: Option<String>,
}
impl GitHubResolver {
    pub fn new(config: ResolverConfig) -> Result<Self> {
        Ok(Self::from_http(MetadataHttp::new(config)?))
    }
    pub(super) fn from_http(http: MetadataHttp) -> Self {
        Self { http }
    }
    /// The repository and release a GitHub URL names.
    fn release_of(&self, url: &Url) -> Result<(String, String, Wanted)> {
        let path = relative_path(&self.http.config.github_web, url)
            .ok_or_else(|| invalid("not a configured GitHub URL"))?;
        let parts = path
            .trim_end_matches('/')
            .split('/')
            .map(component)
            .collect::<Result<Vec<_>>>()?;
        if parts.len() < 2 {
            return Err(invalid("GitHub URL needs an owner and repository"));
        }
        let owner = &parts[0];
        let repo = parts[1].strip_suffix(".git").unwrap_or(&parts[1]);
        let valid = |name: &str| {
            !name.is_empty()
                && name.len() <= 100
                && name != "."
                && name != ".."
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        };
        if !valid(owner) || !valid(repo) {
            return Err(invalid("invalid GitHub repository path"));
        }
        let wanted = match parts.get(2).map(String::as_str) {
            None => Wanted::Latest,
            Some("releases")
                if parts.len() == 3
                    || parts.get(3).is_some_and(|p| p == "latest") && parts.len() == 4 =>
            {
                Wanted::Latest
            }
            Some("releases") if parts.get(3).is_some_and(|p| p == "tag") && parts.len() >= 5 => {
                Wanted::Tag(valid_tag(parts[4..].join("/"))?)
            }
            _ => return Err(invalid("GitHub URL is not a repository or release page")),
        };
        Ok((owner.clone(), repo.to_owned(), wanted))
    }
    fn api_url(&self, owner: &str, repo: &str, wanted: &Wanted) -> Result<Url> {
        let mut api = self.http.config.github_api.clone();
        {
            let mut segments = api
                .path_segments_mut()
                .map_err(|_| invalid("invalid GitHub API base"))?;
            segments
                .pop_if_empty()
                .extend(["repos", owner, repo, "releases"]);
            match wanted {
                Wanted::Latest => segments.push("latest"),
                Wanted::Tag(tag) => segments.push("tags").push(tag),
            };
        }
        Ok(api)
    }
    fn web_url(&self, owner: &str, repo: &str, rest: &[&str]) -> Result<Url> {
        let mut url = self.http.config.github_web.clone();
        url.path_segments_mut()
            .map_err(|_| invalid("invalid GitHub base"))?
            .pop_if_empty()
            .extend([owner, repo])
            .extend(rest);
        Ok(url)
    }
    /// The release from the REST API, or, when the API's hourly limit for
    /// unauthenticated clients is used up, from the release page's asset
    /// list, which has no limit.
    async fn release(&self, url: &Url) -> Result<Release> {
        let (owner, repo, wanted) = self.release_of(url)?;
        let api = self.api_url(&owner, &repo, &wanted)?;
        let headers = BTreeMap::from([("accept".into(), "application/vnd.github+json".into())]);
        match self.http.get_data(&api, &headers).await {
            Ok(response) => serde_json::from_slice(&self.http.body(response).await?)
                .map_err(|_| invalid("invalid GitHub release response")),
            Err(FetchError::Status { status: 403 | 429 }) => {
                self.web_release(&owner, &repo, wanted).await
            }
            Err(error) => Err(error),
        }
    }
    async fn web_release(&self, owner: &str, repo: &str, wanted: Wanted) -> Result<Release> {
        let tag = match wanted {
            Wanted::Tag(tag) => tag,
            // `releases/latest` redirects to the latest release's tag page.
            Wanted::Latest => {
                let latest = self.web_url(owner, repo, &["releases", "latest"])?;
                let response = self.http.get(&latest, &BTreeMap::new()).await?;
                let prefix = self.web_url(owner, repo, &["releases", "tag", ""])?;
                let tag = response
                    .url()
                    .path()
                    .strip_prefix(prefix.path())
                    .filter(|_| response.url().origin() == prefix.origin())
                    .ok_or_else(|| invalid("GitHub's latest release has no tag"))?;
                valid_tag(component(tag)?)?
            }
        };
        let page = self.web_url(owner, repo, &["releases", "expanded_assets", &tag])?;
        let response = self.http.get(&page, &BTreeMap::new()).await?;
        // A network that answers in GitHub's place (a captive portal) sends
        // its own page from elsewhere; its links are not release assets.
        if response.url().origin() != page.origin() {
            return Err(invalid(
                "GitHub's release page did not come from GitHub; check the network connection",
            ));
        }
        let html = self.http.body(response).await?;
        let html = std::str::from_utf8(&html).map_err(|_| invalid("asset list is not UTF-8"))?;
        let download = self.web_url(owner, repo, &["releases", "download", &tag, ""])?;
        Ok(Release {
            draft: false,
            assets: page_assets(html, &page, &download)?,
        })
    }
    async fn checksum(&self, source: &Url, asset: &Asset, all: &[Asset]) -> Result<Option<String>> {
        if let Some(digest) = asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
        {
            return Ok(Some(valid_digest(digest)?));
        }
        let exact = [
            format!("{}.sha256", asset.name),
            format!("{}.sha256sum", asset.name),
        ];
        let companion = all
            .iter()
            .find(|a| exact.iter().any(|n| n.eq_ignore_ascii_case(&a.name)))
            .or_else(|| {
                all.iter().find(|a| {
                    matches!(
                        a.name.to_ascii_lowercase().as_str(),
                        "sha256sums" | "sha256sums.txt" | "sha256sum.txt" | "checksums.txt"
                    )
                })
            });
        let Some(companion) = companion else {
            return Ok(None);
        };
        if companion
            .size
            .is_some_and(|size| size > self.http.config.metadata_max_bytes)
        {
            return Err(invalid("checksum sidecar exceeds resolver metadata budget"));
        }
        let mut request = DownloadRequest::new(companion.browser_download_url.clone());
        carry_fragment(source, &mut request)?;
        let response = self.http.get_data(&request.url, &BTreeMap::new()).await?;
        let bytes = self.http.body(response).await?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| invalid("checksum sidecar is not UTF-8"))?;
        parse_checksum(
            text,
            &asset.name,
            exact
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&companion.name)),
        )
        .map(Some)
    }
}
impl UrlResolver for GitHubResolver {
    fn id(&self) -> &str {
        "github"
    }
    fn claims(&self, url: &Url) -> bool {
        relative_path(&self.http.config.github_web, url).is_some_and(|path| {
            let parts: Vec<_> = path.trim_end_matches('/').split('/').collect();
            parts.len() >= 2
                && (parts.len() == 2
                    || parts.get(2) == Some(&"releases") && parts.get(3) != Some(&"download"))
        })
    }
    fn resolve<'a>(&'a self, url: &'a Url, host: &'a HostCaps) -> ResolveFuture<'a> {
        Box::pin(async move {
            let release = self.release(url).await?;
            if release.draft || release.assets.len() > 1024 {
                return Err(invalid("release is a draft or exceeds the asset budget"));
            }
            for asset in &release.assets {
                if asset.name.len() > 1024 {
                    return Err(invalid("release asset name exceeds budget"));
                }
                safe_file_name(&asset.name)?;
                validate_url(&asset.browser_download_url)?;
            }
            let mut ranked = release
                .assets
                .iter()
                .filter(|asset| {
                    asset
                        .state
                        .as_deref()
                        .is_none_or(|state| state == "uploaded")
                })
                .filter_map(|asset| score(&asset.name, host).map(|score| (score, asset)))
                .collect::<Vec<_>>();
            let top = ranked
                .iter()
                .map(|(score, _)| *score)
                .max()
                .ok_or_else(|| invalid("release has no supported downloadable assets"))?;
            ranked.retain(|(score, _)| *score == top);
            ranked.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));
            if ranked.len() > 64 {
                return Err(invalid("release has too many equally ranked assets"));
            }
            if ranked.len() > 1 {
                let assets = ranked
                    .into_iter()
                    .map(|(_, asset)| {
                        let digest = asset
                            .digest
                            .as_deref()
                            .and_then(|d| d.strip_prefix("sha256:"))
                            .map(valid_digest)
                            .transpose()?;
                        let mut request = DownloadRequest::new(asset.browser_download_url.clone());
                        carry_fragment(url, &mut request)?;
                        Ok(DownloadAsset {
                            name: asset.name.clone(),
                            url: request.url,
                            size: asset.size,
                            sha256: digest,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                return Ok(Resolution::NeedsUser(Question::ChooseAsset { assets }));
            }
            let asset = ranked[0].1;
            let mut request = DownloadRequest::new(asset.browser_download_url.clone());
            carry_fragment(url, &mut request)?;
            request.file_name = Some(asset.name.clone());
            request.size = asset.size;
            request.sha256 = self.checksum(url, asset, &release.assets).await?;
            Ok(Resolution::Downloads(vec![request]))
        })
    }
}
fn valid_tag(tag: String) -> Result<String> {
    if tag.is_empty() || tag.len() > 1024 || tag.chars().any(char::is_control) {
        return Err(invalid("invalid release tag"));
    }
    Ok(tag)
}
/// The download links on a release's `expanded_assets` page (what
/// github.com loads into the release's Assets list): every `href` under
/// `download`, the release's download folder. Source archives link
/// elsewhere, so they are left out like the API leaves them out.
fn page_assets(html: &str, page: &Url, download: &Url) -> Result<Vec<Asset>> {
    let html = Html::parse_fragment(html);
    let links = Selector::parse("a[href]").expect("static selector");
    let mut assets: Vec<Asset> = Vec::new();
    for link in html.select(&links) {
        let Ok(url) = page.join(link.value().attr("href").unwrap_or_default()) else {
            continue;
        };
        if url.origin() != download.origin() || url.query().is_some() {
            continue;
        }
        let Some(name) = url.path().strip_prefix(download.path()) else {
            continue;
        };
        let name = component(name)?;
        if name.is_empty() || name.contains('/') || assets.iter().any(|a| a.name == name) {
            continue;
        }
        if assets.len() >= 1024 {
            return Err(invalid("release exceeds the asset budget"));
        }
        assets.push(Asset {
            name,
            browser_download_url: url,
            size: None,
            state: None,
            digest: None,
        });
    }
    Ok(assets)
}
fn valid_digest(value: &str) -> Result<String> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("invalid SHA-256 release digest"));
    }
    Ok(value.to_ascii_lowercase())
}
fn parse_checksum(text: &str, name: &str, single: bool) -> Result<String> {
    let mut found = None;
    let bsd_prefix = format!("SHA256 ({name}) = ");
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let hash = if let Some(hash) = line.strip_prefix(&bsd_prefix) {
            Some(hash)
        } else if let Some(end) = line.find(char::is_whitespace) {
            let file = line[end..].trim().trim_start_matches('*');
            let file = file.strip_prefix("./").unwrap_or(file);
            (file == name).then_some(&line[..end])
        } else {
            single.then_some(line)
        };
        if let Some(hash) = hash {
            let hash = valid_digest(hash)?;
            if found.as_ref().is_some_and(|previous| previous != &hash) {
                return Err(invalid("conflicting release checksum lines"));
            }
            found = Some(hash);
        }
    }
    found.ok_or_else(|| invalid("checksum sidecar does not identify the selected asset"))
}

fn score(name: &str, host: &HostCaps) -> Option<i32> {
    let lower = name.to_ascii_lowercase();
    let words: Vec<_> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    let has = |tokens: &[&str]| words.iter().any(|word| tokens.contains(word));
    if has(&["debug", "symbols", "source", "sources", "src", "sdk"])
        || lower.ends_with(".sha256")
        || lower.ends_with(".sha256sum")
    {
        return None;
    }
    let supported = [
        ".zip",
        ".7z",
        ".rar",
        ".tar",
        ".tar.gz",
        ".tgz",
        ".tar.xz",
        ".txz",
        ".tar.bz2",
        ".tar.zst",
        ".exe",
        ".msi",
        ".dmg",
        ".pkg",
        ".appimage",
        ".deb",
        ".rpm",
        ".sh",
    ]
    .iter()
    .any(|suffix| lower.ends_with(suffix));
    if !supported {
        return None;
    }
    let windows = has(&["windows", "win", "win32", "win64"])
        || lower.ends_with(".exe")
        || lower.ends_with(".msi");
    let mac = has(&["macos", "mac", "osx", "darwin"])
        || lower.ends_with(".dmg")
        || lower.ends_with(".pkg");
    let linux = has(&["linux", "appimage"])
        || [".deb", ".rpm", ".sh"]
            .iter()
            .any(|ext| lower.ends_with(ext));
    let matched = match host.os {
        OperatingSystem::Windows => windows,
        OperatingSystem::Linux => linux,
        OperatingSystem::Macos => mac,
    };
    if !matched && (windows || mac || linux) {
        return None;
    }
    let mut score = if matched { 100 } else { 0 };
    let x64 =
        has(&["x64", "amd64", "win64"]) || lower.contains("x86_64") || lower.contains("x86-64");
    let arm64 = has(&["arm64", "aarch64"]);
    let x86 = has(&["x86", "i386", "i686", "win32"]) && !x64;
    let arm = has(&["arm", "armv7", "armhf"]) && !arm64;
    let universal = has(&["universal", "universal2"]);
    let arch = host.arch.to_ascii_lowercase();
    let archmatch = match arch.as_str() {
        "x86_64" | "amd64" | "x64" => x64,
        "aarch64" | "arm64" => arm64,
        "x86" | "i686" | "i386" => x86,
        "arm" | "armv7" | "armv7l" | "armhf" => arm,
        _ => false,
    };
    if (x64 || arm64 || x86 || arm) && !archmatch && !universal {
        return None;
    }
    score += if archmatch {
        20
    } else if universal {
        10
    } else {
        0
    };
    Some(score)
}

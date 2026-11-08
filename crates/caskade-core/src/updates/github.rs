//! GitHub releases, without an account or API key.
//!
//! The REST API allows 60 requests an hour per address to clients without
//! a key (304 answers count too), which a library of GitHub games would
//! use up, so checks use the website instead:
//!
//! - `/{owner}/{repo}/releases/latest` redirects to the latest stable
//!   release's tag page.
//! - `/{owner}/{repo}/releases.atom` lists releases newest first,
//!   prereleases included.
//! - `/{owner}/{repo}/releases/download/{tag}/{file}` redirects to a file
//!   that exists and is 404 otherwise.
//!
//! When the installed file's name can't be matched in the new release, the
//! update installs from the release page, which the installer's GitHub
//! resolver turns into this host's file (or a question).

use super::{Followed, ProviderFuture, Release, UpdateProvider, UpdateRow, version};
use crate::{Error, Result};
use url::Url;

/// GitHub releases. `web` is github.com, or a test server.
#[derive(Debug, Clone)]
pub struct GitHub {
    web: Url,
}

impl Default for GitHub {
    fn default() -> Self {
        Self::new(Url::parse("https://github.com/").expect("valid URL"))
    }
}

impl GitHub {
    /// GitHub at `web`, a base URL ending in `/`.
    pub fn new(web: Url) -> Self {
        Self { web }
    }

    /// `owner/repo` and the rest of the path, for a URL on GitHub.
    fn repository(&self, url: &Url) -> Option<(String, Vec<String>)> {
        if url.origin() != self.web.origin() {
            return None;
        }
        let path = url.path().strip_prefix(self.web.path())?;
        let parts = path
            .trim_end_matches('/')
            .split('/')
            .map(|part| {
                percent_encoding::percent_decode_str(part)
                    .decode_utf8()
                    .ok()
                    .map(|part| part.into_owned())
            })
            .collect::<Option<Vec<_>>>()?;
        let [owner, repo, rest @ ..] = parts.as_slice() else {
            return None;
        };
        let repo = repo.strip_suffix(".git").unwrap_or(repo);
        let valid = |name: &str| {
            !name.is_empty()
                && name.len() <= 100
                && name != "."
                && name != ".."
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        };
        (valid(owner) && valid(repo)).then(|| (format!("{owner}/{repo}"), rest.to_vec()))
    }

    /// A page under the repository: `releases`, `tag`, … are path segments.
    fn url(&self, project: &str, rest: &[&str]) -> Result<Url> {
        let mut url = self.web.clone();
        url.path_segments_mut()
            .map_err(|()| Error::Invalid(crate::tr!("GitHub's address can't have a path")))?
            .pop_if_empty()
            .extend(project.split('/'))
            .extend(rest);
        Ok(url)
    }

    /// The tag of a release page's URL.
    fn tag_of(&self, project: &str, page: &Url) -> Option<String> {
        let (found, rest) = self.repository(page)?;
        let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
        match rest.as_slice() {
            ["releases", "tag", tag @ ..] if found == project && !tag.is_empty() => {
                Some(tag.join("/"))
            }
            _ => None,
        }
    }
}

/// Release lookups for emulator installs.
impl GitHub {
    /// The latest stable release's tag; `None` when there is none.
    pub async fn latest_tag(&self, project: &str) -> Result<Option<String>> {
        Ok(self
            .latest(project, false)
            .await?
            .map(|release| release.version))
    }

    /// The files of release `tag`, from its `expanded_assets` page (the
    /// list the release page loads, without the API's rate limit).
    pub async fn assets(&self, project: &str, tag: &str) -> Result<Vec<String>> {
        let page = self.url(project, &["releases", "expanded_assets", tag])?;
        let Some(html) = get_text(page).await? else {
            return Ok(Vec::new());
        };
        let prefix = self
            .url(project, &["releases", "download", tag, ""])?
            .path()
            .to_owned();
        let mut names: Vec<String> = html
            .split("href=\"")
            .skip(1)
            .filter_map(|link| {
                let href = &link[..link.find('"')?];
                let name = href.strip_prefix(&prefix)?;
                percent_encoding::percent_decode_str(name)
                    .decode_utf8()
                    .ok()
                    .map(|name| name.replace("&amp;", "&"))
            })
            .filter(|name| !name.is_empty() && !name.contains('/'))
            .collect();
        names.dedup();
        Ok(names)
    }

    /// When release `tag` was last published, from the release feed: what
    /// tells two builds under a reused tag apart. `None` when the feed
    /// doesn't list it.
    pub async fn published(&self, project: &str, tag: &str) -> Result<Option<String>> {
        let feed = self.url(project, &["releases.atom"])?;
        let Some(text) = get_text(feed).await? else {
            return Ok(None);
        };
        let page = self.url(project, &["releases", "tag", tag])?;
        Ok(entry_updated(&text, page.path()))
    }

    /// The address of a release's file.
    pub fn file(&self, project: &str, tag: &str, name: &str) -> Result<Url> {
        self.url(project, &["releases", "download", tag, name])
    }
}

/// The `<updated>` time of the feed entry linking to `path`.
fn entry_updated(feed: &str, path: &str) -> Option<String> {
    feed.split("<entry>").skip(1).find_map(|entry| {
        let entry = &entry[..entry.find("</entry>")?];
        if !entry.contains(&format!("{path}\"")) {
            return None;
        }
        let updated = &entry[entry.find("<updated>")? + 9..];
        Some(updated[..updated.find("</updated>")?].trim().to_owned())
    })
}

impl UpdateProvider for GitHub {
    fn id(&self) -> &str {
        "github"
    }

    fn follow(&self, urls: &[Url]) -> Option<Followed> {
        urls.iter().find_map(|url| {
            let (project, rest) = self.repository(url)?;
            let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
            let (version, asset) = match rest.as_slice() {
                [] | ["releases"] | ["releases", "latest"] => (None, None),
                ["releases", "latest", "download", asset] => (None, Some(*asset)),
                ["releases", "download", tag, asset] => (Some((*tag).to_owned()), Some(*asset)),
                ["releases", "tag", tag @ ..] if !tag.is_empty() => (Some(tag.join("/")), None),
                _ => return None,
            };
            Some(Followed {
                project,
                version,
                asset: asset.map(str::to_owned),
            })
        })
    }

    fn page(&self, project: &str) -> Option<String> {
        self.url(project, &["releases"]).ok().map(String::from)
    }

    fn latest<'a>(
        &'a self,
        project: &'a str,
        prereleases: bool,
    ) -> ProviderFuture<'a, Option<Release>> {
        Box::pin(async move {
            if prereleases {
                let feed = self.url(project, &["releases.atom"])?;
                let Some(text) = get_text(feed).await? else {
                    return Ok(None);
                };
                return Ok(newest_entry(&text).and_then(|page| {
                    let page = self.web.join(&page).ok()?;
                    Some(Release {
                        version: self.tag_of(project, &page)?,
                        url: page.into(),
                    })
                }));
            }
            let latest = self.url(project, &["releases", "latest"])?;
            // No release, or only prereleases: 404, or a redirect to the
            // release list.
            let Some(page) = redirect(latest).await? else {
                return Ok(None);
            };
            Ok(self.tag_of(project, &page).map(|version| Release {
                version,
                url: page.into(),
            }))
        })
    }

    fn download<'a>(&'a self, row: &'a UpdateRow, version: &'a str) -> ProviderFuture<'a, String> {
        Box::pin(async move {
            let mut names = Vec::new();
            if let Some(asset) = &row.asset {
                if let Some(installed) = &row.installed
                    && let Some(swapped) = version::swap(asset, installed, version)
                {
                    names.push(swapped);
                }
                names.push(asset.clone());
            }
            for name in names {
                let file = self.file(&row.project, version, &name)?;
                if redirect(file.clone()).await?.is_some() {
                    return Ok(file.into());
                }
            }
            Ok(self
                .url(&row.project, &["releases", "tag", version])?
                .into())
        })
    }
}

/// The page of the feed's first (newest) entry.
fn newest_entry(feed: &str) -> Option<String> {
    let entry = &feed[feed.find("<entry>")?..];
    let entry = &entry[..entry.find("</entry>")?];
    entry.split("<link").skip(1).find_map(|link| {
        let link = &link[..link.find('>')?];
        if !link.contains(r#"rel="alternate""#) {
            return None;
        }
        let href = &link[link.find(r#"href=""#)? + 6..];
        Some(href[..href.find('"')?].replace("&amp;", "&"))
    })
}

#[cfg(feature = "net")]
mod http {
    use crate::{Error, Result, error::net};
    use std::{sync::OnceLock, time::Duration};
    use url::Url;

    /// Feeds of busy projects run to a few hundred KiB.
    const MAX_FEED: usize = 4 << 20;

    /// Redirects are read, not followed.
    fn client() -> Result<reqwest::Client> {
        static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
        if let Some(client) = CLIENT.get() {
            return Ok(client.clone());
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(net)?;
        Ok(CLIENT.get_or_init(|| client).clone())
    }

    fn status(response: &reqwest::Response) -> Error {
        Error::Io(std::io::Error::other(crate::i18n::TextError::from(
            crate::msg!("GitHub answered {status}", status = response.status()),
        )))
    }

    /// Where `url` redirects to; `None` when it is 404.
    pub(super) async fn redirect(url: Url) -> Result<Option<Url>> {
        let response = client()?.head(url.clone()).send().await.map_err(net)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_redirection() {
            return Err(status(&response));
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| Error::Invalid(crate::tr!("GitHub's redirect has no location")))?;
        url.join(location)
            .map(Some)
            .map_err(|_| Error::Invalid(crate::tr!("GitHub redirected to an invalid address")))
    }

    /// The text at `url`; `None` when it is 404.
    pub(super) async fn get_text(url: Url) -> Result<Option<String>> {
        let mut response = client()?.get(url).send().await.map_err(net)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(status(&response));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(net)? {
            if chunk.len() > MAX_FEED - bytes.len() {
                return Err(Error::Invalid(crate::tr!(
                    "GitHub's release feed is too large"
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| Error::Invalid(crate::tr!("GitHub's release feed isn't UTF-8")))
    }
}

/// Builds without `net` can't check.
#[cfg(not(feature = "net"))]
mod http {
    use crate::{Error, Result};
    use url::Url;

    fn offline() -> Error {
        Error::Io(std::io::Error::other(crate::tr!(
            "built without network support"
        )))
    }

    pub(super) async fn redirect(_url: Url) -> Result<Option<Url>> {
        Err(offline())
    }

    pub(super) async fn get_text(_url: Url) -> Result<Option<String>> {
        Err(offline())
    }
}

use http::{get_text, redirect};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_entries_tell_reused_tags_apart_by_their_time() {
        let feed = r#"<feed><entry><id>1</id><updated>2026-10-28T01:00:00Z</updated>
            <link rel="alternate" type="text/html" href="https://github.com/o/e/releases/tag/preview"/></entry>
            <entry><updated>2026-10-25T09:30:00Z</updated>
            <link rel="alternate" type="text/html" href="https://github.com/o/e/releases/tag/latest"/></entry></feed>"#;
        assert_eq!(
            entry_updated(feed, "/o/e/releases/tag/latest").as_deref(),
            Some("2026-10-25T09:30:00Z")
        );
        assert_eq!(entry_updated(feed, "/o/e/releases/tag/late"), None);
    }

    fn urls(list: &[&str]) -> Vec<Url> {
        list.iter().map(|url| Url::parse(url).unwrap()).collect()
    }

    #[test]
    fn follows_repositories_releases_and_their_files() {
        let github = GitHub::default();
        let follow = |list: &[&str]| github.follow(&urls(list));
        assert_eq!(
            follow(&["https://github.com/Owner/Game.git"]),
            Some(Followed {
                project: "Owner/Game".into(),
                version: None,
                asset: None
            })
        );
        assert_eq!(
            follow(&["https://github.com/o/g/releases/tag/release%2F1.2"]).and_then(|f| f.version),
            Some("release/1.2".into())
        );
        // The file downloaded comes first and names the release.
        assert_eq!(
            follow(&[
                "https://objects.githubusercontent.com/github-production-release-asset/1/2",
                "https://github.com/o/g/releases/download/v1.2/Game-1.2-linux.zip",
                "https://github.com/o/g",
            ]),
            Some(Followed {
                project: "o/g".into(),
                version: Some("v1.2".into()),
                asset: Some("Game-1.2-linux.zip".into())
            })
        );
        assert_eq!(
            follow(&["https://github.com/o/g/releases/latest/download/game.AppImage"])
                .and_then(|f| f.asset),
            Some("game.AppImage".into())
        );
        for other in [
            "https://github.com/o/g/archive/refs/tags/v1.zip",
            "https://github.com/o",
            "https://example.com/o/g",
            "https://github.com/o/../g",
        ] {
            assert_eq!(follow(&[other]), None, "{other}");
        }
    }

    #[test]
    fn the_feed_s_first_entry_is_the_newest_release() {
        let feed = r#"<?xml version="1.0"?><feed><link rel="alternate" href="https://github.com/o/g/releases"/>
          <entry><id>1</id><link rel="alternate" type="text/html" href="https://github.com/o/g/releases/tag/v2.0-beta"/></entry>
          <entry><link rel="alternate" type="text/html" href="https://github.com/o/g/releases/tag/v1.9"/></entry></feed>"#;
        assert_eq!(
            newest_entry(feed).as_deref(),
            Some("https://github.com/o/g/releases/tag/v2.0-beta")
        );
        assert_eq!(newest_entry("<feed></feed>"), None);
    }
}

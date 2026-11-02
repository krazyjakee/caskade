//! What an install source's newest version is and where its file is
//! (EMULATOR-SETUP-PLAN §2.1).

use crate::{Error, Result, emulation::InstallSource, updates::GitHub};
use url::Url;

/// Where Caskade looks for releases.
#[derive(Debug, Clone)]
pub struct Hosts {
    pub github: GitHub,
    /// Dolphin's update endpoint, ending in `/`.
    pub dolphin: Url,
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            github: GitHub::default(),
            dolphin: Url::parse("https://dolphin-emu.org/update/latest/beta/").expect("valid URL"),
        }
    }
}

/// A source's newest version and the file to install it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// `None` for sources that don't tell (`url`). Rolling GitHub releases
    /// are `<tag>@<published>`, so a rebuild under the same tag differs.
    pub version: Option<String>,
    pub url: Url,
    /// The file's name.
    pub file: String,
}

/// The tag without a leading `v`, for `{version}` in URL templates.
fn bare(tag: &str) -> &str {
    tag.strip_prefix(['v', 'V'])
        .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        .unwrap_or(tag)
}

fn file_name(url: &Url) -> Result<String> {
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|name| !name.is_empty())
        .and_then(|name| {
            percent_encoding::percent_decode_str(name)
                .decode_utf8()
                .ok()
        })
        .map(|name| name.into_owned())
        // Decoded, the name may hold `/`, `..` or a whole path, and it is
        // joined onto the download cache (#157).
        .filter(|name| {
            !matches!(name.as_str(), "." | "..") && !name.contains(['/', '\\', ':', '\0'])
        })
        .ok_or_else(|| Error::Invalid(crate::tr!("{url} names no file", url = url)))
}

/// Resolves `source`. Flatpak sources have nothing to resolve.
pub async fn resolve(hosts: &Hosts, source: &InstallSource) -> Result<Resolved> {
    match source {
        InstallSource::Github {
            repo,
            asset,
            url,
            rolling,
        } => {
            let github = &hosts.github;
            let tag = github
                .latest_tag(repo)
                .await?
                .ok_or_else(|| Error::Invalid(crate::tr!("{repo} has no release", repo = repo)))?;
            let version = if *rolling {
                match github.published(repo, &tag).await? {
                    Some(published) => format!("{tag}@{published}"),
                    None => tag.clone(),
                }
            } else {
                tag.clone()
            };
            let url = match (asset, url) {
                (_, Some(template)) => Url::parse(
                    &template
                        .replace("{version}", bare(&tag))
                        .replace("{tag}", &tag),
                )
                .map_err(|error| Error::Invalid(format!("{repo}: {error}")))?,
                (Some(pattern), None) => {
                    let pattern = regex::Regex::new(pattern)
                        .map_err(|error| Error::Invalid(error.to_string()))?;
                    let assets = github.assets(repo, &tag).await?;
                    let name = assets
                        .iter()
                        .find(|name| pattern.is_match(name))
                        .ok_or_else(|| {
                            Error::Invalid(crate::tr!(
                                "{repo} {tag} has no file for this system (looked for {pattern})",
                                repo = repo,
                                tag = tag,
                                pattern = pattern
                            ))
                        })?;
                    github.file(repo, &tag, name)?
                }
                (None, None) => {
                    return Err(Error::Invalid(crate::tr!(
                        "{repo}: no file named",
                        repo = repo
                    )));
                }
            };
            Ok(Resolved {
                version: Some(version),
                file: file_name(&url)?,
                url,
            })
        }
        InstallSource::Dolphin { artifact } => dolphin(&hosts.dolphin, artifact).await,
        InstallSource::Url { url } => {
            let url = Url::parse(url).map_err(|error| Error::Invalid(error.to_string()))?;
            Ok(Resolved {
                version: None,
                file: file_name(&url)?,
                url,
            })
        }
        InstallSource::Flatpak { app } => Err(Error::Invalid(crate::tr!(
            "{app} installs through Flatpak, not a download",
            app = app
        ))),
    }
}

/// Dolphin's update endpoint: `{"shortrev": "2609a", "artifacts":
/// [{"system": "Windows x64", "url": "…"}, …]}`.
#[cfg(feature = "net")]
async fn dolphin(endpoint: &Url, artifact: &str) -> Result<Resolved> {
    #[derive(serde::Deserialize)]
    struct Latest {
        shortrev: String,
        artifacts: Vec<Artifact>,
    }
    #[derive(serde::Deserialize)]
    struct Artifact {
        system: String,
        url: String,
    }
    use super::download::{client, net};
    // A few KiB. The download client has no total timeout (releases are
    // large), so this small request brings its own.
    const MAX_BODY: usize = 1 << 20;
    let mut response = client()?
        .get(endpoint.clone())
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(net)?
        .error_for_status()
        .map_err(net)?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(net)? {
        if chunk.len() > MAX_BODY - body.len() {
            return Err(Error::Invalid(crate::tr!(
                "Dolphin's update endpoint sent more than 1 MiB"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    let latest: Latest = serde_json::from_slice(&body)?;
    let found = latest
        .artifacts
        .iter()
        .find(|candidate| candidate.system == artifact)
        .ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "Dolphin {version} has no {artifact} build",
                version = latest.shortrev,
                artifact = artifact
            ))
        })?;
    let url = Url::parse(&found.url).map_err(|error| Error::Invalid(error.to_string()))?;
    // The build is downloaded and run unchecked: never over a weaker
    // scheme than the endpoint's own (#157).
    if url.scheme() != "https" && url.scheme() != endpoint.scheme() {
        return Err(Error::Invalid(crate::tr!(
            "Dolphin's update endpoint named a {scheme} download",
            scheme = url.scheme()
        )));
    }
    Ok(Resolved {
        version: Some(latest.shortrev),
        file: file_name(&url)?,
        url,
    })
}

#[cfg(not(feature = "net"))]
async fn dolphin(_endpoint: &Url, _artifact: &str) -> Result<Resolved> {
    Err(Error::Io(std::io::Error::other(
        "built without network support",
    )))
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn templates_take_the_tag_without_its_v() {
        assert_eq!(bare("v1.22.2"), "1.22.2");
        assert_eq!(bare("2126.2"), "2126.2");
        assert_eq!(bare("v.0.20.0"), "v.0.20.0");
        assert_eq!(bare("version"), "version");
        let url = Url::parse("https://x.test/stable/1.22.2/RetroArch%20Metal.dmg").unwrap();
        assert_eq!(file_name(&url).unwrap(), "RetroArch Metal.dmg");
    }

    /// A file name decodes to one name inside the cache, never a path (#157).
    #[test]
    fn file_names_cannot_leave_the_download_cache() {
        for url in [
            "https://x.test/a/..%2F..%2F.config%2Fautostart%2Fa.desktop",
            "https://x.test/a/%2Fhome%2Fu%2F.bashrc",
            "https://x.test/a/..%5C..%5Cevil.exe",
            "https://x.test/a/C%3A%5Cevil.exe",
            "https://x.test/a/%2E%2E",
            "https://x.test/a/evil%00.exe",
        ] {
            let url = Url::parse(url).unwrap();
            assert!(file_name(&url).is_err(), "{url}");
        }
    }
}

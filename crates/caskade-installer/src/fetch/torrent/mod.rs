//! BitTorrent inputs (#159): magnet links and `.torrent` files, downloaded
//! with [librqbit](https://github.com/ikatson/rqbit) and then installed like
//! any other download.
//!
//! A job's torrent runs in a session of its own that ends with the job: no
//! listener, UPnP or local discovery, no session or DHT state on disk, and
//! nothing is seeded once the download is complete. Data lives under
//! `<root>/.caskade/torrents/<info hash>/`, so a later job for the same
//! torrent (after a restart too) verifies the pieces already there and
//! fetches only the rest. Names in the metadata are untrusted: every path
//! component passes the staging name rules, and counts, depth and sizes are
//! limited before anything is written. See the crate README, "Torrents".

mod listing;
mod meta;
mod run;

use std::{
    io,
    net::SocketAddr,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use librqbit::{DhtSessionConfig, Magnet, Session, SessionOptions, limits::LimitsConfig};
use tokio_util::sync::CancellationToken;
use url::Url;

pub use meta::is_metadata;

use super::{
    FetchError, Result,
    engine::job_lock,
    http, safe_file_name,
    storage::{self, Storage},
};
use crate::{
    Question,
    disk::DiskSpace,
    events::{EventSink, JobEvent, Phase},
};

/// What a torrent download may do, and its limits.
#[derive(Debug, Clone)]
pub struct TorrentConfig {
    /// Find peers, and a magnet link's metadata, through the mainline DHT.
    /// Private torrents never use it.
    pub dht: bool,
    /// Announce to the torrent's trackers (a private torrent: its first).
    pub trackers: bool,
    /// Peers to try besides those the DHT and trackers name.
    pub initial_peers: Vec<SocketAddr>,
    /// Upload cap while the download runs, in bytes per second; 0 uploads
    /// nothing. Nothing is uploaded once the job's download ends.
    pub upload_limit_bps: u32,
    /// Peers connected at once.
    pub peer_limit: usize,
    /// How long a magnet link may wait for its metadata.
    pub metadata_timeout: Duration,
    /// How long a download may go without a verified byte.
    pub stall_timeout: Duration,
    /// Largest `.torrent` file (or magnet metadata) read.
    pub max_metadata_bytes: u64,
    pub max_files: usize,
    /// Most path components in one file's name.
    pub max_depth: usize,
    /// Largest total of the torrent's files.
    pub max_bytes: u64,
    /// Free space kept beyond what the download needs.
    pub disk_margin_bytes: u64,
}

impl Default for TorrentConfig {
    fn default() -> Self {
        Self {
            dht: true,
            trackers: true,
            initial_peers: Vec::new(),
            upload_limit_bps: 256 * 1024,
            peer_limit: 50,
            metadata_timeout: Duration::from_secs(180),
            stall_timeout: Duration::from_secs(300),
            max_metadata_bytes: 16 * 1024 * 1024,
            max_files: 10_000,
            max_depth: 32,
            max_bytes: 100 * 1024 * 1024 * 1024,
            disk_margin_bytes: 16 * 1024 * 1024,
        }
    }
}

/// A torrent input. An HTTP(S) link to a `.torrent` downloads as a file
/// first and becomes a [`Source::File`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Magnet(Url),
    File(PathBuf),
}

impl Source {
    /// A `magnet:` link, or a path that ends in `.torrent`.
    pub fn parse(input: &str) -> Option<Self> {
        if let Ok(url) = Url::parse(input) {
            if url.scheme() == "magnet" {
                return Some(Self::Magnet(url));
            }
            // `C:\…` parses with the scheme `c`; any longer scheme is a URL.
            if url.scheme().len() > 1 {
                return None;
            }
        }
        let path = Path::new(input);
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("torrent"))
            .then(|| Self::File(path.into()))
    }
}

/// `url` with only its info hashes and display name, for install records:
/// tracker URLs can carry a private tracker's passkey.
pub fn public_magnet(url: &Url) -> Url {
    let mut public = Url::parse("magnet:").expect("a magnet URL");
    {
        let mut pairs = public.query_pairs_mut();
        for (key, value) in url.query_pairs() {
            if key == "xt" || key == "dn" {
                pairs.append_pair(&key, &value);
            }
        }
    }
    public
}

/// A finished torrent download.
#[derive(Debug, Clone)]
pub struct DownloadedTorrent {
    /// What to install: the chosen file or folder, or the whole torrent.
    pub path: PathBuf,
    /// Lowercase hex v1 info hash.
    pub info_hash: String,
    /// The torrent's name.
    pub name: String,
    /// Bytes downloaded and verified for the selection.
    pub bytes: u64,
}

/// What [`TorrentEngine::download`] ended with.
#[derive(Debug)]
pub enum Fetched {
    Downloaded(DownloadedTorrent),
    /// The torrent holds several payloads: [`Question::ChooseAsset`], the
    /// first asset being everything. Ask again with the answer.
    NeedsUser(Question),
}

/// Downloads torrents under a storage root's `.caskade/torrents`.
#[derive(Clone)]
pub struct TorrentEngine {
    config: TorrentConfig,
    disk: Arc<dyn DiskSpace>,
    sink: Arc<dyn EventSink>,
}

impl TorrentEngine {
    pub fn new(config: TorrentConfig, disk: Arc<dyn DiskSpace>, sink: Arc<dyn EventSink>) -> Self {
        Self { config, disk, sink }
    }

    /// Downloads `source` for job `job_id` into `root`. `answer` answers
    /// the payload question from an earlier run. Cancelling stops all of
    /// the torrent's network activity; its verified pieces stay.
    #[tracing::instrument(skip_all, fields(job_id = job_id))]
    pub async fn download(
        &self,
        root: &Path,
        job_id: &str,
        source: &Source,
        answer: &(dyn Fn(&Question) -> Option<usize> + Send + Sync),
        cancellation: CancellationToken,
    ) -> Result<Fetched> {
        safe_file_name(job_id)?;
        let root = dunce::canonicalize(root)?;
        let capability = crate::safety::open_staging(&root)?;
        let token = cancellation.child_token();
        let _stop = token.clone().drop_guard();
        let report = Report {
            sink: &self.sink,
            job_id,
            token: &token,
        };
        // The info hash names the data folder: read a .torrent first.
        let (hash, file) = match source {
            Source::Magnet(url) => {
                let magnet = Magnet::parse(url.as_str()).map_err(|error| unusable(&error))?;
                let id = magnet.as_id20().ok_or_else(|| FetchError::Torrent {
                    message: "the magnet link has no BitTorrent v1 info hash (v2-only torrents aren't supported)".into(),
                })?;
                (id.as_string(), None)
            }
            Source::File(path) => {
                report.emit("Reading the torrent", 0, None, None, 0)?;
                let bytes = meta::read_limited(path, self.config.max_metadata_bytes)?;
                (meta::hash_of(&bytes)?, Some(bytes))
            }
        };
        if !storage::info_hash(&hash) {
            return Err(FetchError::Torrent {
                message: "invalid info hash".into(),
            });
        }
        let relative = Path::new(storage::TORRENTS).join(&hash);
        let lock = job_lock(&root.join(&relative));
        let _guard = tokio::select! {
            _ = token.cancelled() => return Err(FetchError::Cancelled),
            guard = lock.lock_owned() => guard,
        };
        let folder = storage::directory(&capability, &relative, true)?;
        Storage::open(&root, job_id)?.mark_torrent(&hash)?;
        storage::directory(&folder, Path::new("data"), true)?;
        let bytes = match file {
            Some(bytes) => Some(bytes),
            None => meta::cached(&folder, &hash, self.config.max_metadata_bytes),
        };
        let data = root.join(&relative).join("data");
        let session = self.session(&data, &token).await?;
        let run = run::Run {
            engine: self,
            session: &session,
            root: &root,
            hash: &hash,
            folder: &folder,
            data: &data,
            report: &report,
        };
        let result = run.run(source, bytes, answer).await;
        // Ends every connection, tracker announce and DHT query: nothing
        // is seeded after the job.
        session.stop().await;
        result
    }

    async fn session(&self, data: &Path, token: &CancellationToken) -> Result<Arc<Session>> {
        // librqbit's tracker client is a rustls client without a provider
        // of its own: use ring, which Caskade builds anyway.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Session::new_with_opts(
            data.to_path_buf(),
            SessionOptions {
                dht: self.config.dht.then_some(DhtSessionConfig {
                    bootstrap_addrs: None,
                    port: None,
                    persistence: None,
                }),
                disable_trackers: !self.config.trackers,
                fastresume: false,
                persistence: None,
                listen: None,
                cancellation_token: Some(token.child_token()),
                ratelimits: LimitsConfig {
                    upload_bps: NonZeroU32::new(self.config.upload_limit_bps),
                    download_bps: None,
                },
                peer_limit: Some(self.config.peer_limit),
                disable_upload: self.config.upload_limit_bps == 0,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| engine_error(&error))
    }
}

struct Report<'a> {
    sink: &'a Arc<dyn EventSink>,
    job_id: &'a str,
    token: &'a CancellationToken,
}

impl Report<'_> {
    fn emit(
        &self,
        message: &str,
        bytes: u64,
        total: Option<u64>,
        eta: Option<u64>,
        files: u64,
    ) -> Result<()> {
        http::cancelled(self.token)?;
        self.sink.emit(JobEvent {
            job_id: self.job_id.into(),
            phase: Phase::Download,
            step: None,
            bytes,
            total_bytes: total,
            files,
            eta_secs: eta,
            message: message.into(),
        });
        http::cancelled(self.token)
    }
}

/// An error from the engine, as an I/O error when one caused it (so a full
/// disk, a refused folder or a removed drive classify as such, #158).
fn engine_error<E>(error: &E) -> FetchError
where
    E: AsRef<dyn std::error::Error + Send + Sync + 'static> + std::fmt::Display,
{
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error.as_ref());
    while let Some(current) = cause {
        if let Some(io) = current.downcast_ref::<io::Error>() {
            return FetchError::Io(match io.raw_os_error() {
                Some(code) => io::Error::from_raw_os_error(code),
                None => io::Error::new(io.kind(), io.to_string()),
            });
        }
        cause = current.source();
    }
    FetchError::Torrent {
        message: format!("{error:#}"),
    }
}

/// Metadata the engine couldn't read.
fn unusable(error: &impl std::fmt::Display) -> FetchError {
    FetchError::Torrent {
        message: format!("{error:#}"),
    }
}

fn utf8(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| FetchError::InvalidRequest {
        message: "the storage folder's path isn't valid UTF-8".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_are_magnet_links_and_torrent_files() {
        assert!(matches!(
            Source::parse("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"),
            Some(Source::Magnet(_))
        ));
        assert_eq!(
            Source::parse("/games/Game.TORRENT"),
            Some(Source::File("/games/Game.TORRENT".into()))
        );
        assert_eq!(
            Source::parse(r"C:\games\game.torrent"),
            Some(Source::File(r"C:\games\game.torrent".into()))
        );
        assert_eq!(Source::parse("https://example.test/game.torrent"), None);
        assert_eq!(Source::parse("/games/game.zip"), None);
    }

    #[test]
    fn recorded_magnet_links_lose_their_trackers() {
        let url = Url::parse(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Game&tr=https%3A%2F%2Ftracker.test%2Fpasskey%2Fannounce",
        )
        .unwrap();
        let public = public_magnet(&url);
        assert!(!public.as_str().contains("tracker"), "{public}");
        assert_eq!(
            librqbit::Magnet::parse(public.as_str())
                .unwrap()
                .as_id20()
                .unwrap()
                .as_string(),
            "0123456789abcdef0123456789abcdef01234567"
        );
    }
}

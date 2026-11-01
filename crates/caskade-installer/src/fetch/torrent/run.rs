//! One job's download in its own session: metadata, the payload choice,
//! the space check, progress and the check of the finished files.

use std::{
    io,
    net::SocketAddr,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use cap_std::fs::Dir;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrentState, Session,
    TorrentStatsState, limits::LimitsConfig,
};
use url::Url;

use super::{
    DownloadedTorrent, Fetched, Report, Source, TorrentEngine, engine_error,
    listing::{Listed, Listing},
    meta, unusable, utf8,
};
use crate::{
    Question,
    fetch::{FetchError, Result, storage},
};

pub(super) struct Run<'a> {
    pub(super) engine: &'a TorrentEngine,
    pub(super) session: &'a Arc<Session>,
    pub(super) root: &'a Path,
    pub(super) hash: &'a str,
    pub(super) folder: &'a Dir,
    pub(super) data: &'a Path,
    pub(super) report: &'a Report<'a>,
}

impl Run<'_> {
    pub(super) async fn run(
        &self,
        source: &Source,
        bytes: Option<Vec<u8>>,
        answer: &(dyn Fn(&Question) -> Option<usize> + Send + Sync),
    ) -> Result<Fetched> {
        let config = &self.engine.config;
        let mut peers = config.initial_peers.clone();
        let bytes = match (bytes, source) {
            (Some(bytes), _) => bytes,
            (None, Source::Magnet(url)) => {
                let (bytes, seen) = self.metadata(url).await?;
                peers.extend(seen);
                meta::write_cached(self.folder, &bytes)?;
                bytes
            }
            (None, Source::File(_)) => unreachable!("a .torrent file is read first"),
        };
        let data = utf8(self.data)?;
        // Lists the files without any network activity.
        let listed = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(bytes.clone()),
                Some(AddTorrentOptions {
                    list_only: true,
                    output_folder: Some(data.into()),
                    ..Default::default()
                }),
            )
            .await
            .map_err(|error| unusable(&error))?;
        let AddTorrentResponse::ListOnly(listed) = listed else {
            return Err(FetchError::Torrent {
                message: "the engine didn't list the torrent".into(),
            });
        };
        if listed.info_hash.as_string() != self.hash {
            return Err(FetchError::Torrent {
                message: "the metadata doesn't match the info hash".into(),
            });
        }
        let listing = Listing::inspect(&listed.info, self.hash, config)?;
        let groups = listing.groups();
        let selection = if groups.len() > 1 {
            let question = listing.question(self.hash, &groups);
            match answer(&question) {
                None => return Ok(Fetched::NeedsUser(question)),
                Some(0) => None,
                Some(index) => {
                    Some(
                        groups
                            .get(index - 1)
                            .ok_or_else(|| FetchError::InvalidRequest {
                                message: "torrent choice is out of range".into(),
                            })?,
                    )
                }
            }
        } else {
            None
        };
        let output = if listing.multi {
            Path::new("data").join(&listing.name)
        } else {
            PathBuf::from("data")
        };
        storage::directory(self.folder, &output, true)?;
        let selected: Vec<&Listed> = listing
            .files
            .iter()
            .filter(|file| selection.is_none_or(|group| group.files.contains(&file.index)))
            .collect();
        let total = selected.iter().map(|file| file.len).sum::<u64>();
        self.check_space(&output, &selected)?;
        let base = self.root.join(storage::TORRENTS).join(self.hash);
        let target = utf8(&base.join(&output))?.to_owned();

        let added = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(bytes),
                Some(AddTorrentOptions {
                    only_files: selection.map(|group| group.files.clone()),
                    // Resume over the job's own earlier data, verified first.
                    overwrite: true,
                    output_folder: Some(target),
                    initial_peers: Some(peers),
                    peer_limit: Some(config.peer_limit),
                    disable_trackers: !config.trackers,
                    ratelimits: LimitsConfig {
                        upload_bps: NonZeroU32::new(config.upload_limit_bps),
                        download_bps: None,
                    },
                    ..Default::default()
                }),
            )
            .await
            .map_err(|error| engine_error(&error))?;
        let handle = match added {
            AddTorrentResponse::Added(_, handle)
            | AddTorrentResponse::AlreadyManaged(_, handle) => handle,
            AddTorrentResponse::ListOnly(_) => {
                return Err(FetchError::Torrent {
                    message: "the engine didn't start the torrent".into(),
                });
            }
        };

        let mut tick = tokio::time::interval(Duration::from_millis(500));
        let mut last = (0, Instant::now());
        loop {
            tokio::select! {
                _ = self.report.token.cancelled() => return Err(FetchError::Cancelled),
                _ = tick.tick() => {}
            }
            if let Some(error) = handle.with_state(|state| match state {
                ManagedTorrentState::Error(error) => Some(engine_error(error)),
                _ => None,
            }) {
                return Err(error);
            }
            let stats = handle.stats();
            if let TorrentStatsState::Initializing { .. } = stats.state {
                // Pieces already on disk are checked before they count.
                last = (0, Instant::now());
                self.report.emit(
                    "Verifying downloaded pieces",
                    stats.progress_bytes,
                    Some(stats.total_bytes),
                    None,
                    0,
                )?;
                continue;
            }
            if stats.finished {
                break;
            }
            let connected = stats
                .live
                .as_ref()
                .map_or(0, |live| live.snapshot.peer_stats.live);
            if stats.progress_bytes > last.0 {
                last = (stats.progress_bytes, Instant::now());
            } else if last.1.elapsed() >= config.stall_timeout {
                return Err(FetchError::NoPeers {
                    metadata: false,
                    secs: last.1.elapsed().as_secs(),
                    peers: connected,
                });
            }
            let rate = stats.live.as_ref().map_or(0, |live| {
                (live.download_speed.mbps * 1024.0 * 1024.0).max(0.0) as u64
            });
            let message = if connected == 0 {
                "Connecting to peers".to_owned()
            } else {
                format!(
                    "Downloading from {connected} peer{} at {}/s",
                    if connected == 1 { "" } else { "s" },
                    size(rate)
                )
            };
            let eta =
                (rate > 0).then(|| stats.total_bytes.saturating_sub(stats.progress_bytes) / rate);
            self.report.emit(
                &message,
                stats.progress_bytes,
                Some(stats.total_bytes),
                eta,
                0,
            )?;
        }

        // Done: stop sharing and close the files before checking them.
        if let Err(error) = self.session.delete(handle.id().into(), false).await {
            tracing::debug!(%error, "torrent not removed from its session");
        }
        // Only verified, present, regular files go on to the installer.
        self.report
            .emit("Checking downloaded files", total, Some(total), None, 0)?;
        for file in &selected {
            let path = output.join(&file.path);
            let parent = path.parent().unwrap_or(Path::new(""));
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let dir = storage::directory(self.folder, parent, false)?;
            match storage::regular_len(&dir, &name)? {
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("“{}” is gone after downloading", file.path.display()),
                    )
                    .into());
                }
                Some(len) if len != file.len => {
                    return Err(FetchError::Torrent {
                        message: format!("“{}” changed after it was verified", file.path.display()),
                    });
                }
                Some(_) => {}
            }
        }
        let payload = match selection {
            Some(group) => output.join(&group.payload),
            None if listing.multi => output,
            None => output.join(&listing.files[0].path),
        };
        self.report.emit(
            "Download complete",
            total,
            Some(total),
            None,
            selected.len() as u64,
        )?;
        Ok(Fetched::Downloaded(DownloadedTorrent {
            path: base.join(payload),
            info_hash: self.hash.to_owned(),
            name: listing.name,
            bytes: total,
        }))
    }

    /// A magnet link's metadata from its peers, and the peers that sent it.
    async fn metadata(&self, url: &Url) -> Result<(Vec<u8>, Vec<SocketAddr>)> {
        let config = &self.engine.config;
        let trackers = config.trackers
            && url
                .query_pairs()
                .any(|(key, value)| key == "tr" && !value.is_empty());
        if !config.dht && !trackers && config.initial_peers.is_empty() {
            return Err(FetchError::NoPeers {
                metadata: true,
                secs: 0,
                peers: 0,
            });
        }
        self.report
            .emit("Finding peers for the torrent's metadata", 0, None, None, 0)?;
        let data = utf8(self.data)?.to_owned();
        let started = Instant::now();
        let listed = tokio::select! {
            _ = self.report.token.cancelled() => return Err(FetchError::Cancelled),
            listed = tokio::time::timeout(
                config.metadata_timeout,
                self.session.add_torrent(
                    AddTorrent::from_url(url.as_str()),
                    Some(AddTorrentOptions {
                        list_only: true,
                        output_folder: Some(data),
                        initial_peers: Some(config.initial_peers.clone()),
                        disable_trackers: !config.trackers,
                        ..Default::default()
                    }),
                ),
            ) => listed,
        };
        let no_peers = || FetchError::NoPeers {
            metadata: true,
            secs: started.elapsed().as_secs(),
            peers: 0,
        };
        let listed = match listed {
            Err(_) => return Err(no_peers()),
            // Every peer the DHT, trackers and caller named was tried.
            Ok(Err(error)) if format!("{error:#}").contains("no way to discover") => {
                return Err(no_peers());
            }
            Ok(result) => result.map_err(|error| engine_error(&error))?,
        };
        let AddTorrentResponse::ListOnly(listed) = listed else {
            return Err(FetchError::Torrent {
                message: "the engine didn't list the torrent".into(),
            });
        };
        if listed.torrent_bytes.len() as u64 > config.max_metadata_bytes {
            return Err(FetchError::Torrent {
                message: format!(
                    "the metadata is larger than {} bytes",
                    config.max_metadata_bytes
                ),
            });
        }
        Ok((listed.torrent_bytes.to_vec(), listed.seen_peers))
    }

    /// Room for what the selection still needs, plus the margin.
    fn check_space(&self, output: &Path, selected: &[&Listed]) -> Result<()> {
        let base = self.root.join(storage::TORRENTS).join(self.hash);
        let mut required = self.engine.config.disk_margin_bytes;
        for file in selected {
            let present = std::fs::symlink_metadata(base.join(output).join(&file.path))
                .ok()
                .filter(|metadata| metadata.is_file())
                .map_or(0, |metadata| metadata.len().min(file.len));
            required = required.saturating_add(file.len - present);
        }
        let available = self.engine.disk.available_bytes(self.root)?;
        if available < required {
            return Err(FetchError::DiskFull {
                available,
                required,
            });
        }
        Ok(())
    }
}

/// `1.5 MiB` and the like.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

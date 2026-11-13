//! The download cache: on-demand downloads at launch, verification,
//! unpacking, the atomic commit and LRU eviction.
//!
//! Layout under the cache folder:
//!
//! - `<server>/<rom id>/…` – a committed ROM, exactly as the emulator gets it.
//! - `.staging/<server>-<rom id>/download.part` – a download in progress;
//!   kept when interrupted so the next launch resumes it.
//! - `.staging/<server>-<rom id>/files/` – the ROM being assembled, renamed
//!   to its final folder only once it is complete and verified.
#![cfg_attr(not(feature = "net"), allow(dead_code, unused_imports))]

use super::{
    CacheEntry, CacheStatus, CachedRom, DownloadPhase, DownloadProgress, RemoteFile, RemoteRom,
    Result, RommError, RommGameStatus, RommRom, RommServerConfig, RommService, RommSettings, keyed,
    lock, now_ms,
    sync::{game_roms, target_for},
};
use crate::{
    bus::Event,
    model::{InstallLocation, InstallState, LaunchSpec},
};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Archives the cache unpacks when the platform doesn't list them.
const ARCHIVES: &[&str] = &["zip", "7z", "tar", "gz", "tgz", "xz", "zst", "bz2"];
/// Disc images and playlists, preferred over the track files they name.
const DISCS: &[&str] = &[
    "m3u", "cue", "gdi", "ccd", "chd", "pbp", "iso", "cso", "rvz", "gcz", "wbfs", "cdi", "mds",
];

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub(super) async fn remove_dir(dir: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(dir).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(dir = %dir.display(), %error, "RomM cache folder not removed");
    }
}

/// A hash RomM gives for a file.
#[derive(Debug, Clone, Copy)]
enum HashKind {
    Sha1,
    Md5,
    Crc32,
}

impl HashKind {
    fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA-1",
            Self::Md5 => "MD5",
            Self::Crc32 => "CRC32",
        }
    }
}

/// The strongest hash the server gave, lowercase.
fn strongest(expected: &RemoteFile) -> Option<(HashKind, String)> {
    let wanted = |hash: &Option<String>| {
        hash.as_deref()
            .map(str::trim)
            .filter(|hash| !hash.is_empty())
            .map(str::to_ascii_lowercase)
    };
    wanted(&expected.sha1_hash)
        .map(|hash| (HashKind::Sha1, hash))
        .or_else(|| wanted(&expected.md5_hash).map(|hash| (HashKind::Md5, hash)))
        .or_else(|| wanted(&expected.crc_hash).map(|hash| (HashKind::Crc32, hash)))
}

/// The hash of `paths`' bytes, one after the other.
fn digest(kind: HashKind, paths: &[PathBuf]) -> Result<String> {
    use std::io::Read;
    let mut buffer = vec![0u8; 1 << 20];
    let mut feed = |update: &mut dyn FnMut(&[u8])| -> std::io::Result<()> {
        for path in paths {
            let mut file = std::fs::File::open(path)?;
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                update(&buffer[..read]);
            }
        }
        Ok(())
    };
    Ok(match kind {
        HashKind::Sha1 => {
            use sha1::Digest;
            let mut hasher = sha1::Sha1::new();
            feed(&mut |bytes| hasher.update(bytes))?;
            format!("{:x}", hasher.finalize())
        }
        HashKind::Md5 => {
            use md5::Digest;
            let mut hasher = md5::Md5::new();
            feed(&mut |bytes| hasher.update(bytes))?;
            format!("{:x}", hasher.finalize())
        }
        HashKind::Crc32 => {
            let mut hasher = crc32fast::Hasher::new();
            feed(&mut |bytes| hasher.update(bytes))?;
            format!("{:08x}", hasher.finalize())
        }
    })
}

fn same(want: &str, got: &str) -> bool {
    want.trim_start_matches('0') == got.trim_start_matches('0')
}

fn mismatch(kind: HashKind, expected: &RemoteFile) -> RommError {
    RommError::Verify(crate::tr!(
        "{checksum} for {file}",
        checksum = kind.name(),
        file = expected.file_name
    ))
}

/// Archive members RomM leaves out of its hashes (`DEFAULT_EXCLUDED_FILES`
/// and `DEFAULT_EXCLUDED_EXTENSIONS` in its config).
fn excluded_member(path: &Path) -> bool {
    const NAMES: [&str; 7] = [
        ".DS_Store",
        ".localized",
        ".Trashes",
        ".stfolder",
        "@SynoResource",
        "gamelist.xml",
        "metadata.pegasus.txt",
    ];
    const EXTENSIONS: [&str; 9] = [
        "db",
        "tmp",
        "bak",
        "lock",
        "log",
        "cache",
        "crdownload",
        "part",
        "assembling",
    ];
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let lower = name.to_lowercase();
    NAMES.contains(&name.as_ref())
        || name.ends_with(":Zone.Identifier")
        || EXTENSIONS
            .iter()
            .any(|extension| lower.ends_with(&format!(".{extension}")))
}

/// Whether an archive's unpacked contents in `dir` have RomM's hash. RomM
/// hashes what an archive holds, not the archive: every member in path
/// order (RomM 4 and later), or the largest member (earlier versions).
fn contents_match(dir: &Path, kind: HashKind, want: &str) -> Result<bool> {
    let mut members: Vec<(String, PathBuf)> = walk(dir)?
        .into_iter()
        .filter(|path| !excluded_member(path))
        .map(|path| {
            let name = path
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            (name, dir.join(path))
        })
        .collect();
    if members.is_empty() {
        return Ok(false);
    }
    // Python sorts member names by code point; UTF-8 bytes sort the same.
    members.sort_by(|a, b| a.0.cmp(&b.0));
    let paths: Vec<PathBuf> = members.iter().map(|(_, path)| path.clone()).collect();
    if same(want, &digest(kind, &paths)?) {
        return Ok(true);
    }
    let size = |path: &PathBuf| std::fs::metadata(path).map_or(0, |m| m.len());
    let largest = paths
        .iter()
        .fold(None::<&PathBuf>, |best, path| match best {
            Some(best) if size(best) >= size(path) => Some(best),
            _ => Some(path),
        });
    match largest {
        Some(largest) if paths.len() > 1 => {
            Ok(same(want, &digest(kind, std::slice::from_ref(largest))?))
        }
        _ => Ok(false),
    }
}

fn check_size(path: &Path, expected: &RemoteFile, size: Option<u64>) -> Result<()> {
    let actual = std::fs::metadata(path)?.len();
    if let Some(size) = size.filter(|size| *size > 0)
        && size != actual
    {
        return Err(RommError::Verify(crate::tr!(
            "size for {file} ({actual} bytes, expected {size})",
            file = expected.file_name,
            actual = actual,
            size = size
        )));
    }
    Ok(())
}

/// Checks a file against the size and the strongest hash the server gave.
/// An archive whose own bytes don't have the hash is unpacked beside it
/// and checked by its contents, as RomM hashes them.
fn verify(path: &Path, expected: &RemoteFile, size: Option<u64>) -> Result<()> {
    check_size(path, expected, size)?;
    let Some((kind, want)) = strongest(expected) else {
        return Ok(());
    };
    if same(&want, &digest(kind, &[path.to_path_buf()])?) {
        return Ok(());
    }
    if ARCHIVES.contains(&extension(Path::new(&expected.file_name)).as_str()) {
        let mut check = path.as_os_str().to_owned();
        check.push(".contents");
        let check = PathBuf::from(check);
        let matched = unpack(path, &check).and_then(|()| contents_match(&check, kind, &want));
        let _ = std::fs::remove_dir_all(&check);
        match matched {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            // Out of space, refused or a drive gone: say so rather than
            // blame the download, which would only be fetched again (#160).
            Err(error) if environmental(&error) => return Err(error),
            // An archive that won't unpack doesn't match.
            Err(_) => {}
        }
    }
    Err(mismatch(kind, expected))
}

/// Whether `error` is about the machine (space, access, a drive), not the
/// file's contents.
fn environmental(error: &RommError) -> bool {
    use crate::error::{ErrorCode, ErrorReport};
    matches!(
        ErrorReport::from_error(error).code,
        ErrorCode::DiskFull | ErrorCode::AccessDenied | ErrorCode::StorageOffline | ErrorCode::Busy
    )
}

/// Every regular file under `dir`, relative to it, without following links.
fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        for item in std::fs::read_dir(dir.join(&relative))? {
            let item = item?;
            let kind = item.file_type()?;
            let path = relative.join(item.file_name());
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                files.push(path);
            }
            if files.len() > 100_000 {
                return Err(RommError::Extract(crate::tr!("too many files")));
            }
        }
    }
    files.sort();
    Ok(files)
}

fn unpack(archive: &Path, into: &Path) -> Result<()> {
    crate::managed::unpack::extract_into(archive, into)?.map_err(RommError::Extract)
}

/// What the emulator opens, relative to the ROM's folder: a playlist, the
/// disc images (an `.m3u` is written for several when the platform takes
/// one), or the files the platform runs.
fn launch_files(dir: &Path, rom: &RemoteRom, extensions: &[String]) -> Result<Vec<PathBuf>> {
    let files = walk(dir)?;
    let supports = |extension: &str| extensions.iter().any(|known| known == extension);
    // Shallowest first.
    let by_depth = |mut list: Vec<PathBuf>| {
        list.sort_by_key(|path| (path.components().count(), path.clone()));
        list
    };
    let playlists = by_depth(
        files
            .iter()
            .filter(|path| extension(path) == "m3u")
            .cloned()
            .collect(),
    );
    if supports("m3u")
        && let Some(playlist) = playlists.first()
    {
        return Ok(vec![playlist.clone()]);
    }
    let playable: Vec<PathBuf> = files
        .iter()
        .filter(|path| extensions.is_empty() || supports(&extension(path)))
        .cloned()
        .collect();
    let discs: Vec<PathBuf> = playable
        .iter()
        .filter(|path| DISCS.contains(&extension(path).as_str()))
        .cloned()
        .collect();
    let chosen = if discs.is_empty() { playable } else { discs };
    if chosen.is_empty() {
        // Nothing the platform names: the largest file.
        let largest = files
            .iter()
            .max_by_key(|path| std::fs::metadata(dir.join(path)).map_or(0, |m| m.len()))
            .cloned()
            .ok_or_else(|| RommError::Extract(crate::tr!("the download has no files")))?;
        return Ok(vec![largest]);
    }
    if chosen.len() > 1 && supports("m3u") {
        let stem = if rom.fs_name_no_ext.trim().is_empty() {
            "game".to_owned()
        } else {
            rom.fs_name_no_ext.replace(['/', '\\'], "_")
        };
        let playlist = PathBuf::from(format!("{stem}.m3u"));
        let text: String = chosen
            .iter()
            .map(|path| format!("{}\n", path.to_string_lossy().replace('\\', "/")))
            .collect();
        std::fs::write(dir.join(&playlist), text)?;
        return Ok(vec![playlist]);
    }
    Ok(chosen)
}

/// Turns `download.part` into the finished ROM folder `files/`, verified
/// and unpacked when needed. Returns what the emulator opens and the size.
fn assemble(
    part: &Path,
    files: &Path,
    rom: &RemoteRom,
    extensions: &[String],
    check: bool,
) -> Result<(Vec<PathBuf>, u64)> {
    if files.exists() {
        std::fs::remove_dir_all(files)?;
    }
    let name = Path::new(&rom.fs_name)
        .file_name()
        .map(PathBuf::from)
        .ok_or_else(|| RommError::Server(crate::tr!("the ROM has no file name")))?;
    let archive = ARCHIVES.contains(&extension(&name).as_str())
        && !extensions.iter().any(|known| *known == extension(&name));
    // The extractor recognises archives by content, but give it a plain
    // archive name rather than a `.part`.
    let unpack_part = |kind: &str| -> Result<()> {
        let archive = part.with_file_name(format!("download.{kind}"));
        std::fs::rename(part, &archive)?;
        let unpacked = unpack(&archive, files);
        let _ = std::fs::remove_file(&archive);
        unpacked
    };
    if rom.is_multi() {
        // The server zips the ROM's files.
        unpack_part("zip")?;
        if check {
            let found = walk(files)?;
            for expected in &rom.files {
                let candidates: Vec<&PathBuf> = found
                    .iter()
                    .filter(|path| {
                        path.file_name()
                            .is_some_and(|name| name.to_string_lossy() == expected.file_name)
                    })
                    .collect();
                let path = candidates
                    .iter()
                    .find(|path| {
                        std::fs::metadata(files.join(path))
                            .is_ok_and(|m| m.len() == expected.file_size_bytes)
                    })
                    .or(candidates.first())
                    .ok_or_else(|| {
                        RommError::Verify(crate::tr!(
                            "file list ({file} is missing)",
                            file = expected.file_name
                        ))
                    })?;
                verify(&files.join(path), expected, Some(expected.file_size_bytes))?;
            }
        }
    } else {
        if check {
            let expected = rom.files.first().cloned().unwrap_or_else(|| RemoteFile {
                file_name: rom.fs_name.clone(),
                file_size_bytes: rom.fs_size_bytes,
                crc_hash: rom.crc_hash.clone(),
                md5_hash: rom.md5_hash.clone(),
                sha1_hash: rom.sha1_hash.clone(),
                ..Default::default()
            });
            let size = Some(rom.fs_size_bytes).filter(|size| *size > 0);
            // An archive is checked by its contents once unpacked, unless
            // its own bytes have the hash (older RomM servers).
            let checked = if archive {
                check_size(part, &expected, size).map(|()| {
                    strongest(&expected).filter(|(kind, want)| {
                        !digest(*kind, &[part.to_path_buf()]).is_ok_and(|got| same(want, &got))
                    })
                })
            } else {
                verify(part, &expected, size).map(|()| None)
            };
            match checked {
                Ok(Some((kind, want))) => {
                    unpack_part(&extension(&name))?;
                    if !contents_match(files, kind, &want)? {
                        let _ = std::fs::remove_dir_all(files);
                        return Err(mismatch(kind, &expected));
                    }
                    return finish(files, rom, extensions);
                }
                Ok(None) => {}
                Err(error) => {
                    // Start over next time rather than resume a bad file.
                    let _ = std::fs::remove_file(part);
                    return Err(error);
                }
            }
        }
        if archive {
            unpack_part(&extension(&name))?;
        } else {
            std::fs::create_dir_all(files)?;
            std::fs::rename(part, files.join(&name))?;
        }
    }
    finish(files, rom, extensions)
}

/// What the emulator opens in the ROM's folder, and the folder's size.
fn finish(files: &Path, rom: &RemoteRom, extensions: &[String]) -> Result<(Vec<PathBuf>, u64)> {
    let roms = launch_files(files, rom, extensions)?;
    let size = walk(files)?
        .iter()
        .map(|path| std::fs::metadata(files.join(path)).map_or(0, |m| m.len()))
        .sum();
    Ok((roms, size))
}

impl RommService {
    pub(super) async fn cache_root(&self) -> Result<PathBuf> {
        let settings = self.settings().await?;
        self.cache_root_with(&settings).await
    }

    /// The configured folder, or `romm-cache/` on the default storage
    /// location.
    pub(super) async fn cache_root_with(&self, settings: &RommSettings) -> Result<PathBuf> {
        let configured = settings.cache.dir.trim();
        if !configured.is_empty() {
            return Ok(PathBuf::from(configured));
        }
        Ok(match self.inner.store.roots().await?.first() {
            Some(root) => root.path.join("romm-cache"),
            None => self.inner.fallback_cache.clone(),
        })
    }

    pub(super) async fn cache_status_with(&self, settings: &RommSettings) -> Result<CacheStatus> {
        let store = &self.inner.store;
        let mut games = Vec::new();
        for rom in store.romm_cached().await?.into_iter().rev() {
            let Some(cache) = &rom.cache else { continue };
            let game = store.game(&rom.game_id).await?;
            games.push(CacheEntry {
                server_id: rom.server_id.clone(),
                rom_id: rom.rom_id,
                entry_id: rom.entry_id.clone(),
                game_id: rom.game_id.clone(),
                title: game
                    .as_ref()
                    .map_or_else(|| rom.rom.title(), |game| game.title.clone()),
                size: cache.size,
                last_used_ms: cache.last_used_ms,
                pinned: rom.pinned,
                favorite: game.is_some_and(|game| game.favorite),
                orphaned: rom.orphaned,
            });
        }
        Ok(CacheStatus {
            dir: self.cache_root_with(settings).await?,
            used: games.iter().map(|game| game.size).sum(),
            limit: settings.cache.limit_bytes(),
            games,
        })
    }

    /// "Keep downloaded": a pinned game is never evicted.
    pub async fn set_pinned(&self, entry_id: &str, pinned: bool) -> Result<RommGameStatus> {
        let mut rom = self
            .inner
            .store
            .romm_rom_by_entry(entry_id)
            .await?
            .ok_or_else(|| RommError::Config(crate::tr!("not a RomM game")))?;
        rom.pinned = pinned;
        self.inner.store.save_romm_rom(&rom).await?;
        if !pinned {
            self.enforce_limit(None, 0).await?;
        }
        self.game_status(entry_id)
            .await?
            .ok_or_else(|| RommError::Config(crate::tr!("not a RomM game")))
    }

    /// Removes a game's download; it downloads again at the next launch.
    pub async fn evict(&self, entry_id: &str) -> Result<bool> {
        let Some(rom) = self.inner.store.romm_rom_by_entry(entry_id).await? else {
            return Ok(false);
        };
        if rom.cache.is_none() {
            return Ok(false);
        }
        let _guard = keyed(&self.inner.preparing, entry_id).lock_owned().await;
        self.evict_rom(rom).await?;
        Ok(true)
    }

    /// Removes every download except pinned games.
    pub async fn clear_cache(&self) -> Result<CacheStatus> {
        for rom in self.inner.store.romm_cached().await? {
            if !rom.pinned && !self.busy(&rom.entry_id).await? {
                let _guard = keyed(&self.inner.preparing, &rom.entry_id)
                    .lock_owned()
                    .await;
                self.evict_rom(rom).await?;
            }
        }
        let settings = self.settings().await?;
        self.cache_status_with(&settings).await
    }

    /// Downloading or running now.
    async fn busy(&self, entry_id: &str) -> Result<bool> {
        if lock(&self.inner.downloads).contains_key(entry_id) {
            return Ok(true);
        }
        Ok(self
            .inner
            .store
            .in_flight_sessions()
            .await?
            .iter()
            .any(|session| session.entry_id == entry_id))
    }

    async fn evict_rom(&self, mut rom: RommRom) -> Result<()> {
        let Some(cache) = rom.cache.take() else {
            return Ok(());
        };
        remove_dir(&cache.dir).await;
        tracing::info!(
            server = %rom.server_id,
            rom = rom.rom_id,
            bytes = cache.size,
            "RomM download evicted"
        );
        if rom.orphaned {
            return self.remove_from_library(&rom).await;
        }
        self.inner.store.save_romm_rom(&rom).await?;
        let Some(entry) = self.inner.store.get_entry(&rom.entry_id).await? else {
            return Ok(());
        };
        if self.offline(&rom.server_id) {
            let reason = self.unavailable(&rom.server_id).await;
            self.set_entry_state(entry, InstallState::Broken { reason })
                .await?;
        } else {
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: entry.game_id,
            });
        }
        Ok(())
    }

    /// Evicts least recently played games until `incoming` more bytes fit
    /// under the limit. Pinned, running and (by default) favourite games
    /// stay, as does `keep`; the limit can be exceeded when only those are
    /// left. So do games whose server isn't known to answer now (offline,
    /// not reached yet this session, or removed): they couldn't download
    /// again. A sync that reaches the server enforces the limit again.
    pub(super) async fn enforce_limit(&self, keep: Option<&str>, incoming: u64) -> Result<()> {
        let _evicting = self.inner.evicting.lock().await;
        let settings = self.settings().await?;
        let Some(limit) = settings.cache.limit_bytes() else {
            return Ok(());
        };
        let cached = self.inner.store.romm_cached().await?;
        let mut used: u64 = cached
            .iter()
            .filter_map(|rom| rom.cache.as_ref().map(|cache| cache.size))
            .sum();
        for rom in cached {
            if used.saturating_add(incoming) <= limit {
                return Ok(());
            }
            if rom.pinned
                || keep == Some(rom.entry_id.as_str())
                || !self.connected(&rom.server_id)
                || settings.server(&rom.server_id).is_none()
                || self.busy(&rom.entry_id).await?
            {
                continue;
            }
            if settings.cache.keep_favorites
                && self
                    .inner
                    .store
                    .game(&rom.game_id)
                    .await?
                    .is_some_and(|game| game.favorite)
            {
                continue;
            }
            let size = rom.cache.as_ref().map_or(0, |cache| cache.size);
            self.evict_rom(rom).await?;
            used = used.saturating_sub(size);
        }
        if used.saturating_add(incoming) > limit {
            tracing::warn!(
                used,
                incoming,
                limit,
                "RomM cache is over its limit: only kept games are left"
            );
        }
        Ok(())
    }

    /// Gets a RomM entry ready to launch: downloads it unless it is cached
    /// and points the entry at the files. `false` for other entries.
    pub async fn prepare_entry(&self, entry_id: &str) -> Result<bool> {
        let store = &self.inner.store;
        if store.romm_rom_by_entry(entry_id).await?.is_none() {
            return Ok(false);
        }
        let _guard = keyed(&self.inner.preparing, entry_id).lock_owned().await;
        let mut rom = store
            .romm_rom_by_entry(entry_id)
            .await?
            .ok_or_else(|| RommError::Config(crate::tr!("the game left the library")))?;
        let entry = store
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| RommError::Config(crate::tr!("the game left the library")))?;
        let settings = self.settings().await?;
        let config = settings.server(&rom.server_id).cloned();
        let ready = rom.cache.as_ref().is_some_and(|cache| {
            cache.dir.is_dir() && !cache.roms.is_empty() && cache.roms.iter().all(|p| p.is_file())
        });
        let downloaded = !ready;
        if !ready {
            // Files removed behind our back: forget the stale copy.
            if rom.cache.take().is_some() {
                store.save_romm_rom(&rom).await?;
            }
            let config = config.clone().ok_or_else(|| {
                RommError::Config(crate::tr!("this game's RomM server was removed"))
            })?;
            if rom.orphaned {
                return Err(RommError::Server(crate::tr!(
                    "this game was deleted on the RomM server"
                )));
            }
            let was_offline = self.offline(&rom.server_id);
            match self.download(&config, &settings, &rom, &entry).await {
                Ok(cache) => {
                    self.note(&rom.server_id, &Ok::<(), RommError>(()));
                    if was_offline {
                        self.mark_available(&rom.server_id).await?;
                    }
                    rom.cache = Some(cache);
                }
                Err(error) => {
                    self.note_error(&rom.server_id, &error);
                    return Err(match error {
                        RommError::Unreachable(_) => {
                            let message = self.unavailable(&rom.server_id).await;
                            self.mark_unavailable(&config).await?;
                            RommError::Config(message)
                        }
                        other => other,
                    });
                }
            }
        }
        let Some(cache) = rom.cache.as_mut() else {
            return Err(RommError::Config(crate::tr!("the download is missing")));
        };
        cache.last_used_ms = now_ms();
        let cache = cache.clone();

        // Point the entry at the files and the current platform mapping,
        // unless the game page chose another emulator.
        let mapped = config
            .as_ref()
            .map(|config| target_for(config.mapping(&rom.rom.platform_slug)))
            .unwrap_or_else(|| rom.synced_target.clone());
        let mut next = entry.clone();
        let (target, args) = match &entry.launch {
            Some(LaunchSpec::Emulator { target, args, .. }) => (target.clone(), args.clone()),
            _ => (rom.synced_target.clone(), Default::default()),
        };
        let target = if target == rom.synced_target {
            rom.synced_target = mapped.clone();
            mapped
        } else {
            target
        };
        next.launch = Some(LaunchSpec::Emulator {
            platform_id: rom.platform_id.clone(),
            target,
            roms: game_roms(&cache.roms, Some(cache.size)),
            args,
        });
        next.state = InstallState::Installed;
        next.location = Some(InstallLocation::External {
            path: cache.dir.clone(),
        });
        next.size = Some(cache.size);
        if next != entry {
            store.upsert_entry(&next).await?;
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: next.game_id.clone(),
            });
        }
        store.save_romm_rom(&rom).await?;
        if downloaded {
            self.enforce_limit(Some(entry_id), 0).await?;
        }
        Ok(true)
    }

    /// Downloads, verifies, unpacks and commits one ROM, publishing progress.
    async fn download(
        &self,
        config: &RommServerConfig,
        settings: &RommSettings,
        rom: &RommRom,
        entry: &crate::model::GameEntry,
    ) -> Result<CachedRom> {
        let title = self
            .inner
            .store
            .game(&entry.game_id)
            .await?
            .map_or_else(|| rom.rom.title(), |game| game.title);
        let cancel = CancellationToken::new();
        let mut progress = DownloadProgress {
            entry_id: entry.id.clone(),
            game_id: entry.game_id.clone(),
            title,
            server_id: rom.server_id.clone(),
            phase: DownloadPhase::Downloading,
            received: 0,
            total: (rom.rom.fs_size_bytes > 0).then_some(rom.rom.fs_size_bytes),
            error: None,
        };
        lock(&self.inner.downloads).insert(
            entry.id.clone(),
            super::Download {
                progress: progress.clone(),
                cancel: cancel.clone(),
            },
        );
        self.publish(progress.clone());
        let outcome = self
            .fetch(config, settings, rom, &cancel, &mut progress)
            .await;
        match &outcome {
            Ok(_) => progress.phase = DownloadPhase::Done,
            Err(RommError::Cancelled) => progress.phase = DownloadPhase::Cancelled,
            Err(error) => {
                progress.phase = DownloadPhase::Failed;
                progress.error = Some(error.to_string());
            }
        }
        self.publish(progress);
        lock(&self.inner.downloads).remove(&entry.id);
        outcome
    }

    #[cfg(not(feature = "net"))]
    async fn fetch(
        &self,
        _config: &RommServerConfig,
        _settings: &RommSettings,
        _rom: &RommRom,
        _cancel: &CancellationToken,
        _progress: &mut DownloadProgress,
    ) -> Result<CachedRom> {
        Err(super::offline_build())
    }

    #[cfg(feature = "net")]
    async fn fetch(
        &self,
        config: &RommServerConfig,
        settings: &RommSettings,
        rom: &RommRom,
        cancel: &CancellationToken,
        progress: &mut DownloadProgress,
    ) -> Result<CachedRom> {
        let root = self.cache_root_with(settings).await?;
        let staging = root
            .join(".staging")
            .join(format!("{}-{}", rom.server_id, rom.rom_id));
        let part = staging.join("download.part");
        let files = staging.join("files");
        let target = root.join(&rom.server_id).join(rom.rom_id.to_string());
        tokio::fs::create_dir_all(&staging).await?;
        let client = self.client(config, None).await?;
        {
            let title = progress.title.clone();
            let activity =
                caskade_jobs::global().activity(crate::tr!("Downloading {game}", game = title));
            // The status bar label moves in 5% steps, events in 1% steps
            // (or every 8 MiB without a known size).
            let mut labelled: Option<u64> = None;
            let mut published: Option<u64> = None;
            let mut reported = 0u64;
            let mut update = |received: u64, total: Option<u64>| {
                progress.received = received;
                if total.is_some() {
                    progress.total = total;
                }
                let percent = progress
                    .total
                    .filter(|total| *total > 0)
                    .map(|total| received.saturating_mul(100) / total);
                if let Some(now) = percent {
                    let step = |before: u64| now >= before + 5 || (now == 100 && before < 100);
                    if labelled.is_none_or(step) {
                        activity.set_label(crate::tr!(
                            "Downloading {game} · {percent}%",
                            game = title,
                            percent = now
                        ));
                        labelled = Some(now);
                    }
                    if published != Some(now) {
                        published = Some(now);
                        self.publish(progress.clone());
                    }
                } else if received.saturating_sub(reported) >= 8 << 20 {
                    reported = received;
                    self.publish(progress.clone());
                }
            };
            // Room for the download, once the server is sending it.
            let room = async || {
                // It answered, so its own games may make room too.
                self.note(&rom.server_id, &Ok::<(), RommError>(()));
                self.enforce_limit(Some(&rom.entry_id), rom.rom.fs_size_bytes)
                    .await
            };
            client
                .download(&rom.rom, &part, cancel, room, &mut update)
                .await?;
            drop(activity);
        }
        progress.phase = if rom.rom.is_multi() {
            DownloadPhase::Unpacking
        } else {
            DownloadPhase::Verifying
        };
        self.publish(progress.clone());
        let extensions = self
            .inner
            .emulation
            .catalog()
            .platform(&rom.platform_id)
            .map(|platform| platform.extensions.clone())
            .unwrap_or_default();
        let remote = rom.rom.clone();
        let check = settings.cache.verify;
        let (part_path, files_path) = (part.clone(), files.clone());
        let (roms, size) = caskade_jobs::blocking(crate::tr!("Unpacking ROM"), move || {
            assemble(&part_path, &files_path, &remote, &extensions, check)
        })
        .await
        .map_err(|error| RommError::Core(crate::Error::Io(std::io::Error::other(error))))??;
        // Commit: the finished folder replaces any stale copy in one rename.
        if tokio::fs::try_exists(&target).await? {
            tokio::fs::remove_dir_all(&target).await?;
        }
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(&files, &target).await?;
        remove_dir(&staging).await;
        let now = now_ms();
        tracing::info!(
            server = %rom.server_id,
            rom = rom.rom_id,
            bytes = size,
            "RomM ROM downloaded"
        );
        Ok(CachedRom {
            roms: roms.iter().map(|path| target.join(path)).collect(),
            dir: target,
            size,
            cached_at_ms: now,
            last_used_ms: now,
        })
    }
}

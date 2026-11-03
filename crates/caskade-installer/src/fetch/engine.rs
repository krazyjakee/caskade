use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use futures_util::{Stream, StreamExt};
use reqwest::{Client, StatusCode};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::{
    disk::DiskSpace,
    events::{EventSink, JobEvent, Phase},
    host::HostCaps,
};

use super::{
    DownloadRequest, DownloadedFile, FetchConfig, FetchError, Resolution, ResolverRegistry, Result,
    UrlResolver, http,
    names::response_name,
    storage::{self, State, Storage},
};

#[derive(Clone)]
pub struct FetchEngine {
    config: FetchConfig,
    client: Client,
    queue: Arc<Semaphore>,
    disk: Arc<dyn DiskSpace>,
    sink: Arc<dyn EventSink>,
    registry: ResolverRegistry,
}

impl FetchEngine {
    pub fn new(
        config: FetchConfig,
        disk: Arc<dyn DiskSpace>,
        sink: Arc<dyn EventSink>,
    ) -> Result<Self> {
        if config.concurrent_downloads == 0
            || config.connect_timeout.is_zero()
            || config.stall_timeout.is_zero()
            || config.max_bytes == 0
        {
            return Err(FetchError::InvalidRequest {
                message: "queue size, timeouts and maximum bytes must be nonzero".into(),
            });
        }
        // No total timeout: a large download takes as long as it takes. A
        // server that stops sending for `stall_timeout` fails the attempt.
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.stall_timeout)
            .use_rustls_tls()
            .build()
            .map_err(FetchError::http)?;
        Ok(Self {
            queue: Arc::new(Semaphore::new(config.concurrent_downloads)),
            config,
            client,
            disk,
            sink,
            registry: ResolverRegistry::with_builtins(super::ResolverConfig::default())?,
        })
    }

    /// Replace built-in endpoint/metadata settings, retaining explicitly
    /// registered resolvers (which always run before built-ins).
    pub fn with_resolver_config(mut self, config: super::ResolverConfig) -> Result<Self> {
        self.registry.configure_builtins(config)?;
        Ok(self)
    }

    pub fn register(&mut self, resolver: Arc<dyn UrlResolver>) -> Result<()> {
        self.registry.register(resolver)
    }
    pub async fn resolve(&self, url: &url::Url, host: &HostCaps) -> Result<Resolution> {
        self.registry.resolve(url, host).await
    }

    /// Call only after installation succeeds. Failed or cancelled installs keep
    /// their downloads available for retry; local inputs never pass here.
    pub async fn after_install(&self, root: &Path, job_id: &str) -> Result<()> {
        super::safe_file_name(job_id)?;
        if self.config.keep_downloads {
            return Ok(());
        }
        let root = std::fs::canonicalize(root)?;
        let _guard = job_lock(&root.join(".caskade/downloads").join(job_id))
            .lock_owned()
            .await;
        let capability = crate::safety::open_staging(&root)?;
        let path = Path::new(".caskade/downloads").join(job_id);
        if capability
            .symlink_metadata(&path)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            return Ok(());
        }
        // A torrent's data lives by its info hash, outside the job folder.
        let torrent = storage::torrent_marker(&storage::directory(&capability, &path, false)?);
        if let Some(hash) = torrent {
            let _torrent = job_lock(&root.join(storage::TORRENTS).join(&hash))
                .lock_owned()
                .await;
            storage::remove_torrent(&capability, &hash)?;
        }
        capability.remove_dir_all(path)?;
        storage::prune(&root, job_id);
        Ok(())
    }

    /// Active job IDs come from the persisted job queue. Completed or invalid
    /// orphan files are removed; only validator-backed nonempty partials survive.
    pub async fn cleanup_orphans(&self, root: &Path, active_jobs: &[String]) -> Result<()> {
        let root = std::fs::canonicalize(root)?;
        let capability = crate::safety::open_staging(&root)?;
        if !capability.try_exists(".caskade/downloads")? {
            return Ok(());
        }
        let downloads = storage::directory(&capability, Path::new(".caskade/downloads"), false)?;
        for entry in downloads.entries()? {
            let entry = entry?;
            let job_id =
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| FetchError::InvalidRequest {
                        message: "invalid orphan job name".into(),
                    })?;
            super::safe_file_name(&job_id)?;
            if active_jobs.contains(&job_id) {
                continue;
            }
            let _guard = job_lock(&root.join(".caskade/downloads").join(&job_id))
                .lock_owned()
                .await;
            let storage = Storage::open(&root, &job_id)?;
            let mut keep = std::collections::BTreeSet::new();
            for state_file in storage.states.entries()? {
                let state_file = state_file?;
                let name = state_file.file_name();
                let Some(key) = Path::new(&name)
                    .file_stem()
                    .and_then(|value| value.to_str())
                else {
                    continue;
                };
                let state = if Path::new(&name)
                    .extension()
                    .is_some_and(|extension| extension == "json")
                {
                    storage.read(key).ok().flatten()
                } else {
                    None
                };
                let preserve = state.as_ref().is_some_and(|state| {
                    let partial = storage.part_len(state).ok().filter(|size| *size > 0);
                    let resumable = !state.complete
                        && state.validator().is_some()
                        && partial
                            .is_some_and(|size| state.total.is_none_or(|total| size <= total));
                    let completed = self.config.keep_downloads
                        && state.complete
                        && storage::regular_len(&storage.job, &state.file_name)
                            .ok()
                            .flatten()
                            .is_some();
                    resumable || completed
                });
                if preserve {
                    let state = state.expect("preserved state exists");
                    keep.insert(if state.complete {
                        state.file_name
                    } else {
                        format!("{}.part", state.file_name)
                    });
                } else {
                    storage::remove_file(&storage.states, &name.to_string_lossy())?;
                }
            }
            for file in storage.job.entries()? {
                let file = file?;
                let name = file.file_name();
                if name == ".state" || keep.contains(name.to_string_lossy().as_ref()) {
                    continue;
                }
                if file.file_type()?.is_dir() && !file.file_type()?.is_symlink() {
                    storage.job.remove_dir_all(name)?;
                } else {
                    storage.job.remove_file(name)?;
                }
            }
            storage::prune(&root, &job_id);
        }
        Ok(())
    }

    #[tracing::instrument(skip_all, fields(job_id = job_id))]
    pub async fn download(
        &self,
        root: &Path,
        job_id: &str,
        request: &DownloadRequest,
        cancellation: CancellationToken,
    ) -> Result<DownloadedFile> {
        super::safe_file_name(job_id)?;
        if !matches!(request.url.scheme(), "http" | "https") {
            return Err(FetchError::UnsupportedScheme);
        }
        let expected = expected_checksum(request)?;
        if request
            .size
            .is_some_and(|size| size > self.config.max_bytes)
        {
            return Err(FetchError::TooLarge {
                limit: self.config.max_bytes,
            });
        }
        if let Some(name) = &request.file_name {
            super::safe_file_name(name)?;
        }
        let root = std::fs::canonicalize(root)?;
        let token = cancellation.child_token();
        let _cancel_on_drop = token.clone().drop_guard();
        let _permit = tokio::select! {
            _ = token.cancelled() => return Err(FetchError::Cancelled),
            permit = self.queue.acquire() => permit.map_err(|_| FetchError::Cancelled)?,
        };
        let lock = job_lock(&root.join(".caskade/downloads").join(job_id));
        let _guard = tokio::select! {
            _ = token.cancelled() => return Err(FetchError::Cancelled),
            guard = lock.lock_owned() => guard,
        };
        http::cancelled(&token)?;
        let storage = Storage::open(&root, job_id)?;
        let key = request_key(request);
        let started = Instant::now();
        let context = Context {
            engine: self,
            root: &root,
            job_id,
            token: &token,
            started,
            reached: AtomicU64::new(0),
        };
        let mut failures = 0;
        loop {
            let reached = context.reached.load(Ordering::Relaxed);
            let result = self
                .attempt(&storage, &key, request, expected.as_deref(), &context)
                .await;
            match result {
                Ok(file) => return Ok(file),
                Err(error) if error.retryable() => {
                    // A connection that keeps dropping while the file grows
                    // is slow, not failing: only attempts in a row that got
                    // no further use up the retries.
                    if context.reached.load(Ordering::Relaxed) > reached {
                        failures = 0;
                    }
                    if failures >= self.config.retries {
                        return Err(error);
                    }
                    context.emit("retrying", 0, None)?;
                    let delay = self
                        .config
                        .initial_backoff
                        .saturating_mul(2_u32.saturating_pow(failures))
                        .min(self.config.max_backoff);
                    failures += 1;
                    tokio::select! {
                        _ = token.cancelled() => return Err(FetchError::Cancelled),
                        _ = tokio::time::sleep(delay) => (),
                    }
                }
                Err(error) => {
                    // A web page (a login or captive portal) is never written,
                    // so an earlier partial stays resumable.
                    if matches!(
                        error,
                        FetchError::ChecksumMismatch | FetchError::TooLarge { .. }
                    ) {
                        storage.purge(&key)?;
                    }
                    return Err(error);
                }
            }
        }
    }

    async fn attempt(
        &self,
        storage: &Storage,
        key: &str,
        request: &DownloadRequest,
        expected: Option<&str>,
        context: &Context<'_>,
    ) -> Result<DownloadedFile> {
        // A plugin's download resolves names within its grant (CSK-03).
        let guarded = request
            .allowed_hosts
            .as_deref()
            .map(|grant| super::grant::client(&self.config, grant))
            .transpose()?;
        let client = guarded.as_ref().unwrap_or(&self.client);
        let mut previous = storage.read(key)?;
        if let Some(state) = &previous
            && storage::regular_len(&storage.job, &state.file_name)?.is_some()
            && storage.part_len(state)? == 0
        {
            return self
                .finish(storage, state.clone(), request, expected, context, true)
                .await;
        }
        for _restart in 0..2 {
            let offset = match &previous {
                Some(state) if state.validator().is_some() => storage.part_len(state)?,
                _ => 0,
            };
            let resume = previous
                .as_ref()
                .and_then(|state| state.validator())
                .filter(|_| offset > 0)
                .map(|validator| (offset, validator));
            let response = http::get(
                client,
                &request.url,
                &request.headers,
                resume,
                context.token,
                request.allowed_hosts.as_deref(),
            )
            .await?;
            if http::is_html(response.headers()) {
                return Err(FetchError::Html);
            }
            if response.status() == StatusCode::RANGE_NOT_SATISFIABLE && offset > 0 {
                let state = previous.as_ref().expect("resume has state");
                let total = http::header(response.headers(), "content-range").and_then(|value| {
                    value
                        .strip_prefix("bytes */")
                        .and_then(|value| value.parse::<u64>().ok())
                });
                if total == Some(offset) && validators_match(state, &response) {
                    return self
                        .finish(storage, state.clone(), request, expected, context, false)
                        .await;
                }
                reset(storage, &mut previous)?;
                continue;
            }
            if !response.status().is_success() {
                return Err(FetchError::Status {
                    status: response.status().as_u16(),
                });
            }
            let mut append = false;
            let mut body_length = response.content_length();
            let total = if response.status() == StatusCode::PARTIAL_CONTENT {
                if offset == 0 {
                    return Err(FetchError::InvalidResponse {
                        message: "unsolicited partial response".into(),
                    });
                }
                let state = previous.as_ref().expect("resume has state");
                if !validators_match(state, &response) || state.final_url != *response.url() {
                    reset(storage, &mut previous)?;
                    continue;
                }
                let range = http::header(response.headers(), "content-range").ok_or_else(|| {
                    FetchError::InvalidResponse {
                        message: "partial response has no Content-Range".into(),
                    }
                })?;
                let (start, end, total) = http::range(&range)?;
                let length = end
                    .checked_sub(start)
                    .and_then(|value| value.checked_add(1))
                    .ok_or_else(|| FetchError::InvalidResponse {
                        message: "Content-Range overflow".into(),
                    })?;
                if start != offset || body_length.is_some_and(|actual| actual != length) {
                    return Err(FetchError::InvalidResponse {
                        message:
                            "partial response starts at the wrong offset or has the wrong length"
                                .into(),
                    });
                }
                body_length = Some(length);
                append = true;
                total
            } else if response.status() == StatusCode::OK {
                body_length
            } else {
                return Err(FetchError::InvalidResponse {
                    message: "file response must be HTTP 200 or 206".into(),
                });
            };
            if total.is_some_and(|total| total > self.config.max_bytes) {
                return Err(FetchError::TooLarge {
                    limit: self.config.max_bytes,
                });
            }
            if request
                .size
                .is_some_and(|expected| total.is_some_and(|actual| actual != expected))
            {
                return Err(FetchError::InvalidResponse {
                    message: "download size differs from resolver hint".into(),
                });
            }
            let name = response_name(
                response.headers(),
                response.url(),
                request.file_name.as_deref(),
            )?;
            let final_url = response.url().clone();
            let etag = http::header(response.headers(), "etag");
            let last_modified = http::header(response.headers(), "last-modified");
            let date = http::header(response.headers(), "date");
            let mut stream = response
                .bytes_stream()
                .map(|chunk| chunk.map_err(FetchError::http));
            // Sniff a new body before anything is written: a web page must
            // leave an earlier partial and its state as they were. A body
            // that breaks off early keeps what arrived, for the next attempt.
            let mut head = vec![];
            let mut broken = None;
            if !append {
                let mut sniff = vec![];
                while sniff.len() < 512 {
                    let chunk = match next_chunk(&mut stream, context.token).await {
                        Ok(Some(chunk)) => chunk,
                        Ok(None) => break,
                        Err(FetchError::Cancelled) => return Err(FetchError::Cancelled),
                        Err(error) => {
                            broken = Some(error);
                            break;
                        }
                    };
                    sniff.extend_from_slice(&chunk[..chunk.len().min(512 - sniff.len())]);
                    head.push(chunk);
                    if http::html_bytes(&sniff) {
                        return Err(FetchError::Html);
                    }
                }
            }
            let mut body =
                futures_util::stream::iter(head.into_iter().map(Ok).chain(broken.map(Err)))
                    .chain(stream);
            let mut state = State {
                version: 1,
                request_key: key.into(),
                file_name: match &previous {
                    Some(state) => state.file_name.clone(),
                    None => storage.choose_name(&name)?,
                },
                final_url,
                etag,
                last_modified,
                date,
                total,
                complete: false,
            };
            let mut bytes = if append { offset } else { 0 };
            context.check_space(total.unwrap_or(0).saturating_sub(bytes))?;
            storage.write(&state)?;
            let mut file = storage.open_part(&state, append)?;
            context.emit(if append { "resumed" } else { "started" }, bytes, total)?;
            while let Some(chunk) = next_chunk(&mut body, context.token).await? {
                // Bounded synchronous writes have no detached I/O after a future
                // drop. The file and per-job lock cannot outlive each other.
                for chunk in chunk.chunks(64 * 1024) {
                    let next =
                        bytes
                            .checked_add(chunk.len() as u64)
                            .ok_or(FetchError::TooLarge {
                                limit: self.config.max_bytes,
                            })?;
                    if next > self.config.max_bytes {
                        return Err(FetchError::TooLarge {
                            limit: self.config.max_bytes,
                        });
                    }
                    context.check_space(chunk.len() as u64)?;
                    file.write_all(chunk)?;
                    bytes = next;
                    context.reached.fetch_max(bytes, Ordering::Relaxed);
                    context.emit("progress", bytes, total)?;
                    tokio::task::yield_now().await;
                }
            }
            file.sync_all()?;
            drop(file);
            let body_received = bytes.saturating_sub(if append { offset } else { 0 });
            if body_length.is_some_and(|expected| expected != body_received)
                || total.is_some_and(|expected| expected != bytes)
                || request.size.is_some_and(|expected| expected != bytes)
            {
                return Err(FetchError::Http {
                    message: "download body ended before its expected length".into(),
                });
            }
            state.total = Some(bytes);
            storage.write(&state)?;
            return self
                .finish(storage, state, request, expected, context, false)
                .await;
        }
        Err(FetchError::InvalidResponse {
            message: "server did not restart a changed representation".into(),
        })
    }

    async fn finish(
        &self,
        storage: &Storage,
        mut state: State,
        request: &DownloadRequest,
        expected: Option<&str>,
        context: &Context<'_>,
        already_renamed: bool,
    ) -> Result<DownloadedFile> {
        http::cancelled(context.token)?;
        let name = if already_renamed {
            state.file_name.clone()
        } else {
            format!("{}.part", state.file_name)
        };
        let file = storage.job.open(&name)?;
        let size = file.metadata()?.len();
        if size > self.config.max_bytes {
            return Err(FetchError::TooLarge {
                limit: self.config.max_bytes,
            });
        }
        if state.total.is_some_and(|total| total != size)
            || request.size.is_some_and(|expected| expected != size)
        {
            return Err(FetchError::InvalidResponse {
                message: "completed file length is invalid".into(),
            });
        }
        context.emit("verifying", size, Some(size))?;
        let token = context.token.clone();
        let digest = caskade_jobs::blocking("Verifying download", move || {
            let mut file = file;
            let mut hash = Sha256::new();
            let mut buffer = [0; 64 * 1024];
            loop {
                http::cancelled(&token)?;
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            Ok::<_, FetchError>(format!("{:x}", hash.finalize()))
        })
        .await
        .map_err(|_| FetchError::InvalidResponse {
            message: "checksum worker failed".into(),
        })??;
        if expected.is_some_and(|expected| expected != digest) {
            return Err(FetchError::ChecksumMismatch);
        }
        http::cancelled(context.token)?;
        if !already_renamed {
            storage.job.rename(name, &storage.job, &state.file_name)?;
        }
        state.complete = true;
        storage.write(&state)?;
        context.emit("completed", size, Some(size))?;
        let validator = state.validator().map(str::to_owned);
        Ok(DownloadedFile {
            path: context
                .root
                .join(".caskade/downloads")
                .join(context.job_id)
                .join(state.file_name),
            source_url: request.url.clone(),
            insecure_transport: request.url.scheme() == "http"
                || state.final_url.scheme() == "http",
            validator,
            final_url: state.final_url,
            size,
            sha256: digest,
        })
    }
}

struct Context<'a> {
    engine: &'a FetchEngine,
    root: &'a Path,
    job_id: &'a str,
    token: &'a CancellationToken,
    started: Instant,
    /// The furthest any attempt has written into the file.
    reached: AtomicU64,
}
impl Context<'_> {
    fn check_space(&self, bytes: u64) -> Result<()> {
        http::cancelled(self.token)?;
        let available = self.engine.disk.available_bytes(self.root)?;
        let required = bytes
            .checked_add(self.engine.config.disk_margin_bytes)
            .ok_or(FetchError::DiskFull {
                available,
                required: u64::MAX,
            })?;
        if available < required {
            return Err(FetchError::DiskFull {
                available,
                required,
            });
        }
        Ok(())
    }
    fn emit(&self, message: &str, bytes: u64, total: Option<u64>) -> Result<()> {
        http::cancelled(self.token)?;
        let elapsed = self.started.elapsed().as_secs();
        let eta = total
            .filter(|_| bytes > 0 && elapsed > 0)
            .map(|total| total.saturating_sub(bytes).saturating_mul(elapsed) / bytes);
        self.engine.sink.emit(JobEvent {
            job_id: self.job_id.into(),
            phase: Phase::Download,
            step: None,
            bytes,
            total_bytes: total,
            files: u64::from(message == "completed"),
            eta_secs: eta,
            message: message.into(),
        });
        http::cancelled(self.token)
    }
}

async fn next_chunk<S, B>(stream: &mut S, token: &CancellationToken) -> Result<Option<B>>
where
    S: Stream<Item = Result<B>> + Unpin,
{
    tokio::select! {
        _ = token.cancelled() => Err(FetchError::Cancelled),
        chunk = stream.next() => chunk.transpose(),
    }
}

fn validators_match(state: &State, response: &reqwest::Response) -> bool {
    if let Some(etag) = &state.etag {
        http::header(response.headers(), "etag").as_ref() == Some(etag)
    } else {
        state.last_modified.is_some()
            && http::header(response.headers(), "last-modified") == state.last_modified
    }
}
fn reset(storage: &Storage, previous: &mut Option<State>) -> Result<()> {
    if let Some(state) = previous.as_mut() {
        storage.open_part(state, false)?;
        state.etag = None;
        state.last_modified = None;
        state.date = None;
        state.complete = false;
        storage.write(state)?;
    }
    Ok(())
}

pub(super) fn expected_checksum(request: &DownloadRequest) -> Result<Option<String>> {
    let fragment = request
        .url
        .fragment()
        .and_then(|fragment| fragment.strip_prefix("sha256="));
    let valid =
        |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    for value in request.sha256.as_deref().into_iter().chain(fragment) {
        if !valid(value) {
            return Err(FetchError::InvalidRequest {
                message: "SHA-256 must have 64 hexadecimal digits".into(),
            });
        }
    }
    if let (Some(hint), Some(fragment)) = (request.sha256.as_deref(), fragment)
        && !hint.eq_ignore_ascii_case(fragment)
    {
        return Err(FetchError::InvalidRequest {
            message: "conflicting SHA-256 hints".into(),
        });
    }
    Ok(request
        .sha256
        .as_deref()
        .or(fragment)
        .map(str::to_ascii_lowercase))
}
fn request_key(request: &DownloadRequest) -> String {
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(request).expect("download request serialization is infallible"));
    format!("{:x}", hash.finalize())
}
type JobLocks = Mutex<BTreeMap<PathBuf, Weak<AsyncMutex<()>>>>;
pub(super) fn job_lock(path: &Path) -> Arc<AsyncMutex<()>> {
    static LOCKS: OnceLock<JobLocks> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .expect("download lock registry poisoned");
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(AsyncMutex::new(()));
    locks.insert(path.into(), Arc::downgrade(&lock));
    lock
}

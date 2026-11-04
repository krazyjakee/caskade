use super::*;
use crate::model::InstallLocation;
use caskade_installer::{
    events::EventSink,
    exec::with_roots_exclusive,
    manifest::{InstallManifest, RelocationVerdict, RootManifest},
    pipeline::relocate::{engine, engine_for_move},
    reconcile::Reconciler,
    relocate::RelocateRequest,
    runtime::alias::AliasStore,
};
use std::{path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema_version: u32,
    job_id: String,
    entry_id: String,
    game_id: String,
    source: StorageRoot,
    target: StorageRoot,
    relative: PathBuf,
    references: Vec<caskade_installer::scan::Reference>,
    alias: Option<String>,
}
struct StartupSink;
impl EventSink for StartupSink {
    fn emit(&self, event: caskade_installer::events::JobEvent) {
        tracing::debug!(job_id=%event.job_id,phase=%event.message,"storage maintenance");
    }
}
impl StorageService {
    /// Shared native/prefix backend factory for managed-root reconciliation.
    pub fn reconciler(&self) -> Result<Arc<Reconciler>> {
        let engine = caskade_installer::pipeline::relocate::reconciler(
            caskade_installer::host::HostCaps::detect(),
            Arc::new(StartupSink),
            Some(self.aliases()),
        )
        .map_err(|e| StorageError::Invalid(e.to_string()))?;
        let roots = caskade_installer::runtime::default_watch_roots()
            .into_values()
            .filter(|path| path.is_dir())
            .collect();
        Ok(Arc::new(engine.with_external_roots(roots)))
    }
    /// Startup convenience: restore configured roots before managed-source scans.
    pub async fn initialize(&self) -> Result<Vec<String>> {
        self.restore_roots().await?;
        if let Err(error) = self.ensure_default_root().await {
            tracing::warn!(%error, "no default storage location");
        }
        self.recover_pending(Arc::new(StartupSink), CancellationToken::new())
            .await
    }
    /// What moving every game off `id` to `target_id` takes (#64). Refused
    /// here, with nothing moved, when the plan can't finish as far as can be
    /// told: the location can't be removed, a game can't go, or they don't
    /// fit.
    pub async fn plan_location_move(&self, id: &str, target_id: &str) -> Result<LocationMove> {
        if id == target_id {
            return Err(StorageError::Invalid(crate::tr!(
                "choose a different storage location"
            )));
        }
        let source = self
            .store
            .get_root(id)
            .await?
            .ok_or_else(|| StorageError::Invalid(crate::tr!("unknown storage location")))?;
        if source.is_default {
            return Err(StorageError::DefaultRoot);
        }
        let summary = self.summary(source).await?;
        let target = self.resolve(Some(target_id)).await?;
        let playing = self.store.in_flight_sessions().await?;
        for game in &summary.games {
            if let Some(reason) = &game.move_blocked {
                return Err(StorageError::Invalid(crate::tr!(
                    "“{title}” can't move: {reason}",
                    title = game.title,
                    reason = reason
                )));
            }
            if self.launch.try_game_lease(&game.game_id).is_err()
                || playing.iter().any(|s| s.game_id == game.game_id)
            {
                return Err(StorageError::Invalid(crate::tr!(
                    "“{title}” is running. Close it, then try again.",
                    title = game.title
                )));
            }
        }
        let caps = self.capabilities(&target.id).await?;
        // Within one volume a move takes no new space.
        let same_volume = caps.volume.is_some() && caps.volume == summary.capabilities.volume;
        if !same_volume
            && caps
                .free_bytes
                .is_some_and(|free| free < summary.games_bytes)
        {
            return Err(StorageError::Invalid(crate::tr!(
                "not enough free space in the target location"
            )));
        }
        Ok(LocationMove {
            source: summary.root,
            target,
            games: summary.games,
        })
    }
    /// The job queue supplies its event sink/cancellation token. The shared launch
    /// lease spans root leases, durable relocation and the library update.
    pub async fn move_entry(
        &self,
        entry_id: &str,
        target_root_id: &str,
        job_id: &str,
        sink: Arc<dyn EventSink>,
        cancel: CancellationToken,
    ) -> Result<MoveReport> {
        if job_id.is_empty()
            || job_id.len() > 128
            || !job_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(StorageError::Invalid(crate::tr!("invalid move job id")));
        }
        let entry = self
            .store
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| StorageError::Invalid(crate::tr!("unknown game entry")))?;
        let _lease = self
            .launch
            .try_game_lease(&entry.game_id)
            .map_err(|_| StorageError::Running)?;
        if self
            .store
            .in_flight_sessions()
            .await?
            .iter()
            .any(|s| s.game_id == entry.game_id)
        {
            return Err(StorageError::Running);
        }
        let Some(InstallLocation::Managed { root_id, rel_dir }) = &entry.location else {
            return Err(StorageError::Invalid(crate::tr!(
                "only Caskade-managed installations can move"
            )));
        };
        if root_id == target_root_id {
            return Err(StorageError::Invalid(crate::tr!(
                "choose a different storage location"
            )));
        }
        let source = self.resolve(Some(root_id)).await?;
        let target = self.resolve(Some(target_root_id)).await?;
        let source_dir = managed_dir(&source, rel_dir)?;
        let manifest = InstallManifest::read(&source_dir)?;
        if manifest.record().plan.root_id != source.id
            || manifest.record().plan.relative_dir != *rel_dir
        {
            return Err(StorageError::Invalid(crate::tr!(
                "manifest identity does not match the library"
            )));
        }
        if manifest.record().relocation.verdict == RelocationVerdict::Pinned
            || manifest.record().plan.candidate.strategy == caskade_installer::Strategy::Delegate
        {
            return Err(StorageError::Invalid(crate::tr!(
                "native package-manager installations cannot move"
            )));
        }
        let target_caps = self.capabilities(&target.id).await?;
        if !target_caps.executable {
            return Err(StorageError::Invalid(crate::tr!(
                "the target location cannot execute games"
            )));
        }
        if !target_caps.symlinks && source_dir.join(".caskade/prefix").exists() {
            return Err(StorageError::Invalid(crate::tr!(
                "the target cannot hold this game's prefix links; choose a location with symlink support"
            )));
        }
        if target_caps.free_bytes.is_some_and(|free| {
            free < entry
                .size
                .unwrap_or_else(|| manifest.record().files.iter().map(|f| f.size).sum())
        }) {
            return Err(StorageError::Invalid(crate::tr!(
                "not enough free space in the target location"
            )));
        }
        // The engine keeps the basename. Nested legacy roots are accepted only when
        // the destination relative path is explicit and no directory is overwritten.
        let target_relative = PathBuf::from(
            source_dir
                .file_name()
                .ok_or_else(|| StorageError::Invalid(crate::tr!("game folder has no name")))?,
        );
        let mut intent = Intent {
            schema_version: 1,
            job_id: job_id.into(),
            entry_id: entry.id.clone(),
            game_id: entry.game_id.clone(),
            source: source.clone(),
            target: target.clone(),
            relative: rel_dir.clone(),
            references: manifest.record().relocation.references.clone(),
            alias: manifest.record().alias.clone(),
        };
        let host = caskade_installer::host::HostCaps::detect();
        let sink_for_inspect = sink.clone();
        let inspect_dir = source_dir.clone();
        let inspect_aliases = self.aliases();
        let prepared =
            caskade_jobs::blocking(crate::tr!("Preparing game move"), move || -> Result<_> {
                let inspector = Reconciler::new(
                    engine(Some((&inspect_dir, &manifest)), &host, sink_for_inspect)
                        .map_err(|e| StorageError::Invalid(e.to_string()))?,
                )
                .with_aliases(inspect_aliases)
                .with_external_roots(
                    caskade_installer::runtime::default_watch_roots()
                        .into_values()
                        .filter(|path| path.is_dir())
                        .collect(),
                );
                inspector
                    .prepare_references(&inspect_dir, &inspect_dir.to_string_lossy(), &manifest)
                    .map_err(|e| StorageError::Invalid(e.to_string()))
            })
            .await
            .map_err(|e| StorageError::Worker(e.to_string()))??;
        intent.references = prepared.references.clone();
        let key = format!("storage.move.{job_id}");
        if self.store.get_setting(&key).await?.is_some() {
            return Err(StorageError::Invalid(crate::tr!(
                "recover the previous move before retrying"
            )));
        }
        self.store
            .set_setting(&key, &serde_json::to_value(&intent)?)
            .await?;
        let request = intent.request()?;
        let worker_request = request;
        let host = caskade_installer::host::HostCaps::detect();
        let aliases = self.aliases();
        let db_store = self.store.clone();
        let entry_key = entry.id.clone();
        let journal_key = key.clone();
        let source_root_path = source.path.clone();
        let target_root_path = target.path.clone();
        let result = with_roots_exclusive(
            &[source_root_path.as_path(), target_root_path.as_path()],
            cancel,
            move |_, token| -> Result<_> {
                let lease = _lease;
                check_identity(&source)?;
                check_identity(&target)?;
                let current = InstallManifest::read(&worker_request.game_dir)?;
                if current
                    .record()
                    .relocation
                    .last_path
                    .as_ref()
                    .is_some_and(|old| {
                        std::fs::canonicalize(old).ok().as_deref()
                            != Some(worker_request.game_dir.as_path())
                    })
                {
                    return Err(StorageError::Invalid(crate::tr!("installation moved outside Caskade; reconcile it first")));
                }
                let final_dir = target.path.join(&target_relative);
                let mut request = worker_request;
                request.alias = request.alias.map(|(_, id)| (aliases, id));
                if let Some((store,id))=&request.alias
                    && let Some(target)=store.target(id).map_err(std::io::Error::other)?
                    && target.try_exists()?
                    && std::fs::canonicalize(target)?!=request.game_dir
                { return Err(StorageError::Invalid(crate::tr!("the alias belongs to another existing installation; reconcile the copies first"))); }
                let engine = engine_for_move(&request.game_dir, &final_dir, &current, &host, sink)
                    .map_err(|e| StorageError::Invalid(e.to_string()))?;
                let report = engine
                    .relocate_with_verdict(&request, &token, Some(prepared.verdict))
                    .map_err(|e| StorageError::Invalid(e.to_string()))?;
                let relative = report
                    .game_dir
                    .strip_prefix(&target.path)
                    .map_err(|_| StorageError::Offline)?
                    .to_path_buf();
                // Keep both filesystem leases through the DB commit. A dropped
                // async caller cannot release the game's guard mid-operation.
                tokio::runtime::Handle::current().block_on(async {
                    db_store
                        .update_managed_location(&entry_key, &target.id, &relative)
                        .await?;
                    db_store.delete_setting(&journal_key).await?;
                    Ok::<_, StorageError>(())
                })?;
                Ok((report, lease))
            },
        )
        .await;
        match result {
            Ok((report, _lease)) => Ok(MoveReport {
                entry_id: entry.id,
                game_id: entry.game_id,
                root_id: intent.target.id,
                directory: report.game_dir,
                copied: report.copied,
                warnings: prepared.skipped.iter().map(|w| format!("{w:?}")).collect(),
            }),
            Err(error) => {
                if source_dir.is_dir()
                    && !intent
                        .target
                        .path
                        .join(source_dir.file_name().unwrap())
                        .exists()
                {
                    self.store.delete_setting(&key).await?;
                }
                Err(error)
            }
        }
    }
    fn aliases(&self) -> AliasStore {
        AliasStore::new(self.paths.data.join("g"))
    }
    /// Run on startup before #54 imports manifests. The durable job catalogue is
    /// authoritative; imported directories cannot nominate arbitrary undo paths.
    pub async fn recover_pending(
        &self,
        sink: Arc<dyn EventSink>,
        cancel: CancellationToken,
    ) -> Result<Vec<String>> {
        let mut recovered = vec![];
        // One intent that can't be recovered now (its root is offline, its
        // game is running, its record is damaged) keeps its record for the
        // next start and doesn't hold up the others.
        for (key, value) in self.store.settings_with_prefix("storage.move.").await? {
            if cancel.is_cancelled() {
                break;
            }
            match self
                .recover_intent(&key, value, sink.clone(), cancel.clone())
                .await
            {
                Ok(job_id) => recovered.push(job_id),
                Err(error) => tracing::warn!(%error, key, "move recovery deferred"),
            }
        }
        Ok(recovered)
    }
    async fn recover_intent(
        &self,
        key: &str,
        value: serde_json::Value,
        sink: Arc<dyn EventSink>,
        cancel: CancellationToken,
    ) -> Result<String> {
        let intent: Intent = serde_json::from_value(value)?;
        if intent.schema_version != 1 || key != format!("storage.move.{}", intent.job_id) {
            return Err(StorageError::Invalid(crate::tr!("invalid move intent")));
        }
        let _lease = self
            .launch
            .try_game_lease(&intent.game_id)
            .map_err(|_| StorageError::Running)?;
        check_identity(&intent.source)?;
        check_identity(&intent.target)?;
        let request = intent.request()?;
        let source = request.game_dir.clone();
        let destination = intent.target.path.join(
            source
                .file_name()
                .ok_or_else(|| StorageError::Invalid(crate::tr!("game has no folder name")))?,
        );
        let inspection = if source.is_dir() {
            source.clone()
        } else {
            destination.clone()
        };
        let manifest = InstallManifest::read(&inspection)?;
        let host = caskade_installer::host::HostCaps::detect();
        let job_sink = sink.clone();
        let mut request = request;
        request.alias = request.alias.map(|(_, id)| (self.aliases(), id));
        let target_for_engine = destination.clone();
        let (_, _lease) = with_roots_exclusive(
            &[intent.source.path.as_path(), intent.target.path.as_path()],
            cancel.clone(),
            move |_, _| -> Result<_> {
                let worker =
                    engine_for_move(&inspection, &target_for_engine, &manifest, &host, job_sink)
                        .map_err(|e| StorageError::Invalid(e.to_string()))?;
                worker
                    .recover_move(&request)
                    .map(|recovered| (recovered, _lease))
                    .map_err(|e| StorageError::Invalid(e.to_string()))
            },
        )
        .await?;
        if source.is_dir() && !destination.exists() {
            self.store
                .update_managed_location(&intent.entry_id, &intent.source.id, &intent.relative)
                .await?;
        } else if !source.exists() && destination.is_dir() {
            let relative = destination
                .strip_prefix(&intent.target.path)
                .map_err(|_| StorageError::Offline)?;
            self.store
                .update_managed_location(&intent.entry_id, &intent.target.id, relative)
                .await?;
        } else {
            return Err(StorageError::Invalid(crate::tr!(
                "move recovery found ambiguous copies; no files were discarded"
            )));
        }
        self.store.delete_setting(key).await?;
        Ok(intent.job_id)
    }
}
impl Intent {
    fn request(&self) -> Result<RelocateRequest> {
        let dir = self.source.path.join(&self.relative);
        if self.relative.is_absolute()
            || self
                .relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(StorageError::Invalid(crate::tr!(
                "invalid managed relative folder"
            )));
        }
        Ok(RelocateRequest {
            job_id: self.job_id.clone(),
            game_dir: dir,
            target_root: self.target.path.clone(),
            references: self.references.clone(),
            spellings: vec![],
            alias: self
                .alias
                .clone()
                .map(|id| (AliasStore::new(PathBuf::new()), id)),
            force_copy: false,
        })
    }
}
fn check_identity(root: &StorageRoot) -> Result<()> {
    if RootManifest::read(&root.path)?.root_id() != root.id {
        return Err(StorageError::Offline);
    }
    Ok(())
}
fn managed_dir(root: &StorageRoot, relative: &Path) -> Result<PathBuf> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(StorageError::Invalid(crate::tr!(
            "invalid managed relative path"
        )));
    }
    let path = std::fs::canonicalize(root.path.join(relative))?;
    if path == root.path || !path.starts_with(&root.path) {
        return Err(StorageError::Invalid(crate::tr!(
            "managed folder escaped its storage location"
        )));
    }
    Ok(path)
}

//! Provider adapters (#126): a plugin's provider
//! exports implement the same core traits as the built-in providers, so
//! both use one code path. Each call goes through the host like any other:
//! lazily activated, time-boxed, and contained if it fails. Providers also
//! need the matching capability: `library: write` for a library source,
//! `network` for metadata and URL resolvers, `process` for a runner.
//!
//! Install handlers (#55) need no capability: they read their input
//! through `host.install-input-read` during the call and answer with plan
//! steps that run no program; the installer does the writing.

use crate::{
    HostError, PluginHost,
    capabilities::host_allowed,
    manifest::LibraryAccess,
    runtime::{CallError, GuestError, Instance},
};
use async_trait::async_trait;
use caskade_core::{
    bus::{
        Bus, DownloadRequest, InstallHandler, InstallInput, LibrarySource, PLUGIN_SOURCE_PREFIX,
        UrlResolver,
    },
    launch::{LaunchCommand, PluginRunner},
    model::Game,
};
use caskade_discovery::DetectedGame;
use caskade_installer::{
    ArtifactId, Candidate, Fidelity, HandlerId, Requirements, SourcePath, Step, Strategy,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
use url::Url;

/// Games one scan may return.
const SCAN_MAX: usize = 20_000;
/// The largest provider answer read.
const ANSWER_MAX: usize = 16 << 20;
/// Steps one install plan may have.
const STEPS_MAX: usize = 4096;
/// Downloads one URL may resolve to.
const DOWNLOADS_MAX: usize = 64;

type Answer = (Result<String, GuestError>,);

/// Metadata a provider found (§4.3 `metadata-provider`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Metadata {
    pub description: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    pub release_date: Option<String>,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    /// Artwork to fetch: `{ "kind": "cover" | "hero" | …, "url": "…" }`.
    #[serde(default)]
    pub artwork: Vec<ArtworkUrl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkUrl {
    pub kind: String,
    pub url: String,
}

/// What an install handler is told about its input (#55).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InputRef<'a> {
    artifact_id: &'a str,
    /// The file name only; the plugin never sees host paths.
    name: String,
    size: Option<u64>,
    format: &'a caskade_installer::Format,
    facts: &'a BTreeMap<String, serde_json::Value>,
}

/// An install handler's answer.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanAnswer {
    steps: Vec<Step>,
    #[serde(default)]
    confidence: Option<f32>,
}

/// One download a URL resolver found.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Download {
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    sha256: Option<String>,
}

/// Only steps that describe files: no program runs, nothing is mounted or
/// delegated, and file sources are the plugin's own input.
fn allowed_steps(id: &str, input: &ArtifactId, steps: &[Step]) -> Result<(), HostError> {
    if steps.is_empty() || steps.len() > STEPS_MAX {
        return Err(bad_answer(id, "a plan needs 1 to 4096 steps"));
    }
    for step in steps {
        let allowed = match step {
            Step::Extract { .. }
            | Step::WriteFile { .. }
            | Step::SetExecutable { .. }
            | Step::Symlink { .. }
            | Step::Copy {
                from: SourcePath::Game(_),
                ..
            } => true,
            Step::CopyRange { from, .. }
            | Step::Copy {
                from: SourcePath::Artifact { artifact: from, .. },
                ..
            } => from == input,
            _ => false,
        };
        if !allowed {
            let kind = serde_json::to_value(step)
                .ok()
                .and_then(|value| value.get("type")?.as_str().map(str::to_owned))
                .unwrap_or_default();
            return Err(bad_answer(
                id,
                format!("an install plan may not use a `{kind}` step here"),
            ));
        }
    }
    Ok(())
}

/// `claims`, then `plan` when claimed, in one call with the input readable.
/// `None` when the plugin doesn't export the interface or the input isn't
/// its business.
async fn claim_and_plan(
    instance: &mut Instance,
    input: String,
) -> Result<Option<Answer>, CallError> {
    let claimed = instance
        .provider::<(String,), (bool,)>("install-handler", "claims", (input.clone(),))
        .await?;
    if claimed != Some((true,)) {
        return Ok(None);
    }
    instance
        .provider::<(String,), Answer>("install-handler", "plan", (input,))
        .await
}

/// What a metadata provider is told about the game.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameRef<'a> {
    id: &'a str,
    title: &'a str,
    platforms: &'a [String],
}

fn denied(id: &str, message: &str) -> HostError {
    HostError::Plugin {
        id: id.to_owned(),
        code: "permission_denied".into(),
        message: message.into(),
    }
}

fn bad_answer(id: &str, error: impl std::fmt::Display) -> HostError {
    HostError::Plugin {
        id: id.to_owned(),
        code: "bad_answer".into(),
        message: error.to_string(),
    }
}

impl PluginHost {
    /// Calls a provider export; an error when the plugin doesn't export it.
    async fn provide<P, R>(
        &self,
        id: &str,
        interface: &str,
        func: &str,
        params: P,
    ) -> Result<R, HostError>
    where
        P: wasmtime::component::ComponentNamedList
            + wasmtime::component::Lower
            + Send
            + Sync
            + 'static,
        R: wasmtime::component::ComponentNamedList
            + wasmtime::component::Lift
            + Send
            + Sync
            + 'static,
    {
        self.call(id, async |instance| {
            instance.provider::<P, R>(interface, func, params).await
        })
        .await?
        .ok_or_else(|| HostError::Plugin {
            id: id.to_owned(),
            code: "not_a_provider".into(),
            message: format!("the plugin doesn't export {interface}"),
        })
    }

    fn text(id: &str, (answer,): Answer) -> Result<String, HostError> {
        let text = Self::answered(id, answer)?;
        if text.len() > ANSWER_MAX {
            return Err(bad_answer(id, "the answer is larger than 16 MiB"));
        }
        Ok(text)
    }

    /// `library-source.scan`; needs `library: write` (it adds games).
    pub async fn library_scan(&self, id: &str) -> Result<Vec<DetectedGame>, HostError> {
        if self.granted(id).await.library != LibraryAccess::Write {
            return Err(denied(
                id,
                "a library source needs the library: write permission",
            ));
        }
        let text = Self::text(id, self.provide(id, "library-source", "scan", ()).await?)?;
        let games: Vec<DetectedGame> =
            serde_json::from_str(&text).map_err(|error| bad_answer(id, error))?;
        if games.len() > SCAN_MAX {
            return Err(bad_answer(id, "the scan returned more than 20,000 games"));
        }
        Ok(games)
    }

    /// `metadata-provider.lookup` for `fields`; needs `network`.
    pub async fn metadata_lookup(
        &self,
        id: &str,
        game: &Game,
        fields: &[String],
    ) -> Result<Metadata, HostError> {
        let _activity = caskade_jobs::global().activity("Updating metadata");
        if self.granted(id).await.network.is_empty() {
            return Err(denied(
                id,
                "a metadata provider needs the network permission",
            ));
        }
        let reference = serde_json::to_string(&GameRef {
            id: &game.id,
            title: &game.title,
            platforms: &game.platforms,
        })
        .map_err(|error| bad_answer(id, error))?;
        let text = Self::text(
            id,
            self.provide(
                id,
                "metadata-provider",
                "lookup",
                (reference, fields.to_vec()),
            )
            .await?,
        )?;
        serde_json::from_str(&text).map_err(|error| bad_answer(id, error))
    }

    /// Fills `game_id`'s empty fields from the provider and saves them.
    /// Artwork URLs are returned for the artwork cache to fetch.
    pub async fn apply_metadata(
        &self,
        id: &str,
        game_id: &str,
        fields: &[String],
    ) -> Result<(Game, Vec<ArtworkUrl>), HostError> {
        let store = &self.services().store;
        let mut game = store
            .game(game_id)
            .await?
            .ok_or_else(|| HostError::Unknown(format!("game {game_id}")))?;
        let found = self.metadata_lookup(id, &game, fields).await?;
        if game.description.trim().is_empty()
            && let Some(description) = found.description
        {
            game.description = description;
        }
        if game.genres.is_empty() {
            game.genres = found.genres;
        }
        if game.release_date.is_none() {
            game.release_date = found.release_date;
        }
        let game = store.upsert_game(&game).await?;
        if let Some(bus) = &self.services().bus {
            bus.emit(caskade_core::bus::Event::LibraryGameUpdated {
                game_id: game.id.clone(),
            });
        }
        Ok((game, found.artwork))
    }

    /// `runner.prepare`; needs `process` (it decides what runs).
    pub async fn runner_command(
        &self,
        id: &str,
        runner_id: &str,
        payload: &serde_json::Value,
    ) -> Result<LaunchCommand, HostError> {
        if !self.granted(id).await.process {
            return Err(denied(id, "a runner needs the process permission"));
        }
        let spec = serde_json::json!({ "runnerId": runner_id, "payload": payload }).to_string();
        let text = Self::text(id, self.provide(id, "runner", "prepare", (spec,)).await?)?;
        let command: LaunchCommand =
            serde_json::from_str(&text).map_err(|error| bad_answer(id, error))?;
        command.validate().map_err(|error| bad_answer(id, error))?;
        Ok(command)
    }

    /// `install-handler`: a candidate for `input`, or `None` when the
    /// plugin doesn't claim it. The plugin reads the input only during this
    /// call, through `host.install-input-read`.
    pub async fn install_plan(
        &self,
        id: &str,
        input: &InstallInput,
    ) -> Result<Option<Candidate>, HostError> {
        let artifact = &input.artifact;
        let described = serde_json::to_string(&InputRef {
            artifact_id: &artifact.id.0,
            name: artifact
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: artifact.size,
            format: &artifact.format,
            facts: &artifact.facts,
        })
        .map_err(|error| bad_answer(id, error))?;
        let path = input.path.clone();
        let answer = self
            .call(id, async |instance| {
                instance.set_install_input(Some(path));
                let answer = claim_and_plan(instance, described).await;
                instance.set_install_input(None);
                answer
            })
            .await?;
        let Some(answer) = answer else {
            return Ok(None);
        };
        let plan: PlanAnswer = serde_json::from_str(&Self::text(id, answer)?)
            .map_err(|error| bad_answer(id, error))?;
        allowed_steps(id, &artifact.id, &plan.steps)?;
        let confidence = plan.confidence.unwrap_or(0.8);
        if !(0.0..=1.0).contains(&confidence) {
            return Err(bad_answer(id, "confidence must be between 0 and 1"));
        }
        Ok(Some(Candidate {
            handler: HandlerId(format!("{PLUGIN_SOURCE_PREFIX}{id}")),
            strategy: Strategy::Extract,
            confidence,
            fidelity: Fidelity::Exact,
            requires: Requirements {
                executes_code: false,
                runtime: None,
                may_prompt_elevation: false,
                interactive: false,
                est_disk_bytes: None,
            },
            steps: plan.steps,
            warnings: vec![],
        }))
    }

    /// `url-resolver.resolve`; needs `network`.
    pub async fn url_resolve(
        &self,
        id: &str,
        url: &Url,
    ) -> Result<Vec<DownloadRequest>, HostError> {
        let network = self.granted(id).await.network;
        if network.is_empty() {
            return Err(denied(id, "a URL resolver needs the network permission"));
        }
        let text = Self::text(
            id,
            self.provide(id, "url-resolver", "resolve", (url.to_string(),))
                .await?,
        )?;
        let found: Vec<Download> =
            serde_json::from_str(&text).map_err(|error| bad_answer(id, error))?;
        if found.len() > DOWNLOADS_MAX {
            return Err(bad_answer(id, "more than 64 downloads"));
        }
        found
            .into_iter()
            .map(|download| {
                let url = Url::parse(&download.url).map_err(|error| bad_answer(id, error))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(bad_answer(id, "downloads must be http or https"));
                }
                // The installer fetches these, and the plugin's install
                // handler can read what it fetched: a download elsewhere
                // would get round the network allowlist (#157).
                if !url
                    .host_str()
                    .is_some_and(|host| host_allowed(&network, host))
                {
                    return Err(bad_answer(
                        id,
                        format!(
                            "{} isn't a host the plugin may reach",
                            url.host_str().unwrap_or("")
                        ),
                    ));
                }
                if download.headers.len() > 32 {
                    return Err(bad_answer(id, "more than 32 download headers"));
                }
                Ok(DownloadRequest {
                    url,
                    headers: download.headers,
                    file_name: download.file_name,
                    size: download.size,
                    sha256: download.sha256,
                    // Redirects and DNS answers are checked against the
                    // grant too, while the installer fetches it (CSK-03).
                    allowed_hosts: Some(network.clone()),
                })
            })
            .collect()
    }

    /// Registers the providers every valid plugin declares in
    /// `contributes.providers` on the bus. The adapters check the plugin
    /// is enabled and granted on every call, so this runs once at startup.
    pub fn register_providers(&self, bus: &Bus) {
        for (id, providers) in self.providers() {
            let source = format!("{PLUGIN_SOURCE_PREFIX}{id}");
            for provider in providers {
                let adapter = Adapter {
                    host: self.clone(),
                    plugin: id.clone(),
                    source: source.clone(),
                };
                match provider.as_str() {
                    "library-source" => bus.register_library_source(Arc::new(adapter)),
                    "install-handler" => bus.register_install_handler(Arc::new(adapter)),
                    "url-resolver" => bus.register_url_resolver(Arc::new(adapter)),
                    // Runners are reached through `PluginRunners`; metadata
                    // providers through `apply_metadata`.
                    _ => {}
                }
            }
        }
    }
}

/// One plugin's provider, as a core trait object.
struct Adapter {
    host: PluginHost,
    plugin: String,
    source: String,
}

#[async_trait]
impl LibrarySource for Adapter {
    fn id(&self) -> &str {
        &self.source
    }

    async fn scan(&self) -> Result<Vec<DetectedGame>, String> {
        if !self.host.enabled(&self.plugin) {
            return Ok(Vec::new());
        }
        self.host
            .library_scan(&self.plugin)
            .await
            .map_err(|error| error.to_string())
    }
}

#[async_trait]
impl InstallHandler for Adapter {
    fn id(&self) -> &str {
        &self.source
    }

    async fn propose(&self, input: &InstallInput) -> Result<Option<Candidate>, String> {
        if !self.host.enabled(&self.plugin) {
            return Ok(None);
        }
        self.host
            .install_plan(&self.plugin, input)
            .await
            .map_err(|error| error.to_string())
    }
}

#[async_trait]
impl UrlResolver for Adapter {
    fn id(&self) -> &str {
        &self.source
    }

    /// A resolver only sees pages on hosts it was granted.
    fn matches(&self, url: &Url) -> bool {
        self.host.enabled(&self.plugin)
            && self.host.manifest(&self.plugin).is_some_and(|manifest| {
                host_allowed(
                    &manifest.permissions.network,
                    url.host_str().unwrap_or_default(),
                )
            })
    }

    async fn resolve(&self, url: &Url) -> Result<Vec<DownloadRequest>, String> {
        self.host
            .url_resolve(&self.plugin, url)
            .await
            .map_err(|error| error.to_string())
    }
}

/// Launches `LaunchSpec::Plugin` entries through the plugin's runner; the
/// launch pipeline spawns and tracks the command (`LaunchService::with_plugins`).
/// The launch service is built before the plugin host, so the host is
/// attached later.
#[derive(Clone, Default)]
pub struct PluginRunners {
    host: Arc<OnceLock<PluginHost>>,
}

impl PluginRunners {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn of(host: PluginHost) -> Self {
        let runners = Self::new();
        runners.attach(host);
        runners
    }

    /// Connects the plugin host once it exists; later calls are ignored.
    pub fn attach(&self, host: PluginHost) {
        let _ = self.host.set(host);
    }
}

#[async_trait]
impl PluginRunner for PluginRunners {
    async fn resolve(
        &self,
        plugin_id: &str,
        runner_id: &str,
        payload: &serde_json::Value,
    ) -> caskade_core::launch::Result<LaunchCommand> {
        let host = self.host.get().ok_or_else(|| {
            caskade_core::launch::LaunchError::Invalid("plugins aren't loaded yet".into())
        })?;
        if !host.enabled(plugin_id) {
            return Err(caskade_core::launch::LaunchError::Invalid(format!(
                "the plugin {plugin_id} is disabled"
            )));
        }
        host.runner_command(plugin_id, runner_id, payload)
            .await
            .map_err(|error| caskade_core::launch::LaunchError::Invalid(error.to_string()))
    }
}

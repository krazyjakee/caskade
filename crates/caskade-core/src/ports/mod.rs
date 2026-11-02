//! Fan ports of games: decompilations, static recompilations and source
//! ports that run a game the user owns natively, from their own game file.
//!
//! The list is [Decomp Buddy](https://github.com/TanookiStudios/decomp-buddy)'s
//! published install plans (`plans.json`), fetched again daily when it
//! changed (ETag) and kept for offline use under `<cache>/ports/`. Nothing
//! ships with Caskade. A library game lists the projects for its system
//! whose game it is: the same IGDB game through the Hasheous dumps on the
//! device, or the same title. [`Ports::install`] installs one as another
//! release of the game (`install::AttachTo`): the release file the plan
//! names for this system, checked against the SHA-256 the catalogue
//! scanned, with the user's game file placed or passed as the plan says.

mod catalogue;
pub mod consoles;
mod install;
#[cfg(test)]
mod tests;
mod title;

pub use catalogue::{Catalogue, GameFile, Host, PortGame, Project, Target, parse};

use crate::{
    Error, Result,
    db::LibraryStore,
    hasheous::Hasheous,
    model::{GameEntry, LaunchSpec},
    remote_file,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Decomp Buddy's published plans.
pub const CATALOGUE_URL: &str = "https://tanookistudios.com/decomp-buddy/plans.json";
/// Where the plans come from, credited where they show.
pub const CREDIT_URL: &str = "https://github.com/TanookiStudios/decomp-buddy";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY: Duration = Duration::from_secs(60);
const MAX_RETRY: Duration = Duration::from_secs(15 * 60);

/// `core.ports`: whether game pages list ports, and where the plans come
/// from.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortsSettings {
    #[schemars(title = "Show ports of games")]
    /// List fan decompilations, recompilations and source ports on game
    /// pages, from Decomp Buddy's install plans.
    pub enabled: bool,
    #[schemars(
        title = "Port catalogue URL",
        regex(pattern = r"^https?://[^\s]+$"),
        length(max = 4096)
    )]
    /// Download the install plans of ports from this URL.
    pub catalogue_url: String,
}

impl Default for PortsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            catalogue_url: CATALOGUE_URL.into(),
        }
    }
}

/// How sure a port is the game's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PortMatch {
    /// The same IGDB game, through the Hasheous dumps.
    Igdb,
    /// The same title on the same system.
    Title,
}

/// What this device can do with a port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum PortHere {
    /// A release file for this system: installs in one click.
    Installable {
        asset: String,
        release: String,
        /// The catalogue gives the file's SHA-256, checked on download.
        checked: bool,
    },
    /// This system needs it built from source.
    BuildFromSource,
    /// Not for this system.
    OtherSystems,
}

/// A file the port wants from the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortNeeds {
    pub label: String,
    /// `game`, `bios`, `dlc`, `update` or `other`.
    pub kind: String,
    /// `USA`, `Europe`, `Japan`; `None` when any or unknown.
    pub region: Option<String>,
    /// `inAppPicker` (the port asks for it on first start), `folder`
    /// (installing copies it in) or `commandLine` (passed when it starts).
    pub how: PortFileHow,
    /// The library has a file of the game to give it.
    pub have: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PortFileHow {
    InAppPicker,
    Folder,
    CommandLine,
}

/// A port of a library game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Port {
    /// The catalogue's key, e.g. `github:zelda64recomp/zelda64recomp`.
    pub project: String,
    pub name: String,
    pub owner: String,
    pub url: String,
    /// The game as the plan names it.
    pub game_title: String,
    pub console: String,
    pub matched_by: PortMatch,
    /// Systems it has a release file for: `windows`, `linux`, `macos-arm64`.
    pub runs_on: Vec<String>,
    pub here: PortHere,
    pub needs: Vec<PortNeeds>,
    pub notes: Vec<String>,
    /// What the plan's author wasn't sure of.
    pub unsure: Vec<String>,
    /// The installed release of it, when there is one.
    pub entry_id: Option<String>,
}

/// The catalogue's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortsStatus {
    pub enabled: bool,
    pub available: bool,
    pub projects: u32,
    pub checked_at_ms: Option<i64>,
    pub credit_url: String,
}

/// A game's ports and where they come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GamePorts {
    pub status: PortsStatus,
    pub ports: Vec<Port>,
}

/// What a refresh did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    Offline,
    Fresh,
    Unchanged,
    Updated { projects: u32 },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Cached {
    url: String,
    etag: Option<String>,
    /// Seconds since the epoch of the last check that worked.
    checked: u64,
}

/// The ports catalogue; cheap to clone.
#[derive(Clone)]
pub struct Ports {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    url: Mutex<String>,
    enabled: Mutex<bool>,
    loaded: tokio::sync::Mutex<Option<Arc<Catalogue>>>,
    refresh: tokio::sync::Mutex<()>,
    /// Plan titles' IGDB ids through the Hasheous dumps, until the next
    /// catalogue.
    bridge: Mutex<HashMap<String, Vec<u64>>>,
}

impl Ports {
    /// `dir` is `<cache>/ports/`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_url(dir, CATALOGUE_URL)
    }

    /// Plans from `url`: the `core.ports.catalogueUrl` setting, or a test
    /// server.
    pub fn with_url(dir: impl Into<PathBuf>, url: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                dir: dir.into(),
                url: Mutex::new(url.into()),
                enabled: Mutex::new(true),
                loaded: tokio::sync::Mutex::new(None),
                refresh: tokio::sync::Mutex::new(()),
                bridge: Mutex::default(),
            }),
        }
    }

    fn url(&self) -> String {
        crate::lock(&self.inner.url).clone()
    }

    fn enabled(&self) -> bool {
        *crate::lock(&self.inner.enabled)
    }

    fn plans_path(&self) -> PathBuf {
        self.inner.dir.join("plans.json")
    }

    fn state_path(&self) -> PathBuf {
        self.inner.dir.join("state.json")
    }

    async fn cached(&self) -> Option<Cached> {
        let bytes = tokio::fs::read(self.state_path()).await.ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// The catalogue on disk, parsed once until it changes.
    pub async fn catalogue(&self) -> Result<Option<Arc<Catalogue>>> {
        let mut loaded = self.inner.loaded.lock().await;
        if let Some(catalogue) = &*loaded {
            return Ok(Some(catalogue.clone()));
        }
        let bytes = match tokio::fs::read(self.plans_path()).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let catalogue = tokio::task::spawn_blocking(move || parse(&bytes))
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let catalogue = Arc::new(catalogue);
        *loaded = Some(catalogue.clone());
        Ok(Some(catalogue))
    }

    pub async fn status(&self) -> Result<PortsStatus> {
        let catalogue = self.catalogue().await.ok().flatten();
        Ok(PortsStatus {
            enabled: self.enabled(),
            available: catalogue.is_some(),
            projects: catalogue.map_or(0, |catalogue| catalogue.projects.len() as u32),
            checked_at_ms: self
                .cached()
                .await
                .map(|cached| (cached.checked as i64).saturating_mul(1000)),
            credit_url: CREDIT_URL.into(),
        })
    }

    /// Fetches the plans when online and a day after the last check that
    /// worked, or at once when the URL changed. A failure keeps the plans
    /// on disk.
    pub async fn refresh(&self, online: bool) -> Result<Refresh> {
        if !online {
            return Ok(Refresh::Offline);
        }
        let _refresh = self.inner.refresh.lock().await;
        let now = crate::now_ms().max(0) as u64 / 1000;
        let url = self.url();
        let cached = self.cached().await;
        let have = tokio::fs::try_exists(self.plans_path()).await?;
        let same = cached.as_ref().is_some_and(|cached| cached.url == url) && have;
        if same
            && cached
                .as_ref()
                .is_some_and(|cached| now >= cached.checked && now - cached.checked < DAY.as_secs())
        {
            return Ok(Refresh::Fresh);
        }
        let etag = cached
            .as_ref()
            .filter(|_| same)
            .and_then(|cached| cached.etag.clone());
        match remote_file::fetch(&url, etag.as_deref(), "Port catalogue").await? {
            remote_file::Response::Unchanged => {
                let cached = Cached {
                    url,
                    etag,
                    checked: now,
                };
                self.write_state(&cached).await?;
                Ok(Refresh::Unchanged)
            }
            remote_file::Response::Changed { bytes, etag } => {
                let (catalogue, bytes) = tokio::task::spawn_blocking(move || {
                    parse(&bytes).map(|catalogue| (catalogue, bytes))
                })
                .await
                .map_err(|error| Error::Invalid(error.to_string()))?
                .map_err(|error| Error::Invalid(error.to_string()))?;
                let projects = catalogue.projects.len() as u32;
                tokio::fs::create_dir_all(&self.inner.dir).await?;
                write_atomically(&self.plans_path(), &bytes).await?;
                self.write_state(&Cached {
                    url,
                    etag,
                    checked: now,
                })
                .await?;
                *self.inner.loaded.lock().await = Some(Arc::new(catalogue));
                crate::lock(&self.inner.bridge).clear();
                Ok(Refresh::Updated { projects })
            }
        }
    }

    async fn write_state(&self, cached: &Cached) -> Result<()> {
        tokio::fs::create_dir_all(&self.inner.dir).await?;
        write_atomically(&self.state_path(), &serde_json::to_vec(cached)?).await
    }

    /// Runs under the app's online supervisor, following `core.ports`:
    /// nothing is fetched while it is off. Failures retry after 1, 2, 4, …
    /// minutes, up to 15.
    pub fn watch_settings(
        &self,
        settings: crate::settings::SettingsService,
    ) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move {
            let mut retry = RETRY;
            loop {
                let chosen = settings
                    .read_or_default::<PortsSettings>("core.ports")
                    .await;
                *crate::lock(&this.inner.enabled) = chosen.enabled;
                *crate::lock(&this.inner.url) = chosen.catalogue_url;
                let delay = if !chosen.enabled {
                    RETRY
                } else {
                    match this.refresh(true).await {
                        Ok(_) => {
                            retry = RETRY;
                            RETRY
                        }
                        Err(error) => {
                            tracing::warn!(%error, "port catalogue refresh failed; keeping the last one");
                            let delay = retry;
                            retry = (retry * 2).min(MAX_RETRY);
                            delay
                        }
                    }
                };
                tokio::time::sleep(delay).await;
            }
        })
    }

    /// The ports of library game `game_id`, surest first.
    pub async fn for_game(
        &self,
        store: &LibraryStore,
        hasheous: Option<&Hasheous>,
        game_id: &str,
    ) -> Result<GamePorts> {
        let status = self.status().await?;
        if !status.enabled {
            return Ok(GamePorts {
                status,
                ports: Vec::new(),
            });
        }
        let Some(catalogue) = self.catalogue().await? else {
            return Ok(GamePorts {
                status,
                ports: Vec::new(),
            });
        };
        let Some(game) = store.game(game_id).await? else {
            return Err(Error::Invalid(crate::tr!(
                "This game is no longer in the library."
            )));
        };
        let entries = store.entries_for_game(game_id).await?;
        let library = LibraryGame::new(&game, &entries);
        let igdb = store
            .igdb_match(game_id)
            .await?
            .and_then(|matched| matched.igdb_id);
        let updates = store.game_updates(game_id).await?;
        let mut ports = Vec::new();
        for project in &catalogue.projects {
            let mut best: Option<(PortMatch, &PortGame)> = None;
            for port in &project.games {
                if port.families.is_disjoint(&library.families) {
                    continue;
                }
                // Hasheous naming the port's game as another IGDB game rules
                // it out, whatever the titles say (Doom and Doom).
                let known = match (igdb, hasheous) {
                    (Some(_), Some(hasheous)) => self.igdb_ids(hasheous, port).await,
                    _ => Vec::new(),
                };
                let found = match igdb {
                    Some(igdb) if !known.is_empty() => {
                        known.contains(&igdb).then_some(PortMatch::Igdb)
                    }
                    _ => library.names(port).then_some(PortMatch::Title),
                };
                if let Some(found) = found
                    && best.is_none_or(|(seen, _)| found < seen)
                {
                    best = Some((found, port));
                }
            }
            if let Some((matched_by, port)) = best {
                let entry_id = updates
                    .iter()
                    .find(|row| {
                        row.provider == "github"
                            && project.host == Host::GitHub
                            && row.project.eq_ignore_ascii_case(&format!(
                                "{}/{}",
                                project.owner, project.repo
                            ))
                    })
                    .map(|row| row.entry_id.clone());
                ports.push(describe(project, port, matched_by, &library, entry_id));
            }
        }
        ports.sort_by(|a, b| {
            a.matched_by
                .cmp(&b.matched_by)
                .then_with(|| installable(b).cmp(&installable(a)))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(GamePorts { status, ports })
    }

    /// The IGDB games the Hasheous dumps of the port's system name its game
    /// as; empty when they don't know it.
    async fn igdb_ids(&self, hasheous: &Hasheous, port: &PortGame) -> Vec<u64> {
        let dumps: Vec<String> = port
            .families
            .iter()
            .flat_map(|family| consoles::hasheous_dumps(family).iter())
            .map(|dump| (*dump).to_owned())
            .collect();
        if dumps.is_empty() {
            return Vec::new();
        }
        let key = format!("{}\u{1f}{}", port.title, dumps.join("\u{1f}"));
        let known = crate::lock(&self.inner.bridge).get(&key).cloned();
        match known {
            Some(ids) => ids,
            None => {
                let mut ids = Vec::new();
                for spelling in spellings(&port.title) {
                    let Ok(games) = hasheous.search(&spelling, &dumps, 8).await else {
                        continue;
                    };
                    ids.extend(games.iter().flat_map(|game| {
                        game.metadata
                            .iter()
                            .filter(|id| id.source == "IGDB")
                            .filter_map(|id| id.id.parse::<u64>().ok())
                    }));
                    if !ids.is_empty() {
                        break;
                    }
                }
                ids.sort_unstable();
                ids.dedup();
                crate::lock(&self.inner.bridge).insert(key, ids.clone());
                ids
            }
        }
    }
}

/// How Hasheous may spell a catalogue title: as given, with " - " for ": ",
/// and No-Intro's "Legend of Zelda, The - …".
fn spellings(title: &str) -> Vec<String> {
    let dashed = title.replacen(": ", " - ", 1);
    let mut out = vec![title.to_owned(), dashed.clone()];
    if let Some(rest) = dashed.strip_prefix("The ") {
        out.push(match rest.split_once(" - ") {
            Some((head, tail)) => format!("{head}, The - {tail}"),
            None => format!("{rest}, The"),
        });
    }
    out.dedup();
    out
}

/// What matching knows about a library game.
struct LibraryGame {
    /// Whole titles: the title, sort title and aliases.
    names: BTreeSet<String>,
    /// Halves of "Series: Subtitle" titles.
    halves: BTreeSet<String>,
    families: BTreeSet<String>,
    /// The game's ROMs, by family.
    roms: Vec<(String, crate::emulation::GameRom)>,
}

impl LibraryGame {
    fn new(game: &crate::model::Game, entries: &[GameEntry]) -> Self {
        let mut names = BTreeSet::new();
        let mut halves = BTreeSet::new();
        for name in std::iter::once(&game.title)
            .chain(std::iter::once(&game.sort_title))
            .chain(&game.aliases)
        {
            let key = title::key(name);
            if !key.is_empty() {
                names.insert(key);
            }
            title::halves(name, &mut halves);
        }
        let mut families: BTreeSet<String> = game
            .platforms
            .iter()
            .flat_map(|platform| consoles::family_of_platform(platform))
            .collect();
        let mut roms = Vec::new();
        for entry in entries {
            match &entry.launch {
                Some(LaunchSpec::Emulator {
                    platform_id,
                    roms: files,
                    ..
                }) => {
                    let family = consoles::family_of_platform(platform_id);
                    for family in &family {
                        roms.extend(files.iter().map(|rom| (family.clone(), rom.clone())));
                    }
                    families.extend(family);
                }
                Some(LaunchSpec::Web { .. }) => {}
                // A store's, a launcher's or an install's game runs on the PC.
                _ if entry.source_id != crate::install::HANDOFF_SOURCE => {
                    families.insert(consoles::PC.to_owned());
                }
                _ => {}
            }
        }
        Self {
            names,
            halves,
            families,
            roms,
        }
    }

    /// The port's game names this game: a whole title of either matches a
    /// whole title or a half of this one's, or a half of the port's matches
    /// a whole title of this one's.
    fn names(&self, port: &PortGame) -> bool {
        port.keys
            .iter()
            .any(|key| self.names.contains(key) || self.halves.contains(key))
            || port.parts.iter().any(|part| self.names.contains(part))
    }

    fn has_rom(&self, families: &BTreeSet<String>) -> bool {
        self.roms
            .iter()
            .any(|(family, _)| families.contains(family))
    }
}

/// The catalogue's name of this device's system, when it has ports for it.
pub fn this_system() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("windows")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64")
    } else {
        None
    }
}

/// The plan for this system, or the first one for describing the port.
fn target_here(port: &PortGame) -> Option<&Target> {
    this_system().and_then(|system| port.targets.get(system))
}

fn describe(
    project: &Project,
    port: &PortGame,
    matched_by: PortMatch,
    library: &LibraryGame,
    entry_id: Option<String>,
) -> Port {
    let here = target_here(port);
    let shown = here.or_else(|| port.targets.values().next());
    let state = match here {
        Some(target) if target.installable() && project.host == Host::GitHub => {
            PortHere::Installable {
                asset: target.asset.clone().unwrap_or_default(),
                release: target.tag.clone().unwrap_or_default(),
                checked: target.sha256.is_some(),
            }
        }
        Some(target) if target.method == "source" => PortHere::BuildFromSource,
        _ => PortHere::OtherSystems,
    };
    let runs_on = port
        .targets
        .iter()
        .filter(|(system, target)| {
            target.installable() && ["windows", "linux", "macos-arm64"].contains(&system.as_str())
        })
        .map(|(system, _)| system.clone())
        .collect();
    let have = library.has_rom(&port.families);
    Port {
        project: project.key.clone(),
        name: project.repo.clone(),
        owner: project.owner.clone(),
        url: project.url(),
        game_title: port.title.clone(),
        console: port.console.clone(),
        matched_by,
        runs_on,
        here: state,
        needs: shown
            .map(|target| {
                target
                    .files
                    .iter()
                    .map(|file| PortNeeds {
                        label: file.label.clone(),
                        kind: file.kind.clone(),
                        region: file
                            .region
                            .clone()
                            .filter(|region| !matches!(region.as_str(), "any" | "unknown")),
                        how: how(file),
                        have: file.kind == "game" && have,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        notes: shown.map(|target| target.notes.clone()).unwrap_or_default(),
        unsure: shown
            .map(|target| target.unsure.clone())
            .unwrap_or_default(),
        entry_id,
    }
}

fn how(file: &GameFile) -> PortFileHow {
    match file.how.as_str() {
        "folder" => PortFileHow::Folder,
        "command_line" => PortFileHow::CommandLine,
        _ => PortFileHow::InAppPicker,
    }
}

fn installable(port: &Port) -> bool {
    matches!(port.here, PortHere::Installable { .. })
}

async fn write_atomically(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let partial = path.with_extension("partial");
    tokio::fs::write(&partial, bytes).await?;
    tokio::fs::rename(&partial, path).await?;
    Ok(())
}

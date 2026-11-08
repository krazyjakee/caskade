//! The library's games on the host (#259): every
//! installed, launchable game the host's active account may play is an app
//! named for it, with its cover as a PNG, that runs `caskade --launch <game
//! id>` (#256) and lasts as long as the game. One more app, "Caskade",
//! starts Caskade in console mode, so a client can browse with a pad.
//!
//! An app is Caskade's when its command (or the console app's detached
//! command) runs this Caskade; on a user's own Sunshine (#258) those are
//! the only apps Caskade adds or removes. Changes are applied in batches,
//! a moment after the library changes, never per row.

use super::{
    StreamingHost,
    api::{App, HostApi},
};
use crate::{
    Result,
    accounts::AccountService,
    artwork::ArtworkCache,
    bus::{Bus, Event},
    db::LibraryStore,
    model::{ArtworkKind, GameEntry, GameFilter, InstallState, LaunchSpec, ListGames},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

/// The console app's name.
pub const CONSOLE_APP: &str = "Caskade";
/// Box art as clients show it.
const COVER: (u32, u32) = (600, 800);

/// What Caskade puts on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Game {
        title: String,
        game_id: String,
        /// The cover, as a PNG on this device.
        image: Option<PathBuf>,
    },
    /// Caskade itself, in console mode.
    Console,
}

/// How commands reach Caskade: directly, or out of a Flatpak host's
/// sandbox (`flatpak-spawn --host`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caskade {
    pub program: PathBuf,
    pub flatpak_host: bool,
}

impl Caskade {
    fn prefix(&self) -> String {
        let program = format!("\"{}\"", self.program.display());
        if self.flatpak_host {
            format!("flatpak-spawn --host {program}")
        } else {
            program
        }
    }

    /// The command that plays `game_id`.
    pub fn launch_command(&self, game_id: &str) -> String {
        format!("{} --launch {game_id}", self.prefix())
    }

    /// The console app's command.
    pub fn console_command(&self) -> String {
        format!("{} --console", self.prefix())
    }

    /// Which of Caskade's apps `app` is, if it is one.
    pub fn owns(&self, app: &App) -> Option<Offer> {
        if let Some(game) = app
            .cmd
            .as_deref()
            .and_then(|cmd| cmd.strip_prefix(&format!("{} --launch ", self.prefix())))
            .filter(|game| !game.is_empty() && !game.contains(char::is_whitespace))
        {
            return Some(Offer::Game {
                title: app.name.clone(),
                game_id: game.to_owned(),
                image: None,
            });
        }
        let console = self.console_command();
        let detached = app
            .rest
            .get("detached")
            .and_then(serde_json::Value::as_array);
        detached
            .is_some_and(|commands| {
                commands
                    .iter()
                    .any(|cmd| cmd.as_str() == Some(console.as_str()))
            })
            .then_some(Offer::Console)
    }

    /// The app for `offer`.
    pub fn app(&self, offer: &Offer) -> App {
        match offer {
            Offer::Game {
                title,
                game_id,
                image,
            } => {
                let mut app = App::command(title, &self.launch_command(game_id));
                if let Some(image) = image {
                    app.rest.insert(
                        "image-path".into(),
                        image.to_string_lossy().into_owned().into(),
                    );
                }
                app
            }
            Offer::Console => {
                let mut app = App::command(CONSOLE_APP, "");
                app.cmd = None;
                app.rest.insert(
                    "detached".into(),
                    serde_json::json!([self.console_command()]),
                );
                app
            }
        }
    }
}

/// Whether two offers are the same app (a cover change aside).
fn same(a: &Offer, b: &Offer) -> bool {
    match (a, b) {
        (
            Offer::Game {
                title: ta,
                game_id: ga,
                ..
            },
            Offer::Game {
                title: tb,
                game_id: gb,
                ..
            },
        ) => ta == tb && ga == gb,
        (Offer::Console, Offer::Console) => true,
        _ => false,
    }
}

/// What a sync changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Published {
    pub added: usize,
    pub removed: usize,
}

/// Makes Caskade's apps on the host exactly `offers`: missing ones added,
/// ones no longer offered (or renamed) removed. Other apps stay as they
/// are. Indexes are read again after each removal: the host sorts its list.
pub async fn sync(api: &HostApi, caskade: &Caskade, offers: &[Offer]) -> Result<Published> {
    let mut published = Published::default();
    loop {
        let apps = api.apps().await?;
        let stale = apps.iter().position(|app| {
            caskade
                .owns(app)
                .is_some_and(|ours| !offers.iter().any(|offer| same(offer, &ours)))
        });
        let Some(index) = stale else { break };
        api.delete_app(index).await?;
        published.removed += 1;
    }
    let ours: Vec<Offer> = api
        .apps()
        .await?
        .iter()
        .filter_map(|app| caskade.owns(app))
        .collect();
    for offer in offers {
        if !ours.iter().any(|there| same(there, offer)) {
            api.add_app(&caskade.app(offer)).await?;
            published.added += 1;
        }
    }
    Ok(published)
}

/// Removes every one of Caskade's apps, and only those.
pub async fn withdraw(api: &HostApi, caskade: &Caskade) -> Result<usize> {
    Ok(sync(api, caskade, &[]).await?.removed)
}

/// Whether `entry` is a release Caskade starts on this device.
fn playable(entry: &GameEntry) -> bool {
    entry.state == InstallState::Installed
        && entry
            .launch
            .as_ref()
            .is_some_and(|launch| !matches!(launch, LaunchSpec::Plugin { .. }))
}

/// Where the publisher's apps go.
#[async_trait::async_trait]
pub trait Target: Send + Sync {
    /// Whether the host answers now.
    fn ready(&self) -> bool;
    async fn api(&self) -> Result<HostApi>;
    /// The host runs in a Flatpak sandbox: commands leave it.
    fn flatpak(&self) -> bool;
}

#[async_trait::async_trait]
impl Target for StreamingHost {
    fn ready(&self) -> bool {
        self.status().state == super::HostState::Ready
    }

    async fn api(&self) -> Result<HostApi> {
        StreamingHost::api(self).await
    }

    fn flatpak(&self) -> bool {
        self.host_is_flatpak()
    }
}

struct Inner {
    store: LibraryStore,
    target: Arc<dyn Target>,
    accounts: Option<AccountService>,
    artwork: Option<ArtworkCache>,
    program: PathBuf,
    covers: PathBuf,
    batch: Duration,
    /// Batches applied, for people and tests.
    batches: Mutex<u64>,
}

/// Keeps the host's apps in step with the library; cheap to clone.
#[derive(Clone)]
pub struct Publisher {
    inner: Arc<Inner>,
}

impl Publisher {
    pub fn new(
        store: LibraryStore,
        target: Arc<dyn Target>,
        accounts: Option<AccountService>,
        artwork: Option<ArtworkCache>,
        paths: &crate::AppPaths,
    ) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                target,
                accounts,
                artwork,
                program: std::env::current_exe()?,
                covers: paths.data.join("streaming").join("covers"),
                batch: Duration::from_secs(2),
                batches: Mutex::new(0),
            }),
        })
    }

    /// Another program path and batch delay (tests).
    pub fn with(self, program: PathBuf, batch: Duration) -> Self {
        let inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| panic!("with before the publisher is shared"));
        Self {
            inner: Arc::new(Inner {
                program,
                batch,
                ..inner
            }),
        }
    }

    /// How many batches went to the host.
    pub fn batches(&self) -> u64 {
        *crate::lock(&self.inner.batches)
    }

    fn caskade(&self) -> Caskade {
        Caskade {
            program: self.inner.program.clone(),
            flatpak_host: self.inner.target.flatpak(),
        }
    }

    /// Every game to offer now, by title, then the console app.
    pub async fn offers(&self) -> Result<Vec<Offer>> {
        let mut offers = Vec::new();
        let mut request = ListGames {
            filter: GameFilter::Installed,
            limit: 500,
            offset: 0,
            ..Default::default()
        };
        loop {
            let page = self.inner.store.list_games(&request).await?;
            let count = page.items.len();
            for game in page.items {
                let entries = self.inner.store.entries_for_game(&game.id).await?;
                if !entries.iter().any(playable) {
                    continue;
                }
                // The host's active account may play it (#209).
                if let Some(accounts) = &self.inner.accounts
                    && accounts.age_restriction(&game.id).await?.is_some()
                {
                    continue;
                }
                let image = self.cover(&game.id).await;
                offers.push(Offer::Game {
                    title: game.title,
                    game_id: game.id,
                    image,
                });
            }
            request.offset += count as u32;
            if count == 0 || u64::from(request.offset) >= page.total {
                break;
            }
        }
        offers.push(Offer::Console);
        Ok(offers)
    }

    /// The game's cover as a 600×800 PNG for clients, made once.
    async fn cover(&self, game_id: &str) -> Option<PathBuf> {
        let cache = self.inner.artwork.as_ref()?;
        let path = self.inner.covers.join(format!("{game_id}.png"));
        if path.is_file() {
            return Some(path);
        }
        let artwork = self.inner.store.shown_artwork(game_id).await.ok()?;
        let cover = artwork.iter().find(|art| art.kind == ArtworkKind::Cover)?;
        let served = crate::artwork::serve(cache, &format!("/{}", cover.cached_path)).await;
        if served.status != 200 {
            return None;
        }
        let covers = self.inner.covers.clone();
        let made = path.clone();
        tokio::task::spawn_blocking(move || -> Option<()> {
            let image = image::load_from_memory(&served.bytes).ok()?;
            let fitted = image.resize(COVER.0, COVER.1, image::imageops::FilterType::Lanczos3);
            std::fs::create_dir_all(&covers).ok()?;
            fitted.save_with_format(&made, image::ImageFormat::Png).ok()
        })
        .await
        .ok()??;
        Some(path)
    }

    /// Puts the library on the host now, when it answers.
    pub async fn publish(&self) -> Result<Option<Published>> {
        if !self.inner.target.ready() {
            return Ok(None);
        }
        let offers = self.offers().await?;
        let api = self.inner.target.api().await?;
        let published = sync(&api, &self.caskade(), &offers).await?;
        *crate::lock(&self.inner.batches) += 1;
        if published != Published::default() {
            tracing::info!(
                added = published.added,
                removed = published.removed,
                "games published to the streaming host"
            );
        }
        Ok(Some(published))
    }

    /// Removes Caskade's apps from the host (hosting off).
    pub async fn withdraw(&self) -> Result<usize> {
        let api = self.inner.target.api().await?;
        withdraw(&api, &self.caskade()).await
    }

    /// Publishes a moment after the library or the active account changes,
    /// once for many changes, and when the host comes up.
    pub fn watch(&self, bus: &Bus) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        let mut events = bus.stream();
        tokio::spawn(async move {
            let mut was_ready = false;
            let mut dirty = true;
            loop {
                let ready = this.inner.target.ready();
                if ready && !was_ready {
                    dirty = true;
                }
                was_ready = ready;
                if dirty && ready {
                    // Everything that arrives meanwhile joins this batch.
                    tokio::time::sleep(this.inner.batch).await;
                    while events.try_recv().is_ok() {}
                    dirty = false;
                    if let Err(error) = this.publish().await {
                        tracing::warn!(%error, "games not published to the streaming host");
                        dirty = true;
                    }
                    continue;
                }
                use tokio::sync::broadcast::error::RecvError;
                match tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
                    Ok(Ok(event)) => dirty |= relevant(&event),
                    Ok(Err(RecvError::Lagged(_))) => dirty = true,
                    Ok(Err(RecvError::Closed)) => return,
                    Err(_) => {}
                }
            }
        })
    }
}

/// Events after which the list may differ.
fn relevant(event: &Event) -> bool {
    matches!(
        event,
        Event::LibraryGameAdded { .. }
            | Event::LibraryGameUpdated { .. }
            | Event::LibraryGameRemoved { .. }
            | Event::LibraryScanCompleted { .. }
            | Event::InstallCompleted { .. }
            | Event::UninstallCompleted { .. }
            | Event::AccountChanged { .. }
    )
}

#[cfg(all(test, unix))]
mod tests;
